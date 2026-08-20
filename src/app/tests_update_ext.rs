use super::{App, AppError, CreateKnotOptions, KnotView, StateActorMetadata, UpdateKnotPatch};
use crate::domain::execution_plan::{
    ExecutionPlanAgent, ExecutionPlanData, ExecutionPlanKnot, ExecutionPlanStep, ExecutionPlanWave,
};
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntryInput;
use std::path::Path;
fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-app-test")
}
fn count_json_files(root: &Path) -> usize {
    if !root.exists() {
        return 0;
    }
    let mut c = 0usize;
    let mut d = vec![root.to_path_buf()];
    while let Some(dir) = d.pop() {
        for e in std::fs::read_dir(dir).expect("r") {
            let p = e.expect("r").path();
            if p.is_dir() {
                d.push(p);
            } else if p.extension().is_some_and(|x| x == "json") {
                c += 1;
            }
        }
    }
    c
}
fn empty_patch() -> UpdateKnotPatch {
    UpdateKnotPatch {
        title: None,
        description: None,
        acceptance: None,
        priority: None,
        status: None,
        knot_type: None,
        add_tags: vec![],
        remove_tags: vec![],
        add_invariants: vec![],
        remove_invariants: vec![],
        clear_invariants: false,
        add_verification_steps: vec![],
        remove_verification_steps: vec![],
        clear_verification_steps: false,
        gate_owner_kind: None,
        gate_failure_modes: None,
        clear_gate_failure_modes: false,
        execution_plan_objective: None,
        execution_plan_data: None,
        add_note: None,
        add_handoff_capsule: None,
        expected_profile_etag: None,
        force: false,
        state_actor: StateActorMetadata::default(),
    }
}
#[test]
fn update_knot_applies_parity_fields_and_metadata_arrays() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("u"), root.clone()).expect("o");
    let c = app
        .create_knot(
            "Parity",
            Some("legacy body"),
            Some("work_item"),
            Some("default"),
        )
        .expect("c");
    let u = app.update_knot(&c.id, build_parity_patch()).expect("u");
    assert_parity_fields(&u);
    assert_parity_metadata(&u);
    assert_parity_show(&app, &c.id, &u);
    assert_eq!(count_json_files(&root.join(".knots/index")), 2);
    assert!(count_json_files(&root.join(".knots/events")) >= 8);
}
fn build_parity_patch() -> UpdateKnotPatch {
    UpdateKnotPatch {
        title: Some("Parity updated".into()),
        description: Some("full description".into()),
        priority: Some(1),
        status: Some("implementing".into()),
        knot_type: Some(crate::domain::knot_type::KnotType::Work),
        add_tags: vec!["migration".into(), "beads".into()],
        add_invariants: vec![
            crate::domain::invariant::Invariant::new(
                crate::domain::invariant::InvariantType::Scope,
                "all child knots must have one parent",
            )
            .expect("b"),
            crate::domain::invariant::Invariant::new(
                crate::domain::invariant::InvariantType::State,
                "deferred knots resume to deferred_from_state",
            )
            .expect("b"),
        ],
        add_note: Some(MetadataEntryInput {
            content: "carry context".into(),
            username: Some("acartine".into()),
            datetime: Some("2026-02-23T10:00:00Z".into()),
            agentname: Some("codex".into()),
            model: Some("gpt-5".into()),
            version: Some("0.1".into()),
        }),
        add_handoff_capsule: Some(MetadataEntryInput {
            content: "next owner details".into(),
            username: Some("acartine".into()),
            datetime: Some("2026-02-23T10:05:00Z".into()),
            agentname: Some("codex".into()),
            model: Some("gpt-5".into()),
            version: Some("0.1".into()),
        }),
        ..empty_patch()
    }
}
fn assert_parity_fields(u: &KnotView) {
    assert_eq!(u.title, "Parity updated");
    assert_eq!(u.state, "implementation");
    assert_eq!(u.description.as_deref(), Some("full description"));
    assert_eq!(u.priority, Some(1));
    assert_eq!(u.knot_type, crate::domain::knot_type::KnotType::Work);
    assert_eq!(u.tags, vec!["migration".to_string(), "beads".to_string()]);
}
fn assert_parity_metadata(u: &KnotView) {
    assert_eq!(u.notes.len(), 1);
    assert_eq!(u.notes[0].content, "carry context");
    assert_eq!(u.handoff_capsules.len(), 1);
    assert_eq!(u.handoff_capsules[0].content, "next owner details");
    assert_eq!(u.invariants.len(), 2);
    assert_eq!(
        u.invariants[0].condition,
        "all child knots must have one parent"
    );
    assert_eq!(
        u.invariants[1].condition,
        "deferred knots resume to deferred_from_state"
    );
}
fn assert_parity_show(app: &App, id: &str, updated: &KnotView) {
    let s = app.show_knot(id).expect("s").expect("k");
    assert_eq!(s.description.as_deref(), Some("full description"));
    assert_eq!(s.notes.len(), 1);
    assert_eq!(s.handoff_capsules.len(), 1);
    assert_eq!(s.invariants, updated.invariants);
}
#[test]
fn update_knot_can_remove_and_clear_invariants() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("u"), root.clone()).expect("o");
    let c = app
        .create_knot("Inv mut", None, Some("work_item"), Some("default"))
        .expect("c");
    let seeded = seed_invariants(&app, &c.id);
    assert_eq!(seeded.invariants.len(), 2);
    let removed = remove_scope_invariant(&app, &c.id);
    let si = crate::domain::invariant::Invariant::new(
        crate::domain::invariant::InvariantType::State,
        "state invariant",
    )
    .expect("b");
    assert_eq!(removed.invariants, vec![si]);
    assert!(clear_all_invariants(&app, &c.id).invariants.is_empty());
}
fn seed_invariants(app: &App, id: &str) -> KnotView {
    let sc = crate::domain::invariant::Invariant::new(
        crate::domain::invariant::InvariantType::Scope,
        "scope invariant",
    )
    .expect("b");
    let st = crate::domain::invariant::Invariant::new(
        crate::domain::invariant::InvariantType::State,
        "state invariant",
    )
    .expect("b");
    app.update_knot(
        id,
        UpdateKnotPatch {
            add_invariants: vec![sc, st],
            ..empty_patch()
        },
    )
    .expect("s")
}
fn remove_scope_invariant(app: &App, id: &str) -> KnotView {
    let sc = crate::domain::invariant::Invariant::new(
        crate::domain::invariant::InvariantType::Scope,
        "scope invariant",
    )
    .expect("b");
    app.update_knot(
        id,
        UpdateKnotPatch {
            remove_invariants: vec![sc],
            ..empty_patch()
        },
    )
    .expect("r")
}
fn clear_all_invariants(app: &App, id: &str) -> KnotView {
    app.update_knot(
        id,
        UpdateKnotPatch {
            clear_invariants: true,
            ..empty_patch()
        },
    )
    .expect("c")
}

#[test]
fn update_knot_requires_at_least_one_change() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("u"), root.clone()).expect("o");
    let c = app
        .create_knot("Noop", None, Some("idea"), Some("default"))
        .expect("c");
    assert!(matches!(
        app.update_knot(&c.id, empty_patch()),
        Err(AppError::InvalidArgument(_))
    ));
}

#[test]
fn update_knot_persists_execution_plan_and_rehydrates_it() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("u"), root.clone()).expect("o");
    let created = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .expect("created");
    let ref1 = app
        .create_knot("Ref 1", None, Some("idea"), Some("default"))
        .expect("ref1");
    let ref2 = app
        .create_knot("Ref 2", None, Some("idea"), Some("default"))
        .expect("ref2");

    let execution_plan_data = ExecutionPlanData {
        objective: Some("Ship the feature".to_string()),
        summary: Some("Execution plan for selected knots".to_string()),
        mode: Some("autopilot".to_string()),
        model: Some("gpt-5".to_string()),
        assumptions: vec!["assume current knots are correct".to_string()],
        unassigned_knot_ids: vec![ref2.id.clone()],
        waves: vec![ExecutionPlanWave {
            wave_index: 1,
            name: "Persist".to_string(),
            objective: "Thread the payload".to_string(),
            agents: vec![ExecutionPlanAgent {
                role: "backend".to_string(),
                count: 1,
                specialty: Some("storage".to_string()),
            }],
            knots: vec![ExecutionPlanKnot {
                id: ref1.id.clone(),
                title: "Persist plan payload".to_string(),
            }],
            steps: vec![ExecutionPlanStep {
                step_index: 1,
                knot_ids: vec![ref1.id.clone()],
                notes: Some("Persist JSON".to_string()),
            }],
            notes: Some("First wave".to_string()),
        }],
    };

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                execution_plan_data: Some(execution_plan_data.clone()),
                ..empty_patch()
            },
        )
        .expect("update should succeed");
    assert_eq!(updated.execution_plan.as_ref(), Some(&execution_plan_data),);

    let shown = app.show_knot(&created.id).expect("show").expect("exists");
    assert_eq!(shown.execution_plan.as_ref(), Some(&execution_plan_data),);

    let conn = crate::db::open_connection(db.to_str().expect("u")).expect("db");
    crate::db::delete_knot_hot(&conn, &created.id).expect("delete hot");
    crate::db::upsert_knot_warm(&conn, &created.id, &created.title).expect("warm");
    crate::db::upsert_cold_catalog(
        &conn,
        &created.id,
        &created.title,
        &shown.state,
        &shown.updated_at,
    )
    .expect("cold");

    let rehydrated = app
        .rehydrate(&created.id)
        .expect("rehydrate")
        .expect("exists");
    assert_eq!(
        rehydrated.execution_plan.as_ref(),
        Some(&execution_plan_data),
    );
}

#[test]
fn update_knot_sets_execution_plan_objective_without_replacing_waves() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("utf8"), root.clone()).expect("app");
    let created = app
        .create_knot("Plan", None, Some("idea"), Some("default"))
        .expect("created");
    let seeded = ExecutionPlanData {
        objective: Some("Old objective".to_string()),
        waves: vec![ExecutionPlanWave {
            wave_index: 1,
            name: "Wave 1".to_string(),
            objective: "Preserve".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    app.update_knot(
        &created.id,
        UpdateKnotPatch {
            execution_plan_data: Some(seeded),
            ..empty_patch()
        },
    )
    .expect("seed plan");

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                execution_plan_objective: Some("New objective".to_string()),
                ..empty_patch()
            },
        )
        .expect("objective update");
    let plan = updated.execution_plan.expect("plan should exist");
    assert_eq!(plan.objective.as_deref(), Some("New objective"));
    assert_eq!(plan.waves.len(), 1);
    assert_eq!(plan.waves[0].name, "Wave 1");
}

#[test]
fn create_knot_rejects_execution_plan_without_top_level_objective() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("utf8"), root.clone()).expect("app");

    let err = app
        .create_knot_with_options(
            "Plan",
            None,
            Some("ready_for_design"),
            Some("autopilot"),
            None,
            CreateKnotOptions {
                knot_type: KnotType::ExecutionPlan,
                ..CreateKnotOptions::default()
            },
        )
        .expect_err("missing objective should fail");
    assert_eq!(
        err.to_string(),
        "execution_plan knots require a non-empty top-level objective"
    );
}

#[test]
fn update_knot_rejects_execution_plan_type_without_top_level_objective() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("utf8"), root.clone()).expect("app");
    let created = app
        .create_knot("Needs orchestration", None, Some("idea"), Some("default"))
        .expect("created");

    let err = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                knot_type: Some(KnotType::ExecutionPlan),
                ..empty_patch()
            },
        )
        .expect_err("conversion without objective should fail");
    assert_eq!(
        err.to_string(),
        "execution_plan knots require a non-empty top-level objective"
    );
}

#[test]
fn update_knot_rejects_stale_if_match() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("u"), root.clone()).expect("o");
    let c = app
        .create_knot("OCC", None, Some("work_item"), Some("default"))
        .expect("c");
    let exp = c.profile_etag.clone().expect("e");
    let u = app
        .update_knot(
            &c.id,
            UpdateKnotPatch {
                title: Some("OCC 2".into()),
                expected_profile_etag: Some(exp.clone()),
                ..empty_patch()
            },
        )
        .expect("u");
    assert_ne!(u.profile_etag, Some(exp.clone()));
    assert!(matches!(
        app.update_knot(
            &c.id,
            UpdateKnotPatch {
                title: Some("OCC 3".into()),
                expected_profile_etag: Some(exp),
                ..empty_patch()
            }
        ),
        Err(AppError::StaleWorkflowHead { .. })
    ));
}
#[test]
fn rehydrate_builds_hot_record_from_warm_and_full_events() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let ds = db.to_str().expect("u").to_string();
    std::fs::create_dir_all(db.parent().expect("p")).expect("m");
    let conn = crate::db::open_connection(&ds).expect("o");
    crate::db::upsert_knot_warm(&conn, "K-9", "Warm title").expect("u");
    crate::db::upsert_cold_catalog(
        &conn,
        "K-9",
        "Warm title",
        "work_item",
        "2026-02-24T10:00:01Z",
    )
    .expect("c");
    drop(conn);
    write_rehydrate_events(&root);
    let app = App::open(&ds, root.clone()).expect("o");
    let r = app.rehydrate("9").expect("r").expect("k");
    assert_eq!(r.id, "K-9");
    assert_eq!(r.description.as_deref(), Some("rehydrated details"));
    assert_eq!(r.profile_id, "autopilot");
    assert_eq!(r.workflow_id, "work_sdlc");
    assert_eq!(r.profile_etag.as_deref(), Some("1002"));
}
fn write_rehydrate_events(root: &Path) {
    let fp = root.join(".knots/events/2026/02/24/1001-knot.description_set.json");
    std::fs::create_dir_all(fp.parent().expect("p")).expect("m");
    let description_event = concat!(
        "{\"event_id\":\"1001\",",
        "\"occurred_at\":\"2026-02-24T10:00:00Z\",",
        "\"knot_id\":\"K-9\",",
        "\"type\":\"knot.description_set\",",
        "\"data\":{\"description\":\"rehydrated details\"}}",
    );
    std::fs::write(&fp, description_event).expect("w");
    let ip = root.join(".knots/index/2026/02/24/1002-idx.knot_head.json");
    std::fs::create_dir_all(ip.parent().expect("p")).expect("m");
    let head_event = concat!(
        "{\"event_id\":\"1002\",",
        "\"occurred_at\":\"2026-02-24T10:00:01Z\",",
        "\"type\":\"idx.knot_head\",",
        "\"data\":{\"knot_id\":\"K-9\",",
        "\"title\":\"Warm title\",",
        "\"state\":\"work_item\",",
        "\"workflow_id\":\"work_sdlc\",",
        "\"profile_id\":\"autopilot\",",
        "\"updated_at\":\"2026-02-24T10:00:01Z\",",
        "\"terminal\":false}}",
    );
    std::fs::write(&ip, head_event).expect("w");
}

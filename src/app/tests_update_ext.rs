use super::{App, AppError, KnotView, StateActorMetadata, UpdateKnotPatch};
use crate::domain::metadata::MetadataEntryInput;
use std::path::{Path, PathBuf};
use uuid::Uuid;
fn unique_workspace() -> PathBuf {
    let r = std::env::temp_dir().join(format!("knots-app-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&r).expect("mkdir");
    r
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
        gate_owner_kind: None,
        gate_failure_modes: None,
        clear_gate_failure_modes: false,
        add_note: None,
        add_handoff_capsule: None,
        expected_profile_etag: None,
        force: false,
        state_actor: StateActorMetadata::default(),
    }
}
#[test]
fn update_knot_applies_parity_fields_and_metadata_arrays() {
    let root = unique_workspace();
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
    let _ = std::fs::remove_dir_all(root);
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
    let root = unique_workspace();
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
    let _ = std::fs::remove_dir_all(root);
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
    let root = unique_workspace();
    let db = root.join(".knots/cache/state.sqlite");
    let app = App::open(db.to_str().expect("u"), root.clone()).expect("o");
    let c = app
        .create_knot("Noop", None, Some("idea"), Some("default"))
        .expect("c");
    assert!(matches!(
        app.update_knot(&c.id, empty_patch()),
        Err(AppError::InvalidArgument(_))
    ));
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn update_knot_rejects_stale_if_match() {
    let root = unique_workspace();
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
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn rehydrate_builds_hot_record_from_warm_and_full_events() {
    let root = unique_workspace();
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
    assert_eq!(r.profile_id, "default");
    assert_eq!(r.profile_etag.as_deref(), Some("1002"));
    let _ = std::fs::remove_dir_all(root);
}
fn write_rehydrate_events(root: &Path) {
    let fp = root.join(".knots/events/2026/02/24/1001-knot.description_set.json");
    std::fs::create_dir_all(fp.parent().expect("p")).expect("m");
    std::fs::write(&fp, "{\"event_id\":\"1001\",\"occurred_at\":\"2026-02-24T10:00:00Z\",\"knot_id\":\"K-9\",\"type\":\"knot.description_set\",\"data\":{\"description\":\"rehydrated details\"}}").expect("w");
    let ip = root.join(".knots/index/2026/02/24/1002-idx.knot_head.json");
    std::fs::create_dir_all(ip.parent().expect("p")).expect("m");
    std::fs::write(&ip, "{\"event_id\":\"1002\",\"occurred_at\":\"2026-02-24T10:00:01Z\",\"type\":\"idx.knot_head\",\"data\":{\"knot_id\":\"K-9\",\"title\":\"Warm title\",\"state\":\"work_item\",\"profile_id\":\"default\",\"updated_at\":\"2026-02-24T10:00:01Z\",\"terminal\":false}}").expect("w");
}

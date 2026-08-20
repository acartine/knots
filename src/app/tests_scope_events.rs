use serde_json::{json, Value};

use super::{rehydrate_from_events, App, CreateKnotOptions};
use crate::domain::scope::{ScopeData, ScopeFloat, ScopePatch};

fn unique_root(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
}

fn write_event(root: &std::path::Path, filename: &str, body: &Value) {
    let path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25")
        .join(filename);
    std::fs::create_dir_all(path.parent().expect("event parent should exist"))
        .expect("event parent should be creatable");
    std::fs::write(
        path,
        serde_json::to_vec_pretty(body).expect("event serializes"),
    )
    .expect("event should be writable");
}

fn latest_knot_head_payload(root: &std::path::Path, knot_id: &str) -> Value {
    let mut paths = Vec::new();
    let mut stack = vec![root.join(".knots/index")];
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir).expect("index dir should be readable") {
            let path = entry.expect("index entry should be readable").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .into_iter()
        .rev()
        .find_map(|path| {
            let json: Value = serde_json::from_slice(
                &std::fs::read(&path).expect("index event should be readable"),
            )
            .expect("index event should parse");
            let data = json.get("data")?;
            (json["type"] == "idx.knot_head" && data["knot_id"] == knot_id).then(|| data.clone())
        })
        .expect("knot head payload should exist")
}

fn created_event() -> Value {
    json!({
        "event_id": "1000",
        "type": "knot.created",
        "occurred_at": "2026-02-25T10:00:00Z",
        "knot_id": "K-scope",
        "data": {
            "title": "Scoped",
            "state": "implementation",
            "workflow_id": "work_sdlc",
            "profile_id": "autopilot",
        },
    })
}

fn full_scope() -> ScopeData {
    ScopeData {
        volume: Some(8),
        scale: Some("fib_v1".to_string()),
        volume_score_confidence: Some(ScopeFloat::new(0.72).expect("finite")),
        volume_stddev: Some(ScopeFloat::new(1.25).expect("finite")),
        volume_result_id: Some("vol-1".to_string()),
        reliability: Some(44),
        reliability_score_confidence: Some(ScopeFloat::new(0.91).expect("finite")),
        reliability_stddev: Some(ScopeFloat::new(2.5).expect("finite")),
        reliability_band: Some("medium".to_string()),
        reliability_result_id: Some("rel-1".to_string()),
    }
}

fn scope_event(event_id: &str, occurred_at: &str, scope: &ScopeData) -> Value {
    json!({
        "event_id": event_id,
        "type": "knot.scope_set",
        "occurred_at": occurred_at,
        "knot_id": "K-scope",
        "data": serde_json::to_value(scope).expect("scope serializes"),
    })
}

#[test]
fn scope_update_writes_scope_to_index_head_and_list_view() {
    let root_ws = unique_root("knots-scope-index-head");
    let root = root_ws.path().to_path_buf();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");
    let created = app
        .create_knot_with_options(
            "Scoped index",
            Some("desc"),
            Some("implementation"),
            Some("autopilot"),
            None,
            CreateKnotOptions::default(),
        )
        .expect("create should succeed");
    let expected = ScopeData {
        volume: Some(13),
        scale: Some("fib_v1".to_string()),
        reliability: Some(55),
        reliability_band: Some("high".to_string()),
        ..ScopeData::default()
    };

    let updated = app
        .update_knot_scope(
            &created.id,
            ScopePatch {
                volume: expected.volume,
                scale: expected.scale.clone(),
                reliability: expected.reliability,
                reliability_band: expected.reliability_band.clone(),
                ..ScopePatch::default()
            },
            None,
        )
        .expect("scope update should succeed");

    assert_eq!(updated.scope.as_ref(), Some(&expected));
    let listed = app.list_knots().expect("list should succeed");
    let listed_scope = listed
        .iter()
        .find(|view| view.id == created.id)
        .and_then(|view| view.scope.as_ref());
    assert_eq!(listed_scope, Some(&expected));
    let head = latest_knot_head_payload(&root, &created.id);
    assert_eq!(head["scope"]["volume"], json!(13));
    assert_eq!(head["scope"]["scale"], json!("fib_v1"));
    assert_eq!(head["scope"]["reliability"], json!(55));
    assert_eq!(head["scope"]["reliability_band"], json!("high"));
}

#[test]
fn rehydrate_restores_full_scope_metadata_from_events() {
    let root_ws = unique_root("knots-rehydrate-scope-full");
    let root = root_ws.path().to_path_buf();
    write_event(&root, "1000-knot.created.json", &created_event());
    let scope = full_scope();
    write_event(
        &root,
        "1100-knot.scope_set.json",
        &scope_event("1100", "2026-02-25T10:10:00Z", &scope),
    );

    let projection = rehydrate_from_events(
        &[root.join(".knots").as_path()],
        "K-scope",
        "Scoped".to_string(),
        "implementation".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("rehydrate should succeed");

    assert_eq!(projection.scope_data, scope);
    assert_eq!(projection.updated_at, "2026-02-25T10:10:00Z");
}

#[test]
fn rehydrate_merges_partial_scope_event_into_prior_scope() {
    let root_ws = unique_root("knots-rehydrate-scope-partial");
    let root = root_ws.path().to_path_buf();
    write_event(&root, "1000-knot.created.json", &created_event());
    let initial = full_scope();
    write_event(
        &root,
        "1100-knot.scope_set.json",
        &scope_event("1100", "2026-02-25T10:10:00Z", &initial),
    );

    // The emission side pre-merges patches into ScopeData before writing,
    // so the second event carries the FULL merged state. Replay's
    // replace-semantics must reproduce that exact merged state.
    let merged = ScopeData {
        volume: Some(13),
        reliability: Some(55),
        reliability_band: Some("high".to_string()),
        ..initial.clone()
    };
    write_event(
        &root,
        "1200-knot.scope_set.json",
        &scope_event("1200", "2026-02-25T11:00:00Z", &merged),
    );

    let projection = rehydrate_from_events(
        &[root.join(".knots").as_path()],
        "K-scope",
        "Scoped".to_string(),
        "implementation".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("rehydrate should succeed");

    assert_eq!(projection.scope_data, merged);
    assert_eq!(projection.scope_data.volume, Some(13));
    assert_eq!(projection.scope_data.reliability, Some(55));
    assert_eq!(
        projection.scope_data.reliability_band.as_deref(),
        Some("high")
    );
    assert_eq!(projection.scope_data.scale.as_deref(), Some("fib_v1"));
}

#[test]
fn rehydrate_preserves_field_absence_through_round_trip() {
    let root_ws = unique_root("knots-rehydrate-scope-absence");
    let root = root_ws.path().to_path_buf();
    write_event(&root, "1000-knot.created.json", &created_event());
    let sparse = ScopeData {
        volume: Some(3),
        reliability_band: Some("low".to_string()),
        ..ScopeData::default()
    };
    let payload = serde_json::to_value(&sparse).expect("sparse scope serializes");
    let payload_object = payload
        .as_object()
        .expect("sparse scope payload is an object");
    assert!(!payload_object.contains_key("scale"));
    assert!(!payload_object.contains_key("reliability"));

    write_event(
        &root,
        "1100-knot.scope_set.json",
        &scope_event("1100", "2026-02-25T10:10:00Z", &sparse),
    );

    let projection = rehydrate_from_events(
        &[root.join(".knots").as_path()],
        "K-scope",
        "Scoped".to_string(),
        "implementation".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("rehydrate should succeed");

    assert_eq!(projection.scope_data, sparse);
    assert!(projection.scope_data.scale.is_none());
    assert!(projection.scope_data.volume_score_confidence.is_none());
    assert!(projection.scope_data.reliability.is_none());
    assert!(projection.scope_data.reliability_result_id.is_none());
}

#[test]
fn rehydrate_without_scope_events_yields_default_scope() {
    let root_ws = unique_root("knots-rehydrate-scope-none");
    let root = root_ws.path().to_path_buf();
    write_event(&root, "1000-knot.created.json", &created_event());

    let projection = rehydrate_from_events(
        &[root.join(".knots").as_path()],
        "K-scope",
        "Scoped".to_string(),
        "implementation".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("rehydrate should succeed");

    assert_eq!(projection.scope_data, ScopeData::default());
    assert!(projection.scope_data.is_empty());
}

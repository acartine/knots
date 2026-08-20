use super::{rehydrate_from_events, App, CreateKnotOptions, UpdateKnotPatch};
use crate::db;
use serde_json::Value;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-app-verification")
}

fn open_app(root: &std::path::Path) -> (App, String) {
    let db_path = root.join(".knots/cache/state.sqlite");
    let db_path_str = db_path.to_str().expect("utf8 path").to_string();
    let app = App::open(&db_path_str, root.to_path_buf()).expect("app should open");
    (app, db_path_str)
}

fn verification_events(root: &std::path::Path) -> Vec<Value> {
    let mut values = Vec::new();
    let mut stack = vec![root.join(".knots/events")];
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir).expect("events should read") {
            let path = entry.expect("entry should read").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let value: Value =
                serde_json::from_slice(&std::fs::read(path).expect("event should read"))
                    .expect("event should parse");
            if value.get("type").and_then(Value::as_str) == Some("knot.verification_steps_set") {
                values.push(value);
            }
        }
    }
    values
}

#[test]
fn create_with_verification_steps_round_trips_and_serializes() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (app, db_path) = open_app(&root);
    let created = app
        .create_knot_with_options(
            "Verify create",
            Some("Description"),
            Some("ready_for_implementation"),
            Some("autopilot"),
            None,
            CreateKnotOptions {
                verification_steps: vec![
                    "  cargo test  ".to_string(),
                    String::new(),
                    "cargo test".to_string(),
                    "kno show --json".to_string(),
                ],
                ..CreateKnotOptions::default()
            },
        )
        .expect("create should succeed");
    assert_eq!(
        created.verification_steps,
        vec!["cargo test".to_string(), "kno show --json".to_string()]
    );

    let conn = db::open_connection(&db_path).expect("db should reopen");
    let record = db::get_knot_hot(&conn, &created.id)
        .expect("get should succeed")
        .expect("record should exist");
    assert_eq!(record.verification_steps, created.verification_steps);

    let json = serde_json::to_value(&created).expect("view should serialize");
    assert_eq!(
        json.get("verification_steps"),
        Some(&serde_json::json!(["cargo test", "kno show --json"]))
    );
}

#[test]
fn update_add_remove_clear_and_empty_noop_verification_steps() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot(
            "Verify update",
            None,
            Some("ready_for_implementation"),
            Some("autopilot"),
        )
        .expect("create should succeed");

    let added = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                add_verification_steps: vec![
                    "cargo test".to_string(),
                    "cargo test".to_string(),
                    "make sanity".to_string(),
                ],
                ..UpdateKnotPatch::default()
            },
        )
        .expect("add should succeed");
    assert_eq!(
        added.verification_steps,
        vec!["cargo test".to_string(), "make sanity".to_string()]
    );

    let removed = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                remove_verification_steps: vec!["cargo test".to_string()],
                ..UpdateKnotPatch::default()
            },
        )
        .expect("remove should succeed");
    assert_eq!(removed.verification_steps, vec!["make sanity".to_string()]);

    let event_count = verification_events(&root).len();
    let unchanged = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                add_verification_steps: vec!["   ".to_string()],
                ..UpdateKnotPatch::default()
            },
        )
        .expect("empty add should be a no-op update");
    assert_eq!(
        unchanged.verification_steps,
        vec!["make sanity".to_string()]
    );
    assert_eq!(verification_events(&root).len(), event_count);

    let still_unchanged = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                remove_verification_steps: vec!["   ".to_string()],
                ..UpdateKnotPatch::default()
            },
        )
        .expect("empty remove should be a no-op update");
    assert_eq!(
        still_unchanged.verification_steps,
        vec!["make sanity".to_string()]
    );
    assert_eq!(verification_events(&root).len(), event_count);

    let cleared = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                clear_verification_steps: true,
                ..UpdateKnotPatch::default()
            },
        )
        .expect("clear should succeed");
    assert!(cleared.verification_steps.is_empty());
}

#[test]
fn rehydrate_restores_verification_steps_from_events() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let (app, _) = open_app(&root);
    let created = app
        .create_knot_with_options(
            "Rehydrate verify",
            None,
            Some("ready_for_implementation"),
            Some("autopilot"),
            None,
            CreateKnotOptions {
                verification_steps: vec!["cargo test".to_string()],
                ..CreateKnotOptions::default()
            },
        )
        .expect("create should succeed");
    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                add_verification_steps: vec!["make sanity".to_string()],
                ..UpdateKnotPatch::default()
            },
        )
        .expect("update should succeed");

    let store = root.join(".knots");
    let projection = rehydrate_from_events(
        &[store.as_path()],
        &created.id,
        created.title,
        created.state,
        created.updated_at,
    )
    .expect("rehydrate should succeed");
    assert_eq!(projection.verification_steps, updated.verification_steps);
}

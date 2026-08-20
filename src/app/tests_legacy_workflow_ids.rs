use super::{rehydrate_from_events, AppError};
use crate::app::App;
use crate::db::{self, UpsertKnotHot};

fn unique_root(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
}

fn write_event(root: &std::path::Path, filename: &str, body: &str) {
    let path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25")
        .join(filename);
    std::fs::create_dir_all(path.parent().expect("event parent should exist"))
        .expect("event parent should be creatable");
    std::fs::write(path, body).expect("event should be writable");
}

fn open_app(root: &std::path::Path) -> App {
    let db_path = root.join(".knots").join("cache").join("state.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("db parent should exist"))
        .expect("db parent should be creatable");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn rehydrate_from_events_rejects_missing_workflow_id() {
    let missing_root_ws = unique_root("knots-rehydrate-missing-workflow");
    let missing_root = missing_root_ws.path().to_path_buf();
    let missing = rehydrate_from_events(
        &[missing_root.as_path()],
        "K-missing",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect_err("missing workflow id should fail");
    assert!(matches!(missing, AppError::InvalidArgument(message) if
        message.contains("missing workflow_id")));
}

#[test]
fn rehydrate_from_events_reads_union_of_local_and_worktree_roots() {
    // Simulates a knot whose events live only under the `_worktree` copy
    // (pulled from origin, never written locally). Before the multi-root
    // fix, rehydrate would fail with "missing workflow_id".
    let root_ws = unique_root("knots-rehydrate-worktree-only");
    let root = root_ws.path().to_path_buf();
    let worktree_knots = root.join(".knots").join("_worktree").join(".knots");
    let event_dir = worktree_knots
        .join("events")
        .join("2026")
        .join("02")
        .join("25");
    std::fs::create_dir_all(&event_dir).expect("worktree event dir should be creatable");
    std::fs::write(
        event_dir.join("1000-knot.created.json"),
        concat!(
            "{\n",
            "  \"event_id\": \"1000\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-pulled\",\n",
            "  \"data\": {\n",
            "    \"title\": \"Pulled\",\n",
            "    \"state\": \"implementation\",\n",
            "    \"workflow_id\": \"work_sdlc\",\n",
            "    \"profile_id\": \"autopilot\"\n",
            "  }\n",
            "}\n"
        ),
    )
    .expect("worktree event should be writable");

    let local_root = root.join(".knots");
    let worktree_root = root.join(".knots").join("_worktree");
    let projection = rehydrate_from_events(
        &[local_root.as_path(), worktree_root.as_path()],
        "K-pulled",
        "Pulled".to_string(),
        "implementation".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("rehydrate should find events under the worktree root");
    assert_eq!(projection.workflow_id, "work_sdlc");
    assert_eq!(projection.profile_id, "autopilot");
}

#[test]
fn rehydrate_from_events_dedupes_events_present_in_both_roots() {
    // After a push, the same event file exists in both `.knots/events/`
    // and `.knots/_worktree/.knots/events/`. Replaying it twice would
    // double-append list fields like tags; the dedupe pass prevents that.
    let root_ws = unique_root("knots-rehydrate-dedup");
    let root = root_ws.path().to_path_buf();
    let local_dir = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25");
    let worktree_dir = root
        .join(".knots")
        .join("_worktree")
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25");
    std::fs::create_dir_all(&local_dir).expect("local dir creatable");
    std::fs::create_dir_all(&worktree_dir).expect("worktree dir creatable");
    let created = concat!(
        "{\n",
        "  \"event_id\": \"1000\",\n",
        "  \"type\": \"knot.created\",\n",
        "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
        "  \"knot_id\": \"K-dup\",\n",
        "  \"data\": {\n",
        "    \"title\": \"Dup\",\n",
        "    \"state\": \"implementation\",\n",
        "    \"workflow_id\": \"work_sdlc\",\n",
        "    \"profile_id\": \"autopilot\"\n",
        "  }\n",
        "}\n"
    );
    let tag_add = concat!(
        "{\n",
        "  \"event_id\": \"1001\",\n",
        "  \"type\": \"knot.tag_add\",\n",
        "  \"occurred_at\": \"2026-02-25T10:01:00Z\",\n",
        "  \"knot_id\": \"K-dup\",\n",
        "  \"data\": { \"tag\": \"alpha\" }\n",
        "}\n"
    );
    std::fs::write(local_dir.join("1000-knot.created.json"), created)
        .expect("local created writable");
    std::fs::write(worktree_dir.join("1000-knot.created.json"), created)
        .expect("worktree created writable");
    std::fs::write(local_dir.join("1001-knot.tag_add.json"), tag_add)
        .expect("local tag_add writable");
    std::fs::write(worktree_dir.join("1001-knot.tag_add.json"), tag_add)
        .expect("worktree tag_add writable");

    let local_root = root.join(".knots");
    let worktree_root = root.join(".knots").join("_worktree");
    let projection = rehydrate_from_events(
        &[local_root.as_path(), worktree_root.as_path()],
        "K-dup",
        "Dup".to_string(),
        "implementation".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("rehydrate should dedupe events present in both roots");
    assert_eq!(
        projection
            .tags
            .iter()
            .filter(|t| t.as_str() == "alpha")
            .count(),
        1,
        "a tag_add event in both roots should apply exactly once"
    );
}

#[test]
fn rehydrate_from_events_converts_legacy_workflow_id() {
    let legacy_root_ws = unique_root("knots-rehydrate-legacy-workflow");
    let legacy_root = legacy_root_ws.path().to_path_buf();
    write_event(
        &legacy_root,
        "1000-knot.created.json",
        concat!(
            "{\n",
            "  \"event_id\": \"1000\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"occurred_at\": \"2026-02-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-legacy\",\n",
            "  \"data\": {\n",
            "    \"title\": \"Legacy\",\n",
            "    \"state\": \"ready_for_planning\",\n",
            "    \"workflow_id\": \"knots_sdlc\",\n",
            "    \"profile_id\": \"autopilot\"\n",
            "  }\n",
            "}\n"
        ),
    );
    let projection = rehydrate_from_events(
        &[legacy_root.as_path()],
        "K-legacy",
        "Legacy".to_string(),
        "ready_for_planning".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("legacy workflow id should be converted, not rejected");
    assert_eq!(projection.workflow_id, "work_sdlc");
}

#[test]
fn rehydrate_from_events_reports_invalid_json() {
    let root_ws = unique_root("knots-rehydrate-invalid-json");
    let root = root_ws.path().to_path_buf();
    write_event(&root, "bad-knot.created.json", "{");

    let bad_full = rehydrate_from_events(
        &[root.as_path()],
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(bad_full, Err(AppError::InvalidArgument(_))));

    std::fs::remove_file(
        root.join(".knots")
            .join("events")
            .join("2026")
            .join("02")
            .join("25")
            .join("bad-knot.created.json"),
    )
    .expect("bad full file should be removable");

    let index_path = root
        .join(".knots")
        .join("index")
        .join("2026")
        .join("02")
        .join("25")
        .join("bad-idx.knot_head.json");
    std::fs::create_dir_all(
        index_path
            .parent()
            .expect("index event parent should exist"),
    )
    .expect("index event parent should be creatable");
    std::fs::write(&index_path, "{").expect("index event should be writable");

    let bad_index = rehydrate_from_events(
        &[root.as_path()],
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(bad_index, Err(AppError::InvalidArgument(_))));
}

#[test]
fn show_knot_fails_when_cache_contains_legacy_workflow_id() {
    let root_ws = unique_root("knots-show-legacy-workflow");
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    db::upsert_knot_hot(
        &app.conn,
        &UpsertKnotHot {
            id: "K-legacy-db",
            title: "Legacy DB",
            state: "ready_for_planning",
            updated_at: "2026-02-25T10:00:00Z",
            body: None,
            description: None,
            acceptance: None,
            priority: None,
            knot_type: Some("work"),
            tags: &[],
            notes: &[],
            handoff_capsules: &[],
            invariants: &[],
            verification_steps: &[],
            step_history: &[],
            gate_data: &crate::domain::gate::GateData::default(),
            lease_data: &crate::domain::lease::LeaseData::default(),
            execution_plan_data: &crate::domain::execution_plan::ExecutionPlanData::default(),
            lease_id: None,
            lease_expiry_ts: 0,
            workflow_id: "knots_sdlc",
            profile_id: "autopilot",
            profile_etag: Some("etag-1"),
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: Some("2026-02-25T10:00:00Z"),
        },
    )
    .expect("legacy row should upsert");

    let view = app
        .show_knot("K-legacy-db")
        .expect("show should succeed for legacy profile")
        .expect("knot should exist");
    assert!(
        view.step_metadata.is_none(),
        "no metadata for unknown profile"
    );
}

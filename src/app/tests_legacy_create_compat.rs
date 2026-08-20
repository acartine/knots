use super::App;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-app-test")
}

#[test]
fn rehydrate_uses_body_from_legacy_knot_created_when_no_description_set() {
    // Regression: pre-fix `knot.created` events embedded the description
    // inline as `body` and did not emit a separate `knot.description_set`.
    // Rehydrate must read body so old events still recover the description.
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let db = root.join(".knots/cache/state.sqlite");
    let ds = db.to_str().expect("u").to_string();
    std::fs::create_dir_all(db.parent().expect("p")).expect("m");
    let conn = crate::db::open_connection(&ds).expect("o");
    crate::db::upsert_knot_warm(&conn, "K-LEGACY", "Legacy title").expect("u");
    crate::db::upsert_cold_catalog(
        &conn,
        "K-LEGACY",
        "Legacy title",
        "work_item",
        "2026-02-24T10:00:01Z",
    )
    .expect("c");
    drop(conn);
    let fp = root.join(".knots/events/2026/02/24/1001-knot.created.json");
    std::fs::create_dir_all(fp.parent().expect("p")).expect("m");
    let created_event = concat!(
        "{\"event_id\":\"1001\",",
        "\"occurred_at\":\"2026-02-24T10:00:00Z\",",
        "\"knot_id\":\"K-LEGACY\",",
        "\"type\":\"knot.created\",",
        "\"data\":{\"title\":\"Legacy title\",",
        "\"state\":\"work_item\",",
        "\"workflow_id\":\"work_sdlc\",",
        "\"profile_id\":\"autopilot\",",
        "\"body\":\"legacy inline description\",",
        "\"type\":\"work\"}}",
    );
    std::fs::write(&fp, created_event).expect("w");
    let ip = root.join(".knots/index/2026/02/24/1002-idx.knot_head.json");
    std::fs::create_dir_all(ip.parent().expect("p")).expect("m");
    let head_event = concat!(
        "{\"event_id\":\"1002\",",
        "\"occurred_at\":\"2026-02-24T10:00:01Z\",",
        "\"type\":\"idx.knot_head\",",
        "\"data\":{\"knot_id\":\"K-LEGACY\",",
        "\"title\":\"Legacy title\",",
        "\"state\":\"work_item\",",
        "\"workflow_id\":\"work_sdlc\",",
        "\"profile_id\":\"autopilot\",",
        "\"updated_at\":\"2026-02-24T10:00:01Z\",",
        "\"terminal\":false}}",
    );
    std::fs::write(&ip, head_event).expect("w");
    let app = App::open(&ds, root.clone()).expect("o");
    let r = app.rehydrate("LEGACY").expect("r").expect("k");
    assert_eq!(r.id, "K-LEGACY");
    assert_eq!(r.description.as_deref(), Some("legacy inline description"));
    assert_eq!(r.body.as_deref(), Some("legacy inline description"));
}

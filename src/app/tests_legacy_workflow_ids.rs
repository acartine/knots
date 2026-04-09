use super::{rehydrate_from_events, AppError};

fn write_legacy_rehydrate_event(root: &std::path::Path) {
    let full_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25")
        .join("1000-knot.created.json");
    std::fs::create_dir_all(
        full_path
            .parent()
            .expect("legacy event parent directory should exist"),
    )
    .expect("legacy event parent should be creatable");
    std::fs::write(
        &full_path,
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
    )
    .expect("legacy full event should be writable");
}

#[test]
fn rehydrate_from_events_reports_missing_workflow_and_invalid_json() {
    let root = std::env::temp_dir().join(format!("knots-rehydrate-ext-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("root should be creatable");

    let missing = rehydrate_from_events(
        &root,
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("rehydrate should fall back to builtin workflow");
    assert_eq!(missing.workflow_id, "work_sdlc");
    assert_eq!(missing.profile_id, "work_sdlc");

    let legacy_root =
        std::env::temp_dir().join(format!("knots-rehydrate-legacy-{}", uuid::Uuid::now_v7()));
    write_legacy_rehydrate_event(&legacy_root);
    let legacy = rehydrate_from_events(
        &legacy_root,
        "K-legacy",
        "Legacy".to_string(),
        "ready_for_planning".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    )
    .expect("legacy builtin workflow id should be canonicalized");
    assert_eq!(legacy.workflow_id, "work_sdlc");
    assert_eq!(legacy.profile_id, "autopilot");

    let full_path = root
        .join(".knots")
        .join("events")
        .join("2026")
        .join("02")
        .join("25")
        .join("bad-knot.created.json");
    std::fs::create_dir_all(
        full_path
            .parent()
            .expect("full event parent directory should exist"),
    )
    .expect("full event parent should be creatable");
    std::fs::write(&full_path, "{").expect("full event should be writable");

    let bad_full = rehydrate_from_events(
        &root,
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(bad_full, Err(AppError::InvalidArgument(_))));

    std::fs::remove_file(&full_path).expect("bad full file should be removable");
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
            .expect("index event parent directory should exist"),
    )
    .expect("index event parent should be creatable");
    std::fs::write(&index_path, "{").expect("index event should be writable");

    let bad_index = rehydrate_from_events(
        &root,
        "K-1",
        "Title".to_string(),
        "work_item".to_string(),
        "2026-02-25T10:00:00Z".to_string(),
    );
    assert!(matches!(bad_index, Err(AppError::InvalidArgument(_))));

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(legacy_root);
}

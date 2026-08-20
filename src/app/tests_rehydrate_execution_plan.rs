use super::rehydrate_from_events;

fn unique_root(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
}

fn write_event(root: &std::path::Path, subdir: &str, filename: &str, body: &str) {
    let path = root
        .join(".knots")
        .join(subdir)
        .join("2026")
        .join("05")
        .join("25")
        .join(filename);
    std::fs::create_dir_all(path.parent().expect("event parent should exist"))
        .expect("event parent should be creatable");
    std::fs::write(path, body).expect("event should be writable");
}

#[test]
fn rehydrate_full_execution_plan_snapshot_wins_over_sparse_index() {
    let root_ws = unique_root("knots-rehydrate-plan-snapshot");
    let root = root_ws.path().to_path_buf();
    write_event(
        &root,
        "events",
        "1000-knot.created.json",
        concat!(
            "{\n",
            "  \"event_id\": \"1000\",\n",
            "  \"type\": \"knot.created\",\n",
            "  \"occurred_at\": \"2026-05-25T10:00:00Z\",\n",
            "  \"knot_id\": \"K-plan\",\n",
            "  \"data\": {\n",
            "    \"title\": \"Plan\",\n",
            "    \"state\": \"ready_for_design\",\n",
            "    \"workflow_id\": \"execution_plan_sdlc\",\n",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"type\": \"execution_plan\"\n",
            "  }\n",
            "}\n"
        ),
    );
    write_event(
        &root,
        "events",
        "1001-knot.execution_plan_data_set.json",
        concat!(
            "{\n",
            "  \"event_id\": \"1001\",\n",
            "  \"type\": \"knot.execution_plan_data_set\",\n",
            "  \"occurred_at\": \"2026-05-25T10:01:00Z\",\n",
            "  \"knot_id\": \"K-plan\",\n",
            "  \"data\": { \"execution_plan\": {\n",
            "    \"objective\": \"recover\",\n",
            "    \"waves\": [{\n",
            "      \"wave_index\": 5,\n",
            "      \"name\": \"Wave 5\",\n",
            "      \"objective\": \"keep waves\",\n",
            "      \"steps\": [{\"step_index\": 4, \"knot_ids\": [\"K-gate\"]}]\n",
            "    }]\n",
            "  }}\n",
            "}\n"
        ),
    );
    write_event(
        &root,
        "index",
        "1002-idx.knot_head.json",
        concat!(
            "{\n",
            "  \"event_id\": \"1002\",\n",
            "  \"type\": \"idx.knot_head\",\n",
            "  \"occurred_at\": \"2026-05-25T10:01:00Z\",\n",
            "  \"data\": {\n",
            "    \"knot_id\": \"K-plan\",\n",
            "    \"title\": \"Plan\",\n",
            "    \"state\": \"ready_for_design\",\n",
            "    \"workflow_id\": \"execution_plan_sdlc\",\n",
            "    \"profile_id\": \"autopilot\",\n",
            "    \"updated_at\": \"2026-05-25T10:01:00Z\",\n",
            "    \"terminal\": false,\n",
            "    \"type\": \"execution_plan\",\n",
            "    \"execution_plan\": {\"objective\": \"recover\"}\n",
            "  }\n",
            "}\n"
        ),
    );

    let projection = rehydrate_from_events(
        &[root.as_path()],
        "K-plan",
        "Plan".to_string(),
        "ready_for_design".to_string(),
        "2026-05-25T10:01:00Z".to_string(),
    )
    .expect("rehydrate should replay the full execution plan snapshot");

    assert_eq!(
        projection.execution_plan_data.objective.as_deref(),
        Some("recover")
    );
    assert_eq!(projection.execution_plan_data.waves.len(), 1);
    assert_eq!(projection.execution_plan_data.waves[0].wave_index, 5);
    assert_eq!(projection.execution_plan_data.waves[0].steps.len(), 1);
}

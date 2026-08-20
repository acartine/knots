mod cli_dispatch_helpers;

use std::path::Path;

use cli_dispatch_helpers::*;
use serde_json::{json, Value};

fn canonical_id(repo_root: &Path, db: &Path, id_or_alias: &str) -> String {
    let shown = run_knots(repo_root, db, &["show", id_or_alias, "--json"]);
    assert_success(&shown);
    let view: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    view["id"]
        .as_str()
        .expect("show json should expose id field")
        .to_string()
}

fn assert_round_trip_plan(plan: &Value, work_a_id: &str, work_b_id: &str) {
    assert!(plan.get("repo_path").is_none());
    assert_eq!(plan["objective"], "Smoke-test execution plan persistence");
    assert_eq!(plan["mode"], "autopilot");
    assert_eq!(plan["model"], "smoke-tester");
    assert_eq!(
        plan["assumptions"],
        json!(["assume CLI dispatch stays wired"])
    );
    assert!(plan.get("knot_ids").is_none());

    let waves = plan["waves"].as_array().expect("waves array");
    assert_eq!(waves.len(), 1);
    let wave = &waves[0];
    assert_eq!(wave["wave_index"], 1);
    assert_eq!(wave["name"], "Wave 1");
    assert_eq!(wave["objective"], "Land both work knots together");
    assert_eq!(wave["notes"], "Sole wave");

    let agents = wave["agents"].as_array().expect("agents array");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["role"], "implementer");
    assert_eq!(agents[0]["count"], 2);
    assert_eq!(agents[0]["specialty"], "rust");

    let steps = wave["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["step_index"], 1);
    assert_eq!(steps[0]["knot_ids"], json!([work_a_id, work_b_id]));
    assert_eq!(steps[0]["notes"], "Run A and B concurrently");
}

#[test]
fn execution_plan_file_round_trips_through_show_json() {
    let root_ws = unique_workspace("knots-cli-exec-plan-file");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Execution plan carrier",
            "--type",
            "execution_plan",
            "--objective",
            "Coordinate execution-plan updates",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);
    let work_a = run_knots(&root, &db, &["new", "Wave 1 work A"]);
    assert_success(&work_a);
    let work_a_id = canonical_id(&root, &db, &parse_created_id(&work_a));
    let work_b = run_knots(&root, &db, &["new", "Wave 1 work B"]);
    assert_success(&work_b);
    let work_b_id = canonical_id(&root, &db, &parse_created_id(&work_b));

    let plan_payload = json!({
        "repo_path": "/repo",
        "objective": "Smoke-test execution plan persistence",
        "summary": "Ensure typed plan survives the CLI write/read boundary",
        "mode": "autopilot",
        "model": "smoke-tester",
        "assumptions": ["assume CLI dispatch stays wired"],
        "knot_ids": [work_a_id.clone(), work_b_id.clone()],
        "unassigned_knot_ids": [],
        "waves": [
            {
                "wave_index": 1,
                "name": "Wave 1",
                "objective": "Land both work knots together",
                "agents": [{
                    "role": "implementer",
                    "count": 2,
                    "specialty": "rust"
                }],
                "knots": [
                    { "id": work_a_id.clone(), "title": "Wave 1 work A" },
                    { "id": work_b_id.clone(), "title": "Wave 1 work B" }
                ],
                "steps": [{
                    "step_index": 1,
                    "knot_ids": [work_a_id.clone(), work_b_id.clone()],
                    "notes": "Run A and B concurrently"
                }],
                "notes": "Sole wave"
            }
        ]
    });
    let plan_path = root.join("plan.json");
    std::fs::write(&plan_path, plan_payload.to_string()).expect("plan file should write");

    let updated = run_knots(
        &root,
        &db,
        &[
            "update",
            &knot_id,
            "--execution-plan-file",
            plan_path.to_str().expect("utf8 plan path"),
        ],
    );
    assert_success(&updated);
    let shown = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let view: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    assert_eq!(view["type"], "execution_plan");
    let plan = view
        .get("execution_plan")
        .cloned()
        .expect("execution_plan field should be present");
    assert_round_trip_plan(&plan, &work_a_id, &work_b_id);
}

#[test]
fn execution_plan_file_survives_rehydrate_after_hot_eviction() {
    let root_ws = unique_workspace("knots-cli-exec-plan-rehydrate");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Plan to rehydrate",
            "--type",
            "execution_plan",
            "--objective",
            "Coordinate rehydrate coverage",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let plan_path = root.join("rehydrate-plan.json");
    std::fs::write(
        &plan_path,
        json!({
            "objective": "Survive rehydrate boundary",
            "summary": "Plan body must rebuild from event log",
            "waves": [{
                "wave_index": 1,
                "name": "Foundations",
                "objective": "Land payload"
            }]
        })
        .to_string(),
    )
    .expect("plan file should write");

    let updated = run_knots(
        &root,
        &db,
        &[
            "update",
            &knot_id,
            "--execution-plan-file",
            plan_path.to_str().expect("utf8 plan path"),
        ],
    );
    assert_success(&updated);

    let conn = rusqlite::Connection::open(&db).expect("db should open for hot eviction");
    conn.execute("DELETE FROM knot_hot WHERE id = ?1", [&knot_id])
        .expect("hot row should delete");
    drop(conn);

    let rehydrated = run_knots(&root, &db, &["rehydrate", &knot_id, "--json"]);
    assert_success(&rehydrated);

    let shown = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let view: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    let plan = view
        .get("execution_plan")
        .cloned()
        .expect("execution_plan should rehydrate");
    assert_eq!(plan["objective"], "Survive rehydrate boundary");
    assert_eq!(plan["summary"], "Plan body must rebuild from event log");
    let waves = plan["waves"].as_array().expect("waves array");
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0]["name"], "Foundations");
    assert_eq!(waves[0]["objective"], "Land payload");
}

#[test]
fn updating_work_knot_to_execution_plan_re_roots_workflow() {
    let root_ws = unique_workspace("knots-cli-exec-plan-type-update");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(&root, &db, &["new", "Needs orchestration"]);
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let updated = run_knots(
        &root,
        &db,
        &[
            "update",
            &knot_id,
            "--type",
            "execution_plan",
            "--objective",
            "Coordinate the execution plan",
        ],
    );
    assert_success(&updated);

    let shown = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let view: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    assert_eq!(view["type"], "execution_plan");
    assert_eq!(view["workflow_id"], "execution_plan_sdlc");
    assert_eq!(view["profile_id"], "autopilot");
    assert_eq!(view["state"], "ready_for_design");
    assert_eq!(
        view["execution_plan"]["objective"],
        "Coordinate the execution plan"
    );
}

#[test]
fn creating_execution_plan_without_objective_fails() {
    let root_ws = unique_workspace("knots-cli-exec-plan-create-fail");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &["new", "Execution plan", "--type", "execution_plan"],
    );
    assert_failure(&created);
    let stderr = String::from_utf8_lossy(&created.stderr);
    assert!(
        stderr.contains("execution_plan knots require a non-empty top-level objective"),
        "stderr should mention missing objective: {stderr}"
    );
}

#[test]
fn execution_plan_objective_update_preserves_existing_waves() {
    let root_ws = unique_workspace("knots-cli-exec-plan-objective-update");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Execution plan",
            "--type",
            "execution_plan",
            "--objective",
            "Original objective",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let wave = run_knots(
        &root,
        &db,
        &[
            "plan",
            "wave",
            "add",
            &knot_id,
            "--name",
            "Wave 1",
            "--objective",
            "Land the wave",
        ],
    );
    assert_success(&wave);

    let updated = run_knots(
        &root,
        &db,
        &["update", &knot_id, "--objective", "Updated objective"],
    );
    assert_success(&updated);

    let shown = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let view: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    assert_eq!(view["execution_plan"]["objective"], "Updated objective");
    let waves = view["execution_plan"]["waves"]
        .as_array()
        .expect("waves should remain present");
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0]["name"], "Wave 1");
}

#[test]
fn updating_work_knot_to_execution_plan_without_objective_fails() {
    let root_ws = unique_workspace("knots-cli-exec-plan-type-fail");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(&root, &db, &["new", "Needs orchestration"]);
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let updated = run_knots(
        &root,
        &db,
        &["update", &knot_id, "--type", "execution_plan"],
    );
    assert_failure(&updated);
    let stderr = String::from_utf8_lossy(&updated.stderr);
    assert!(
        stderr.contains("execution_plan knots require a non-empty top-level objective"),
        "stderr should mention missing objective: {stderr}"
    );
}

#[test]
fn execution_plan_next_advances_from_design_queue() {
    let root_ws = unique_workspace("knots-cli-exec-plan-next");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Execution plan",
            "--type",
            "execution_plan",
            "--objective",
            "Coordinate the rollout",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let next = run_knots(&root, &db, &["next", &knot_id, "--json"]);
    assert_success(&next);
    let next_json: Value = serde_json::from_slice(&next.stdout).expect("next json");
    assert_eq!(next_json["state"], "design");

    let shown = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let shown_json: Value = serde_json::from_slice(&shown.stdout).expect("show json");
    assert_eq!(shown_json["workflow_id"], "execution_plan_sdlc");
    assert_eq!(shown_json["profile_id"], "autopilot");
    assert_eq!(shown_json["state"], "design");
}

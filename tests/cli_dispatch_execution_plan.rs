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

#[test]
fn execution_plan_file_round_trips_through_show_json() {
    let root = unique_workspace("knots-cli-exec-plan-file");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &["new", "Execution plan carrier", "--type", "execution_plan"],
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
    assert_eq!(plan["repo_path"], "/repo");
    assert_eq!(plan["objective"], "Smoke-test execution plan persistence");
    assert_eq!(plan["mode"], "autopilot");
    assert_eq!(plan["model"], "smoke-tester");
    assert_eq!(
        plan["assumptions"],
        json!(["assume CLI dispatch stays wired"])
    );
    assert_eq!(plan["knot_ids"], json!([work_a_id, work_b_id]));

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

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn execution_plan_file_survives_rehydrate_after_hot_eviction() {
    let root = unique_workspace("knots-cli-exec-plan-rehydrate");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let created = run_knots(
        &root,
        &db,
        &["new", "Plan to rehydrate", "--type", "execution_plan"],
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

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn updating_work_knot_to_execution_plan_re_roots_workflow() {
    let root = unique_workspace("knots-cli-exec-plan-type-update");
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
    assert_success(&updated);

    let shown = run_knots(&root, &db, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let view: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    assert_eq!(view["type"], "execution_plan");
    assert_eq!(view["workflow_id"], "execution_plan_sdlc");
    assert_eq!(view["profile_id"], "autopilot");
    assert_eq!(view["state"], "ready_for_design");

    let _ = std::fs::remove_dir_all(root);
}

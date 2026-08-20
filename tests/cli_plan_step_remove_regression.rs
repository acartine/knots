mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;
use serde_json::Value;

fn create_work(root: &std::path::Path, db: &std::path::Path, title: &str) -> String {
    let created = run_knots(root, db, &["new", title, "-d", "x", "--acceptance", "x"]);
    assert_success(&created);
    parse_created_id(&created)
}

fn create_gate(root: &std::path::Path, db: &std::path::Path) -> String {
    let created = run_knots(
        root,
        db,
        &[
            "new",
            "gate 1",
            "--type",
            "gate",
            "--gate-owner-kind",
            "human",
            "-d",
            "x",
            "--acceptance",
            "x",
        ],
    );
    assert_success(&created);
    parse_created_id(&created)
}

fn add_wave(root: &std::path::Path, db: &std::path::Path, plan_id: &str, index: usize) {
    let name = format!("Wave {index}");
    let added = run_knots(
        root,
        db,
        &[
            "plan",
            "wave",
            "add",
            plan_id,
            "--name",
            &name,
            "--objective",
            "o",
        ],
    );
    assert_success(&added);
}

fn add_step(root: &std::path::Path, db: &std::path::Path, plan_id: &str, knot_id: &str) {
    let added = run_knots(
        root,
        db,
        &[
            "plan",
            "step",
            "add",
            plan_id,
            "--wave",
            "5",
            "--knot-ids",
            knot_id,
        ],
    );
    assert_success(&added);
}

fn show_plan(root: &std::path::Path, db: &std::path::Path, plan_id: &str) -> Value {
    let shown = run_knots(root, db, &["show", plan_id, "--json"]);
    assert_success(&shown);
    let view: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    view["execution_plan"].clone()
}

#[test]
fn plan_step_remove_after_edge_edits_keeps_remaining_waves() {
    let root_ws = unique_workspace("knots-cli-plan-remove-regression");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db);

    let plan = run_knots(
        &root,
        &db,
        &[
            "new",
            "Test plan",
            "--workflow",
            "execution_plan_sdlc",
            "--profile",
            "autopilot",
            "--type",
            "execution_plan",
            "--objective",
            "repro",
        ],
    );
    assert_success(&plan);
    let plan_id = parse_created_id(&plan);

    let work_1 = create_work(&root, &db, "work 1");
    let work_2 = create_work(&root, &db, "work 2");
    let work_3 = create_work(&root, &db, "work 3");
    let work_4 = create_work(&root, &db, "work 4");
    let gate_1 = create_gate(&root, &db);

    for index in 1..=5 {
        add_wave(&root, &db, &plan_id, index);
    }
    let wave_1 = run_knots(
        &root,
        &db,
        &[
            "plan",
            "step",
            "add",
            &plan_id,
            "--wave",
            "1",
            "--knot-ids",
            &work_1,
        ],
    );
    assert_success(&wave_1);
    for knot_id in [&work_2, &work_3, &work_4, &gate_1] {
        add_step(&root, &db, &plan_id, knot_id);
    }

    let before = show_plan(&root, &db, &plan_id);
    assert_eq!(before["waves"].as_array().expect("waves").len(), 5);
    assert_eq!(
        before["waves"][4]["steps"].as_array().expect("steps").len(),
        4
    );

    let new_leaf = create_work(&root, &db, "new leaf");
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &new_leaf, "blocked_by", &work_2],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &new_leaf, "blocked_by", &work_3],
    ));
    assert_success(&run_knots(
        &root,
        &db,
        &["edge", "add", &gate_1, "blocked_by", &new_leaf],
    ));

    let removed = run_knots(
        &root,
        &db,
        &[
            "plan", "step", "remove", &plan_id, "--wave", "5", "--step", "4", "--force",
        ],
    );
    assert_success(&removed);

    let after_remove = show_plan(&root, &db, &plan_id);
    assert_eq!(after_remove["waves"].as_array().expect("waves").len(), 5);
    assert_eq!(
        after_remove["waves"][4]["steps"]
            .as_array()
            .expect("steps")
            .len(),
        3
    );

    let invalid = run_knots(
        &root,
        &db,
        &[
            "plan", "step", "remove", &plan_id, "--wave", "5", "--step", "9", "--force",
        ],
    );
    assert_failure(&invalid);
    assert_eq!(show_plan(&root, &db, &plan_id), after_remove);
}

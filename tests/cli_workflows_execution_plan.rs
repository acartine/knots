use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn unique_workspace(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
}

fn knots_binary() -> PathBuf {
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    if configured.exists() {
        return std::fs::canonicalize(&configured).unwrap_or(configured);
    }
    let manifest_relative = Path::new(env!("CARGO_MANIFEST_DIR")).join(&configured);
    if manifest_relative.exists() {
        return std::fs::canonicalize(&manifest_relative).unwrap_or(manifest_relative);
    }
    configured
}

fn run_knots(repo_root: &Path, db_path: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(knots_binary())
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("HOME", home)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args)
        .output()
        .expect("knots command should run")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success but failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_created_id(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .expect("created output should include knot id")
        .to_string()
}

fn bootstrap_builtin_workflows(root: &Path, db: &Path, home: &Path) {
    for (knot_type, workflow_id) in [
        ("work", "work_sdlc"),
        ("gate", "gate_sdlc"),
        ("lease", "lease_sdlc"),
        ("explore", "explore_sdlc"),
        ("execution_plan", "execution_plan_sdlc"),
    ] {
        let output = run_knots(
            root,
            db,
            home,
            &["workflow", "use", workflow_id, "--type", knot_type],
        );
        assert_success(&output);
    }
}

#[test]
fn execution_plan_builtin_type_resolves_default_workflow_and_profile() {
    let root_ws = unique_workspace("knots-cli-execution-plan-builtin");
    let root = root_ws.path().to_path_buf();
    let home_ws = unique_workspace("knots-cli-execution-plan-builtin-home");
    let home = home_ws.path().to_path_buf();
    std::fs::create_dir_all(root.join(".knots")).expect(".knots dir should exist");
    let db = root.join(".knots/cache/state.sqlite");
    bootstrap_builtin_workflows(&root, &db, &home);

    let current = run_knots(
        &root,
        &db,
        &home,
        &["workflow", "current", "--type", "execution_plan", "--json"],
    );
    assert_success(&current);
    let current_json: Value = serde_json::from_slice(&current.stdout).expect("current json");
    assert_eq!(current_json["knot_type"], "execution_plan");
    assert_eq!(current_json["id"], "execution_plan_sdlc");
    assert_eq!(current_json["default_profile"], "autopilot");

    let created = run_knots(
        &root,
        &db,
        &home,
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

    let shown = run_knots(&root, &db, &home, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let shown_json: Value = serde_json::from_slice(&shown.stdout).expect("show json");
    assert_eq!(shown_json["type"], "execution_plan");
    assert_eq!(shown_json["workflow_id"], "execution_plan_sdlc");
    assert_eq!(shown_json["profile_id"], "autopilot");
    assert_eq!(shown_json["state"], "ready_for_design");
}

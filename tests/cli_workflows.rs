use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

fn knots_binary() -> PathBuf {
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    if configured.exists() {
        return std::fs::canonicalize(&configured).unwrap_or(configured);
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if !configured.is_absolute() {
            for ancestor in current_exe.ancestors().skip(1) {
                let candidate = ancestor.join(&configured);
                if candidate.exists() {
                    return std::fs::canonicalize(&candidate).unwrap_or(candidate);
                }
            }
        }
        if let Some(debug_dir) = current_exe.parent().and_then(|deps| deps.parent()) {
            for name in ["knots", "knots.exe"] {
                let candidate = debug_dir.join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .nth(1)
        .expect("created output should include knot id")
        .to_string()
}

const CUSTOM_BUNDLE: &str = r#"
[workflow]
name = "custom_flow"
version = 1
default_profile = "autopilot"

[states.ready_for_work]
display_name = "Ready for Work"
kind = "queue"

[states.work]
display_name = "Work"
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "work"

[states.ready_for_review]
display_name = "Ready for Review"
kind = "queue"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"

[states.done]
display_name = "Done"
kind = "terminal"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.abandoned]
display_name = "Abandoned"
kind = "terminal"

[steps.work_step]
queue = "ready_for_work"
action = "work"

[steps.review_step]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "work_step"
gate = "review_step"

[profiles.autopilot]
description = "Custom autopilot flow"
phases = ["main"]
output = "remote_main"

[prompts.work]
accept = ["Working change"]
body = """
# Work

Perform the work.
"""

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "deferred"

[prompts.review]
accept = ["Reviewed change"]
body = """
# Review

Review the work.
"""

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

#[test]
fn custom_workflow_install_use_and_runtime_flow() {
    let root = unique_workspace("knots-cli-workflows");
    let home = unique_workspace("knots-cli-workflows-home");
    std::fs::create_dir_all(root.join(".knots")).expect(".knots dir should exist");
    let db = root.join(".knots/cache/state.sqlite");
    let bundle_path = root.join("custom-flow.toml");
    std::fs::write(&bundle_path, CUSTOM_BUNDLE).expect("bundle should write");

    let install = run_knots(
        &root,
        &db,
        &home,
        &[
            "workflow",
            "install",
            bundle_path.to_str().expect("utf8 path"),
        ],
    );
    assert_success(&install);
    assert!(
        root.join(".knots/workflows/custom_flow/1/bundle.json")
            .exists(),
        "installed bundle should be copied into repo-local workflow registry"
    );

    let list = run_knots(&root, &db, &home, &["workflow", "list", "--json"]);
    assert_success(&list);
    let listed: Value = serde_json::from_slice(&list.stdout).expect("workflow list json");
    let ids = listed
        .as_array()
        .expect("workflow list should be an array")
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(ids.contains(&"compatibility"));
    assert!(ids.contains(&"custom_flow"));

    let use_workflow = run_knots(&root, &db, &home, &["workflow", "use", "custom_flow"]);
    assert_success(&use_workflow);

    let current = run_knots(&root, &db, &home, &["workflow", "current"]);
    assert_success(&current);
    let current_stdout = String::from_utf8_lossy(&current.stdout);
    let current_stdout = current_stdout.trim();
    assert!(current_stdout.starts_with("custom_flow v1 profile="));
    assert!(current_stdout.ends_with("autopilot"));

    let created = run_knots(&root, &db, &home, &["new", "Custom workflow knot"]);
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let shown = run_knots(&root, &db, &home, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let shown_json: Value = serde_json::from_slice(&shown.stdout).expect("show json");
    assert_eq!(shown_json["state"], "ready_for_work");
    assert_eq!(shown_json["workflow_id"], "custom_flow");
    assert!(shown_json["profile_id"]
        .as_str()
        .expect("profile id should exist")
        .ends_with("autopilot"));

    let claim = run_knots(&root, &db, &home, &["claim", &knot_id, "--json"]);
    assert_success(&claim);
    let claim_json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
    let prompt = claim_json["prompt"].as_str().expect("prompt should exist");
    assert!(prompt.contains("# Work"));
    assert!(prompt.contains("Perform the work."));
    assert!(prompt.contains("Working change"));
    assert_eq!(claim_json["state"], "work");

    let next = run_knots(
        &root,
        &db,
        &home,
        &["next", &knot_id, "--expected-state", "work", "--json"],
    );
    assert_success(&next);
    let next_json: Value = serde_json::from_slice(&next.stdout).expect("next json");
    assert_eq!(next_json["state"], "ready_for_review");
}

#[test]
fn workflow_commands_render_text_and_json_views() {
    let root = unique_workspace("knots-cli-workflow-views");
    let home = unique_workspace("knots-cli-workflow-views-home");
    std::fs::create_dir_all(root.join(".knots")).expect(".knots dir should exist");
    let db = root.join(".knots/cache/state.sqlite");
    let bundle_path = root.join("custom-flow.toml");
    std::fs::write(&bundle_path, CUSTOM_BUNDLE).expect("bundle should write");

    let install = run_knots(
        &root,
        &db,
        &home,
        &[
            "workflow",
            "install",
            bundle_path.to_str().expect("utf8 path"),
        ],
    );
    assert_success(&install);
    assert!(String::from_utf8_lossy(&install.stdout).contains("installed workflow: custom_flow"));

    let list_text = run_knots(&root, &db, &home, &["workflow", "list"]);
    assert_success(&list_text);
    let list_stdout = String::from_utf8_lossy(&list_text.stdout);
    assert!(list_stdout.contains("compatibility v1"));
    assert!(list_stdout.contains("custom_flow v1 (current)"));

    let show_text = run_knots(&root, &db, &home, &["workflow", "show", "custom_flow"]);
    assert_success(&show_text);
    let show_stdout = String::from_utf8_lossy(&show_text.stdout);
    assert!(show_stdout.contains("workflow: custom_flow"));
    assert!(show_stdout.contains("version: 1"));
    assert!(show_stdout.contains("default_profile: autopilot"));
    assert!(show_stdout.contains("builtin: false"));

    let show_json = run_knots(
        &root,
        &db,
        &home,
        &["workflow", "show", "custom_flow", "--json"],
    );
    assert_success(&show_json);
    let show_json: Value = serde_json::from_slice(&show_json.stdout).expect("show json");
    assert_eq!(show_json["id"], "custom_flow");
    assert_eq!(show_json["version"], 1);

    let current_json = run_knots(&root, &db, &home, &["workflow", "current", "--json"]);
    assert_success(&current_json);
    let current_json: Value = serde_json::from_slice(&current_json.stdout).expect("current json");
    assert_eq!(current_json["id"], "custom_flow");
    assert_eq!(current_json["version"], 1);
    assert_eq!(current_json["current_profile"], "autopilot");

    let use_builtin = run_knots(&root, &db, &home, &["workflow", "use", "compatibility"]);
    assert_success(&use_builtin);
    let use_stdout = String::from_utf8_lossy(&use_builtin.stdout);
    assert!(use_stdout.contains("current workflow: compatibility v1"));

    let current_text = run_knots(&root, &db, &home, &["workflow", "current"]);
    assert_success(&current_text);
    let current_stdout = String::from_utf8_lossy(&current_text.stdout);
    assert!(current_stdout.contains("compatibility v1 profile=autopilot"));
}

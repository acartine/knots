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
    let manifest_relative = Path::new(env!("CARGO_MANIFEST_DIR")).join(&configured);
    if manifest_relative.exists() {
        return std::fs::canonicalize(&manifest_relative).unwrap_or(manifest_relative);
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

const ALT_QUEUE_NAMES_BUNDLE: &str = r#"
[workflow]
name = "alt_flow"
version = 1
default_profile = "autopilot"

[states.triage]
display_name = "Triage"
kind = "queue"

[states.build]
display_name = "Build"
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "build"

[states.qa_handoff]
display_name = "QA Handoff"
kind = "queue"

[states.approve]
display_name = "Approve"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "approve"

[states.done]
display_name = "Done"
kind = "terminal"

[states.blocked]
display_name = "Blocked"
kind = "escape"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.abandoned]
display_name = "Abandoned"
kind = "terminal"

[steps.build_step]
queue = "triage"
action = "build"

[steps.approve_step]
queue = "qa_handoff"
action = "approve"

[phases.main]
produce = "build_step"
gate = "approve_step"

[profiles.autopilot]
description = "Alt custom flow"
phases = ["main"]
output = "remote_main"

[prompts.build]
accept = ["Built change"]
body = """
# Build

Do the build work.
"""

[prompts.build.success]
complete = "qa_handoff"

[prompts.build.failure]
blocked = "blocked"

[prompts.approve]
accept = ["Approved change"]
body = """
# Approve

Approve the work.
"""

[prompts.approve.success]
approved = "done"

[prompts.approve.failure]
changes = "triage"
"#;

#[test]
fn custom_workflow_supports_non_ready_queue_names() {
    let root = unique_workspace("knots-cli-workflow-generic-queues");
    let home = unique_workspace("knots-cli-workflow-generic-queues-home");
    std::fs::create_dir_all(root.join(".knots")).expect(".knots dir should exist");
    let db = root.join(".knots/cache/state.sqlite");
    let bundle_path = root.join("alt-flow.toml");
    std::fs::write(&bundle_path, ALT_QUEUE_NAMES_BUNDLE).expect("bundle should write");

    let install = run_knots(
        &root,
        &db,
        &home,
        &[
            "workflow",
            "install",
            "--type",
            "work",
            bundle_path.to_str().expect("utf8 path"),
        ],
    );
    assert_success(&install);
    assert_success(&run_knots(
        &root,
        &db,
        &home,
        &["workflow", "use", "alt_flow"],
    ));

    let created = run_knots(&root, &db, &home, &["new", "Alt workflow knot"]);
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let shown = run_knots(&root, &db, &home, &["show", &knot_id, "--json"]);
    assert_success(&shown);
    let shown_json: Value = serde_json::from_slice(&shown.stdout).expect("show json");
    assert_eq!(shown_json["state"], "triage");

    assert_claim_and_advance(&root, &db, &home, &knot_id);
}

fn assert_claim_and_advance(root: &Path, db: &Path, home: &Path, knot_id: &str) {
    let claim = run_knots(root, db, home, &["claim", knot_id, "--json"]);
    assert_success(&claim);
    let claim_json: Value = serde_json::from_slice(&claim.stdout).expect("claim json");
    assert_eq!(claim_json["state"], "build");
    assert!(claim_json["prompt"]
        .as_str()
        .expect("prompt should exist")
        .contains("# Build"));

    let next = run_knots(
        root,
        db,
        home,
        &["next", knot_id, "--expected-state", "build", "--json"],
    );
    assert_success(&next);
    let next_json: Value = serde_json::from_slice(&next.stdout).expect("next json");
    assert_eq!(next_json["state"], "qa_handoff");

    let claim_review = run_knots(root, db, home, &["claim", knot_id, "--json"]);
    assert_success(&claim_review);
    let claim_review_json: Value =
        serde_json::from_slice(&claim_review.stdout).expect("claim review json");
    assert_eq!(claim_review_json["state"], "approve");

    let rollback = run_knots(root, db, home, &["rollback", knot_id]);
    assert_success(&rollback);
    let after = run_knots(root, db, home, &["show", knot_id, "--json"]);
    assert_success(&after);
    let after_json: Value = serde_json::from_slice(&after.stdout).expect("show json");
    assert_eq!(after_json["state"], "triage");
}

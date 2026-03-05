use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
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
        if let Some(debug_dir) = current_exe.parent().and_then(|deps| deps.parent()) {
            let unix_candidate = debug_dir.join("knots");
            if unix_candidate.exists() {
                return unix_candidate;
            }
            let windows_candidate = debug_dir.join("knots.exe");
            if windows_candidate.exists() {
                return windows_candidate;
            }
        }
    }

    configured
}

fn configure_coverage_env(command: &mut Command) {
    let _ = command;
}

fn run_knots(repo_root: &Path, db_path: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(knots_binary());
    command
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args);
    configure_coverage_env(&mut command);
    command.output().expect("knots command should run")
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

fn read_event_payloads(root: &Path, event_type: &str) -> Vec<Value> {
    let mut payloads = Vec::new();
    let mut stack = vec![root.join(".knots/events")];
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir).expect("events directory should read") {
            let path = entry.expect("dir entry should read").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let payload = std::fs::read(&path).expect("event file should read");
            let value: Value = serde_json::from_slice(&payload).expect("event should parse");
            if value.get("type").and_then(Value::as_str) == Some(event_type) {
                payloads.push(value);
            }
        }
    }
    payloads
}

#[test]
fn next_rejects_stale_expected_state() {
    let root = unique_workspace("knots-next-optimistic");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Optimistic next",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_plan_review",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let first_next = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_id,
            "--expected-state",
            "ready_for_plan_review",
            "--json",
        ],
    );
    assert_success(&first_next);
    let first_json: Value =
        serde_json::from_slice(&first_next.stdout).expect("first next json should parse");
    assert_eq!(first_json["previous_state"], "ready_for_plan_review");
    assert_eq!(first_json["state"], "plan_review");

    let stale_next = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_id,
            "--expected-state",
            "ready_for_plan_review",
            "--json",
        ],
    );
    assert!(
        !stale_next.status.success(),
        "stale optimistic next should fail.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stale_next.stdout),
        String::from_utf8_lossy(&stale_next.stderr)
    );
    let stale_stderr = String::from_utf8_lossy(&stale_next.stderr);
    assert!(
        stale_stderr.contains("expected state 'ready_for_plan_review' but knot is currently"),
        "stale optimistic next should report mismatch: {stale_stderr}"
    );

    let state_events = read_event_payloads(&root, "knot.state_set");
    assert_eq!(
        state_events.len(),
        1,
        "stale optimistic next should not create another state_set event"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn next_preserves_first_metadata_when_followup_request_is_stale() {
    let root = unique_workspace("knots-next-metadata");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Metadata winner",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_plan_review",
        ],
    );
    assert_success(&created);
    let knot_id = parse_created_id(&created);

    let first = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_id,
            "--expected-state",
            "ready_for_plan_review",
            "--actor-kind",
            "agent",
            "--agent-name",
            "first-agent",
        ],
    );
    assert_success(&first);

    let stale = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_id,
            "--expected-state",
            "ready_for_plan_review",
            "--actor-kind",
            "robot",
            "--agent-name",
            "second-agent",
        ],
    );
    assert!(
        !stale.status.success(),
        "stale optimistic next should fail.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stale.stdout),
        String::from_utf8_lossy(&stale.stderr)
    );

    let state_events = read_event_payloads(&root, "knot.state_set");
    assert_eq!(
        state_events.len(),
        1,
        "only the first queued transition should write state_set"
    );

    let event_data = state_events[0]
        .get("data")
        .and_then(Value::as_object)
        .expect("state_set event should have data");
    assert_eq!(
        event_data.get("agent_name").and_then(Value::as_str),
        Some("first-agent")
    );
    assert_eq!(
        event_data.get("actor_kind").and_then(Value::as_str),
        Some("agent")
    );

    let _ = std::fs::remove_dir_all(root);
}

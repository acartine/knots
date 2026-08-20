use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn unique_workspace(prefix: &str) -> knots_test_support::TestWorkspace {
    knots_test_support::workspace(prefix)
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
    let root_ws = unique_workspace("knots-next-optimistic");
    let root = root_ws.path().to_path_buf();
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
}

fn setup_knot_with_first_agent_lease(root: &Path, db: &Path) -> (String, String) {
    let created = run_knots(
        root,
        db,
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

    let lease_create = run_knots(
        root,
        db,
        &[
            "lease",
            "create",
            "--nickname",
            "first-agent-session",
            "--agent-name",
            "first-agent",
            "--model",
            "opus",
            "--model-version",
            "4.7",
            "--agent-type",
            "cli",
            "--provider",
            "Anthropic",
            "--json",
        ],
    );
    assert_success(&lease_create);
    let lease_json: Value =
        serde_json::from_slice(&lease_create.stdout).expect("lease create json");
    let lease_id = lease_json["id"]
        .as_str()
        .expect("lease id in json")
        .to_string();

    let claim = run_knots(
        root,
        db,
        &["claim", &knot_id, "--lease", &lease_id, "--json"],
    );
    assert_success(&claim);
    (knot_id, lease_id)
}

fn assert_state_events_carry_lease_identity(root: &Path, knot_id: &str) {
    let state_events = read_event_payloads(root, "knot.state_set");
    // The claim and `next` steps each emit state_set events for the knot.
    // The stored `knot_id` is the full (non-truncated) id, so match by
    // suffix when the CLI printed a stripped id. Sort by `occurred_at` so
    // the last element is the most recent (the `next` transition).
    let mut knot_events: Vec<&Value> = state_events
        .iter()
        .filter(|event| {
            event
                .get("knot_id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == knot_id || id.ends_with(knot_id))
        })
        .collect();
    knot_events.sort_by_key(|event| {
        event
            .get("occurred_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    });
    assert!(
        !knot_events.is_empty(),
        "expected at least one state_set event for the knot"
    );
    for event in &knot_events {
        let event_data = event
            .get("data")
            .and_then(Value::as_object)
            .expect("state_set event should have data");
        assert_eq!(
            event_data.get("agent_name").and_then(Value::as_str),
            Some("first-agent"),
            "agent_name should come from the bound lease on every step"
        );
        assert_eq!(
            event_data.get("actor_kind").and_then(Value::as_str),
            Some("agent"),
            "actor_kind should be 'agent' on every step"
        );
    }
}

#[test]
fn next_preserves_first_metadata_when_followup_request_is_stale() {
    // Lease-declared identity: agent_name on the recorded state_set event
    // comes from the bound lease's agent_info, not from a deprecated
    // `--agent-name` flag. The test creates a lease with agent_name
    // "first-agent", binds it to the knot via `kno claim`, runs `next`, and
    // verifies the event carries "first-agent" from the lease.
    let root_ws = unique_workspace("knots-next-metadata");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let (knot_id, lease_id) = setup_knot_with_first_agent_lease(&root, &db);

    let first = run_knots(
        &root,
        &db,
        &[
            "next",
            &knot_id,
            "--expected-state",
            "plan_review",
            "--actor-kind",
            "agent",
            "--lease",
            &lease_id,
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
            "plan_review",
            "--actor-kind",
            "agent",
            "--lease",
            &lease_id,
        ],
    );
    assert!(
        !stale.status.success(),
        "stale optimistic next should fail.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stale.stdout),
        String::from_utf8_lossy(&stale.stderr)
    );

    assert_state_events_carry_lease_identity(&root, &knot_id);
}

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
    run_knots_with_path(repo_root, db_path, args, None)
}

fn run_knots_with_current_dir(
    current_dir: &Path,
    repo_root: &Path,
    db_path: Option<&Path>,
    args: &[&str],
) -> Output {
    let mut command = Command::new(knots_binary());
    command
        .current_dir(current_dir)
        .arg("-C")
        .arg(repo_root)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1");
    if let Some(db_path) = db_path {
        command.arg("--db").arg(db_path);
    }
    command.args(args);
    configure_coverage_env(&mut command);
    command.output().expect("knots command should run")
}

fn run_knots_with_path(
    repo_root: &Path,
    db_path: &Path,
    args: &[&str],
    path_override: Option<&Path>,
) -> Output {
    let mut command = Command::new(knots_binary());
    command
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args);
    if let Some(path) = path_override {
        command.env("KNOTS_LOOM_BIN", path.join("loom"));
    }
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

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure but command succeeded.\nstdout:\n{}\nstderr:\n{}",
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

fn install_stub_loom(root: &Path) -> PathBuf {
    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    let bundle = r#"
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

[states.blocked]
display_name = "Blocked"
kind = "escape"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.done]
display_name = "Done"
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
blocked = "blocked"

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
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo 'loom 0.1.0'; exit 0; fi\n\
         if [ \"$1\" = \"init\" ]; then test -n \"$2\" || exit 1; touch loom.toml; exit 0; fi\n\
         if [ \"$1\" = \"validate\" ]; then exit 0; fi\n\
         if [ \"$1\" = \"build\" ]; then\n\
           cat <<'EOF'\n\
{bundle}\n\
EOF\n\
           exit 0\n\
         fi\n\
         echo 'unexpected args' >&2\n\
         exit 1\n"
    );
    let loom = bin_dir.join("loom");
    std::fs::write(&loom, script).expect("loom script should write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&loom).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&loom, perms).expect("permissions");
    }
    bin_dir
}

#[test]
fn toplevel_help_uses_custom_help_path() {
    let root = unique_workspace("knots-main-help");
    setup_repo(&root);

    let mut command = Command::new(knots_binary());
    command
        .current_dir(&root)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1");
    configure_coverage_env(&mut command);
    let output = command.output().expect("knots command should run");
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Common Commands:"), "stdout: {stdout}");
    assert!(stdout.contains("Other Commands:"), "stdout: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fsck_non_json_failure_prints_issue_rows() {
    let root = unique_workspace("knots-main-fsck-issues");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Broken fsck input",
            "--profile",
            "autopilot",
            "--state",
            "idea",
        ],
    );
    assert_success(&created);

    let bad_file = root.join(".knots/index/bad-event.json");
    std::fs::create_dir_all(
        bad_file
            .parent()
            .expect("bad fsck file should always have a parent"),
    )
    .expect("index directory should be creatable");
    std::fs::write(&bad_file, "{ this is not valid json").expect("invalid fsck file should write");

    let fsck = run_knots(&root, &db, &["fsck"]);
    assert_failure(&fsck);
    let stdout = String::from_utf8_lossy(&fsck.stdout);
    let stderr = String::from_utf8_lossy(&fsck.stderr);
    assert!(stdout.contains("issues="), "stdout: {stdout}");
    assert!(stdout.contains("invalid JSON payload"), "stdout: {stdout}");
    assert!(stderr.contains("fsck found"), "stderr: {stderr}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ready_claim_peek_skill_terminal_and_rehydrate_missing_paths() {
    let root = unique_workspace("knots-main-branches");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let ready_knot = run_knots(
        &root,
        &db,
        &[
            "new",
            "Peek candidate",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&ready_knot);
    let ready_id = parse_created_id(&ready_knot);

    let ready = run_knots(&root, &db, &["ready"]);
    assert_success(&ready);
    assert!(
        String::from_utf8_lossy(&ready.stdout).contains("Peek candidate"),
        "ready should include knot title"
    );

    let peek = run_knots(&root, &db, &["claim", &ready_id, "--peek"]);
    assert_success(&peek);

    let shown = run_knots(&root, &db, &["show", &ready_id, "--json"]);
    assert_success(&shown);
    let shown_json: Value = serde_json::from_slice(&shown.stdout).expect("show should return json");
    assert_eq!(shown_json["state"], "ready_for_implementation");

    let shipped = run_knots(
        &root,
        &db,
        &[
            "new",
            "Terminal skill",
            "--profile",
            "autopilot",
            "--state",
            "shipped",
        ],
    );
    assert_success(&shipped);
    let shipped_id = parse_created_id(&shipped);

    let skill_terminal = run_knots(&root, &db, &["skill", &shipped_id]);
    assert_failure(&skill_terminal);
    assert!(
        String::from_utf8_lossy(&skill_terminal.stderr).contains("no next state"),
        "terminal skill should report no next state"
    );

    let missing_rehydrate = run_knots(&root, &db, &["rehydrate", "missing-id"]);
    assert_failure(&missing_rehydrate);
    assert!(
        String::from_utf8_lossy(&missing_rehydrate.stderr).contains("not found"),
        "rehydrate missing should return not found"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn hooks_status_command_dispatches_through_main() {
    let root = unique_workspace("knots-main-hooks-dispatch");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let output = run_knots(&root, &db, &["hooks", "status"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("missing"), "stdout: {stdout}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn loom_compat_test_dispatches_through_main() {
    let root = unique_workspace("knots-main-loom-dispatch");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");
    let bin_dir = install_stub_loom(&root);

    let output = run_knots_with_path(
        &root,
        &db,
        &["loom", "compat-test", "--mode", "matrix"],
        Some(&bin_dir),
    );
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("loom compat-test custom_flow matrix"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("success -> ready_for_review"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("blocked -> blocked"), "stdout: {stdout}");

    let json = run_knots_with_path(
        &root,
        &db,
        &["loom", "compat-test", "--json"],
        Some(&bin_dir),
    );
    assert_success(&json);
    let parsed: Value = serde_json::from_slice(&json.stdout).expect("loom json should parse");
    assert_eq!(parsed["workflow_id"], "custom_flow");
    assert_eq!(parsed["mode"], "smoke");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn repo_root_flag_resolves_default_db_relative_to_repo() {
    let root = unique_workspace("knots-main-repo-root-db");
    let outside = unique_workspace("knots-main-repo-root-db-outside");
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let created = run_knots(
        &root,
        &db,
        &[
            "new",
            "Repo root default db",
            "--profile",
            "autopilot",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let id = parse_created_id(&created);

    let shown = run_knots_with_current_dir(&outside, &root, None, &["show", &id, "--json"]);
    assert_success(&shown);
    let parsed: Value = serde_json::from_slice(&shown.stdout).expect("show json should parse");
    assert_eq!(parsed["title"], "Repo root default db");
    let shown_id = parsed["id"].as_str().expect("show should return string id");
    assert!(
        shown_id.ends_with(&format!("-{id}")),
        "show id {shown_id} should end with created suffix {id}"
    );

    let _ = std::fs::remove_dir_all(outside);
    let _ = std::fs::remove_dir_all(root);
}

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[cfg(unix)]
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use uuid::Uuid;

fn unique_dir(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
}

fn knots_binary() -> PathBuf {
    let configured = PathBuf::from(env!("CARGO_BIN_EXE_knots"));
    if configured.is_absolute() && configured.exists() {
        return configured;
    }
    std::fs::canonicalize(&configured).unwrap_or(configured)
}

fn run_knots(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(knots_binary())
        .current_dir(cwd)
        .env("HOME", home)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args)
        .output()
        .expect("knots command should run")
}

fn run_knots_with_input(home: &Path, cwd: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(knots_binary())
        .current_dir(cwd)
        .env("HOME", home)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("knots command should spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin should be available")
        .write_all(input.as_bytes())
        .expect("stdin should be writable");
    child
        .wait_with_output()
        .expect("knots command should finish")
}

#[cfg(unix)]
fn run_knots_in_pty(home: &Path, cwd: &Path, args: &[&str], input: &str) -> (bool, String) {
    use std::io::Read;

    let pty_system = NativePtySystem::default();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("pty should open");

    let mut cmd = CommandBuilder::new(knots_binary());
    cmd.cwd(cwd);
    cmd.env("HOME", home);
    cmd.env("KNOTS_SKIP_DOCTOR_UPGRADE", "1");
    for arg in args {
        cmd.arg(arg);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .expect("knots command should spawn in pty");
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("pty reader should clone");
    let mut writer = pair.master.take_writer().expect("pty writer should open");
    writer
        .write_all(input.as_bytes())
        .expect("pty input should write");
    drop(writer);

    let status = child.wait().expect("pty child should finish");
    let mut output = String::new();
    reader
        .read_to_string(&mut output)
        .expect("pty output should read");
    (status.success(), output)
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
    std::fs::write(root.join("README.md"), "# repo\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

fn app_data_root(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("knots")
    } else if cfg!(target_os = "windows") {
        home.join("AppData").join("Roaming").join("knots")
    } else {
        home.join(".local").join("share").join("knots")
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "expected failure\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_created_id(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .expect("created output should include id")
        .to_string()
}

#[test]
fn init_project_creates_app_data_without_workspace_knots_dir() {
    let home = unique_dir("knots-home");
    let workspace = unique_dir("knots-workspace");

    let output = run_knots(&home, &workspace, &["--project", "demo", "init"]);
    assert_success(&output);

    let config = std::fs::read_to_string(home.join(".config/knots/config.toml"))
        .expect("config should exist");
    assert!(config.contains("active_project = \"demo\""));

    let store_root = app_data_root(&home).join("projects/demo");
    assert!(store_root.join("cache/state.sqlite").exists());
    assert!(!workspace.join(".knots").exists());
}

#[test]
fn active_project_allows_commands_outside_workspace() {
    let home = unique_dir("knots-home-active");
    let workspace = unique_dir("knots-workspace-active");
    let outside = unique_dir("knots-outside-active");

    assert_success(&run_knots(
        &home,
        &workspace,
        &["--project", "demo", "init"],
    ));

    let created = run_knots(
        &home,
        &outside,
        &[
            "new",
            "Named project task",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let id = parse_created_id(&created);

    let shown = run_knots(&home, &outside, &["show", &id, "--json"]);
    assert_success(&shown);
    let stdout = String::from_utf8_lossy(&shown.stdout);
    assert!(stdout.contains("Named project task"));

    let store_root = app_data_root(&home).join("projects/demo");
    assert!(store_root.join("events").exists());
    assert!(store_root.join("index").exists());
    assert!(!workspace.join(".knots").exists());
}

#[test]
fn explicit_project_rejects_unknown_id() {
    let home = unique_dir("knots-home-missing");
    let cwd = unique_dir("knots-cwd-missing");

    let output = run_knots(&home, &cwd, &["--project", "missing", "ls"]);
    assert_failure(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown project 'missing'"));
}

#[test]
fn sync_is_rejected_for_local_only_projects() {
    let home = unique_dir("knots-home-sync");
    let workspace = unique_dir("knots-workspace-sync");

    assert_success(&run_knots(
        &home,
        &workspace,
        &["--project", "demo", "init"],
    ));
    let output = run_knots(&home, &workspace, &["sync"]);
    assert_failure(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("sync is not supported"));
}

#[test]
fn explicit_c_repo_overrides_active_named_project() {
    let home = unique_dir("knots-home-override");
    let workspace = unique_dir("knots-workspace-override");
    let repo = unique_dir("knots-repo-override");
    setup_repo(&repo);

    assert_success(&run_knots(
        &home,
        &workspace,
        &["--project", "demo", "init"],
    ));

    let created = run_knots(
        &home,
        &workspace,
        &[
            "-C",
            repo.to_str().expect("utf8 repo path"),
            "--db",
            repo.join(".knots/cache/state.sqlite")
                .to_str()
                .expect("utf8 db path"),
            "new",
            "Repo override task",
            "--state",
            "ready_for_implementation",
        ],
    );
    assert_success(&created);
    let id = parse_created_id(&created);

    let shown = run_knots(
        &home,
        &workspace,
        &[
            "-C",
            repo.to_str().expect("utf8 repo path"),
            "--db",
            repo.join(".knots/cache/state.sqlite")
                .to_str()
                .expect("utf8 db path"),
            "show",
            &id,
            "--json",
        ],
    );
    assert_success(&shown);
    assert!(String::from_utf8_lossy(&shown.stdout).contains("Repo override task"));
    assert!(repo.join(".knots").exists());
}

#[test]
fn project_delete_requires_matching_confirmation() {
    let home = unique_dir("knots-home-delete-confirm");
    let workspace = unique_dir("knots-workspace-delete-confirm");

    assert_success(&run_knots(
        &home,
        &workspace,
        &["--project", "demo", "init"],
    ));
    let store_root = app_data_root(&home).join("projects/demo");
    let output = run_knots_with_input(&home, &workspace, &["project", "delete", "demo"], "nope\n");

    assert_failure(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("confirmation did not match"));
    assert!(store_root.exists());
    assert!(home.join(".config/knots/projects/demo.toml").exists());
}

#[test]
fn project_delete_removes_store_and_project_record() {
    let home = unique_dir("knots-home-delete");
    let workspace = unique_dir("knots-workspace-delete");

    assert_success(&run_knots(
        &home,
        &workspace,
        &["--project", "demo", "init"],
    ));
    let store_root = app_data_root(&home).join("projects/demo");
    let output = run_knots_with_input(&home, &workspace, &["project", "delete", "demo"], "demo\n");

    assert_success(&output);
    assert!(!store_root.exists());
    assert!(!home.join(".config/knots/projects/demo.toml").exists());
    let config = std::fs::read_to_string(home.join(".config/knots/config.toml"))
        .expect("config should still exist");
    assert!(!config.contains("active_project"));
}

#[test]
fn project_delete_yes_skips_confirmation_prompt() {
    let home = unique_dir("knots-home-delete-yes");
    let workspace = unique_dir("knots-workspace-delete-yes");

    assert_success(&run_knots(
        &home,
        &workspace,
        &["--project", "demo", "init"],
    ));
    let output = run_knots(&home, &workspace, &["project", "delete", "demo", "--yes"]);

    assert_success(&output);
    assert!(!home.join(".config/knots/projects/demo.toml").exists());
}

#[cfg(unix)]
#[test]
fn project_select_works_interactively_with_a_tty() {
    let home = unique_dir("knots-home-select");
    let workspace = unique_dir("knots-workspace-select");

    assert_success(&run_knots(
        &home,
        &workspace,
        &["project", "create", "alpha"],
    ));
    assert_success(&run_knots(
        &home,
        &workspace,
        &["project", "create", "beta"],
    ));

    let (success, output) = run_knots_in_pty(&home, &workspace, &["project", "select"], "2\n");
    assert!(success, "expected success from PTY select:\n{output}");

    let config = std::fs::read_to_string(home.join(".config/knots/config.toml"))
        .expect("config should exist");
    assert!(config.contains("active_project = \"beta\""));
}

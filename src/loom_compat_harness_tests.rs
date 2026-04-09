use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::app::AppError;
use crate::loom_compat_harness::{run_compat_test, CompatTestConfig, CompatTestMode};

const SAMPLE_BUNDLE: &str = r#"
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

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

fn install_stub_loom(root: &Path, bundle: &str, validate_failure: Option<&str>) -> PathBuf {
    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then\n\
           echo 'loom 0.1.0'\n\
           exit 0\n\
         fi\n\
         if [ \"$1\" = \"init\" ]; then\n\
           test -n \"$2\" || exit 1\n\
           touch loom.toml\n\
           exit 0\n\
         fi\n\
         if [ \"$1\" = \"validate\" ]; then\n\
           {validate}\n\
           exit 0\n\
         fi\n\
         if [ \"$1\" = \"build\" ]; then\n\
           cat <<'EOF'\n\
{bundle}\n\
EOF\n\
           exit 0\n\
         fi\n\
         echo 'unexpected args' >&2\n\
         exit 1\n",
        validate = validate_failure.map_or(String::new(), |message| {
            format!("echo '{}' >&2\nexit 1", message.replace('\'', ""))
        }),
    );
    install_script(&bin_dir, &script)
}

fn install_script(bin_dir: &Path, script: &str) -> PathBuf {
    let loom = bin_dir.join("loom");
    std::fs::write(&loom, script).expect("loom script should write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&loom).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&loom, perms).expect("permissions");
    }
    bin_dir.to_path_buf()
}

fn loom_bin(bin_dir: &Path) -> PathBuf {
    bin_dir.join("loom")
}

fn invalid_argument(err: AppError) -> String {
    match err {
        AppError::InvalidArgument(message) => message,
        other => other.to_string(),
    }
}

#[test]
fn invalid_argument_helper_formats_non_argument_errors() {
    let rendered = invalid_argument(AppError::NotFound("missing".to_string()));
    assert!(rendered.contains("missing"));
}

#[test]
fn compat_harness_reports_missing_loom() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-missing");

    let err = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(root.join("missing-loom")),
    })
    .expect_err("missing loom should fail");
    assert!(invalid_argument(err).contains("loom is not discoverable"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_reports_loom_execution_errors() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-exec-error");
    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");

    let err = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(bin_dir),
    })
    .expect_err("directory loom path should fail");
    assert!(invalid_argument(err).contains("failed to execute loom --version"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_reports_invalid_bundle_output() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-invalid-bundle");
    let bin_dir = install_stub_loom(&root, "not valid toml", None);

    let err = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect_err("invalid bundle should fail");
    assert!(err.to_string().contains("invalid workflow bundle"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_preserves_workspace_when_requested() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-keep");
    let bin_dir = install_stub_loom(&root, SAMPLE_BUNDLE, None);

    let result = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Matrix,
        keep_artifacts: true,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect("compat run should succeed");
    assert!(result.success);
    assert_eq!(result.scenarios.len(), 2);
    assert!(result
        .scenarios
        .iter()
        .all(|scenario| scenario.prompt_verified));
    assert!(result
        .workspace_path
        .as_deref()
        .expect("workspace should be kept")
        .exists());

    let _ = std::fs::remove_dir_all(root);
    if let Some(workspace) = result.workspace_path {
        let _ = std::fs::remove_dir_all(workspace);
    }
}

#[test]
fn compat_harness_uses_stable_serializable_output() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-json");
    let bin_dir = install_stub_loom(&root, SAMPLE_BUNDLE, None);

    let result = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect("compat run should succeed");
    let json = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(json["mode"], "smoke");
    assert_eq!(json["workflow_id"], "custom_flow");
    assert_eq!(json["success"], true);
    assert_eq!(json["scenarios"][0]["actual_state"], "ready_for_review");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_drops_workspace_when_keep_artifacts_is_disabled() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-clean");
    let bin_dir = install_stub_loom(&root, SAMPLE_BUNDLE, None);

    let result = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect("compat run should succeed");
    assert!(result.success);
    assert!(result.workspace_path.is_none());
    assert_eq!(result.scenarios.len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_reports_validate_command_failures() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-validate-fail");
    let bin_dir = install_stub_loom(&root, SAMPLE_BUNDLE, Some("validate exploded"));

    let err = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect_err("validate failure should bubble up");
    let message = invalid_argument(err);
    assert!(message.contains("loom validate failed in"));
    assert!(message.contains("validate exploded"));
    assert!(message.contains("/package"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_reports_validate_failures_without_stderr() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-validate-empty");
    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    install_script(
        &bin_dir,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo 'loom 0.1.0'; exit 0; fi\n\
         if [ \"$1\" = \"init\" ]; then test -n \"$2\" || exit 1; touch loom.toml; exit 0; fi\n\
         if [ \"$1\" = \"validate\" ]; then exit 1; fi\n\
         if [ \"$1\" = \"build\" ]; then exit 0; fi\n\
         exit 1\n",
    );

    let err = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect_err("validate failure should bubble up");
    let message = invalid_argument(err);
    assert!(message.contains("loom validate failed"));
    assert!(!message.contains("failed:"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_reports_invalid_utf8_from_build_output() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-invalid-utf8");
    let bin_dir = root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("bin dir should exist");
    install_script(
        &bin_dir,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo 'loom 0.1.0'; exit 0; fi\n\
         if [ \"$1\" = \"init\" ]; then test -n \"$2\" || exit 1; touch loom.toml; exit 0; fi\n\
         if [ \"$1\" = \"validate\" ]; then exit 0; fi\n\
         if [ \"$1\" = \"build\" ]; then printf '\\377\\376'; exit 0; fi\n\
         exit 1\n",
    );

    let err = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect_err("invalid utf-8 should fail");
    assert!(invalid_argument(err).contains("produced invalid UTF-8"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_harness_reports_builtin_source_in_result() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_workspace("knots-loom-builtin-source");
    let bin_dir = install_stub_loom(&root, SAMPLE_BUNDLE, None);

    let result = run_compat_test(&CompatTestConfig {
        mode: CompatTestMode::Smoke,
        keep_artifacts: false,
        loom_bin: Some(loom_bin(&bin_dir)),
    })
    .expect("compat run should succeed");
    assert_eq!(result.source, PathBuf::from("<builtin:work_sdlc>"));

    let _ = std::fs::remove_dir_all(root);
}

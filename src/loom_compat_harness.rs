use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use uuid::Uuid;

use crate::app::{App, AppError, StateActorMetadata};
use crate::db;
use crate::domain::knot_type::KnotType;
use crate::installed_workflows::{self, InstalledWorkflowRegistry, PromptDefinition};
use crate::poll_claim;
use crate::workflow_runtime;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatTestMode {
    Smoke,
    Matrix,
}

impl fmt::Display for CompatTestMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Smoke => write!(f, "smoke"),
            Self::Matrix => write!(f, "matrix"),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompatTestConfig {
    pub source: PathBuf,
    pub mode: CompatTestMode,
    pub keep_artifacts: bool,
    pub loom_bin: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StepResult {
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScenarioResult {
    pub outcome: String,
    pub expected_state: String,
    pub actual_state: String,
    pub prompt_verified: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TestResult {
    pub success: bool,
    pub mode: CompatTestMode,
    pub source: PathBuf,
    pub workflow_id: String,
    pub workspace_path: Option<PathBuf>,
    pub steps: Vec<StepResult>,
    pub scenarios: Vec<ScenarioResult>,
}

pub fn run_compat_test(config: &CompatTestConfig) -> Result<TestResult, AppError> {
    let source = std::fs::canonicalize(&config.source).map_err(|err| {
        AppError::InvalidArgument(format!(
            "invalid Loom source '{}': {err}",
            config.source.display()
        ))
    })?;
    if !source.is_dir() {
        return Err(AppError::InvalidArgument(format!(
            "loom compat-test source must be a directory: {}",
            source.display()
        )));
    }

    let workspace = unique_workspace();
    let result = run_compat_test_inner(config, &source, &workspace);
    if config.keep_artifacts {
        return result.map(|mut ok| {
            ok.workspace_path = Some(workspace);
            ok
        });
    }
    let _ = std::fs::remove_dir_all(&workspace);
    result.map(|mut ok| {
        ok.workspace_path = None;
        ok
    })
}

fn run_compat_test_inner(
    config: &CompatTestConfig,
    source: &Path,
    workspace: &Path,
) -> Result<TestResult, AppError> {
    let package_dir = workspace.join("package");
    std::fs::create_dir_all(&package_dir)?;
    let package_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("loom-compat");

    let mut steps = Vec::new();
    steps.push(StepResult {
        name: "check_loom".to_string(),
        detail: run_loom(config.loom_bin.as_deref(), workspace, &["--version"])?,
    });

    run_loom(
        config.loom_bin.as_deref(),
        &package_dir,
        &["init", package_name],
    )?;
    copy_dir_contents(source, &package_dir)?;
    steps.push(StepResult {
        name: "prepare_package".to_string(),
        detail: format!("copied {}", source.display()),
    });

    steps.push(StepResult {
        name: "validate".to_string(),
        detail: run_loom(config.loom_bin.as_deref(), &package_dir, &["validate"])?,
    });

    let bundle = run_loom(
        config.loom_bin.as_deref(),
        &package_dir,
        &["build", "--emit", "knots-bundle"],
    )?;
    let bundle_path = workspace.join(if bundle.trim_start().starts_with('{') {
        "bundle.json"
    } else {
        "bundle.toml"
    });
    std::fs::write(&bundle_path, &bundle)?;
    steps.push(StepResult {
        name: "build".to_string(),
        detail: format!("wrote {}", bundle_path.display()),
    });

    let db_path = workspace.join(".knots/cache/state.sqlite");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = db::open_connection(&db_path.to_string_lossy())?;
    let workflow_id = installed_workflows::install_bundle(workspace, &bundle_path)?;
    let config_selection =
        installed_workflows::set_current_workflow_selection(workspace, &workflow_id, None, None)?;
    steps.push(StepResult {
        name: "install_bundle".to_string(),
        detail: format!(
            "selected {}",
            config_selection
                .current_profile
                .unwrap_or_else(|| workflow_id.clone())
        ),
    });

    let app = App::open(&db_path.to_string_lossy(), workspace.to_path_buf())?;
    let installed = InstalledWorkflowRegistry::load(workspace)?;
    let workflow = installed.current_workflow()?;
    let profile_id = installed
        .current_profile_id()
        .ok_or_else(|| {
            AppError::InvalidArgument("workflow selection did not pick a profile".into())
        })?
        .to_string();
    let registry = app.profile_registry();
    let initial_queue = registry.require(&profile_id)?.initial_state.clone();
    let action_state = workflow_runtime::next_happy_path_state(
        registry,
        &profile_id,
        KnotType::Work,
        &initial_queue,
    )?
    .ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "workflow '{}' has no initial action state from '{}'",
            workflow.id, initial_queue
        ))
    })?;
    let prompt = workflow
        .prompt_for_action_state(&action_state)
        .ok_or_else(|| {
            AppError::InvalidArgument(format!(
                "workflow '{}' has no prompt for action state '{}'",
                workflow.id, action_state
            ))
        })?
        .clone();
    let scenarios = run_scenarios(
        &app,
        workspace,
        &workflow.id,
        &profile_id,
        &action_state,
        &prompt,
        config.mode,
    )?;
    steps.push(StepResult {
        name: "exercise_runtime".to_string(),
        detail: format!("{} scenario(s)", scenarios.len()),
    });

    Ok(TestResult {
        success: true,
        mode: config.mode,
        source: source.to_path_buf(),
        workflow_id: workflow.id.clone(),
        workspace_path: Some(workspace.to_path_buf()),
        steps,
        scenarios,
    })
}

fn run_scenarios(
    app: &App,
    repo_root: &Path,
    workflow_id: &str,
    profile_id: &str,
    action_state: &str,
    prompt: &PromptDefinition,
    mode: CompatTestMode,
) -> Result<Vec<ScenarioResult>, AppError> {
    let mut outcomes = vec!["success".to_string()];
    if mode == CompatTestMode::Matrix {
        outcomes.extend(prompt.failure_targets.iter().map(|(name, _)| name.clone()));
    }
    let mut scenarios = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        scenarios.push(run_single_scenario(
            app,
            repo_root,
            workflow_id,
            profile_id,
            action_state,
            prompt,
            &outcome,
        )?);
    }
    Ok(scenarios)
}

fn run_single_scenario(
    app: &App,
    repo_root: &Path,
    workflow_id: &str,
    profile_id: &str,
    action_state: &str,
    prompt: &PromptDefinition,
    outcome: &str,
) -> Result<ScenarioResult, AppError> {
    let knot = app.create_knot("Loom compat harness knot", None, None, Some(profile_id))?;
    let prompt_view = poll_claim::peek_knot(app, &knot.id)?;
    let prompt_verified = prompt_view.skill.contains(prompt.body.trim())
        && prompt
            .accept
            .iter()
            .all(|item| prompt_view.skill.contains(item));
    let claimed = poll_claim::claim_knot(app, &knot.id, compat_actor())?;
    if claimed.knot.state != action_state {
        return Err(AppError::InvalidArgument(format!(
            "claim moved knot '{}' to '{}' instead of '{}'",
            claimed.knot.id, claimed.knot.state, action_state
        )));
    }
    let expected_state = workflow_runtime::next_outcome_state(
        app.profile_registry(),
        repo_root,
        workflow_id,
        profile_id,
        KnotType::Work,
        action_state,
        outcome,
    )?
    .ok_or_else(|| {
        AppError::InvalidArgument(format!(
            "workflow '{}' has no '{}' outcome from '{}'",
            workflow_id, outcome, action_state
        ))
    })?;
    let updated = app.set_state_with_actor(
        &claimed.knot.id,
        &expected_state,
        false,
        None,
        compat_actor(),
    )?;
    Ok(ScenarioResult {
        outcome: outcome.to_string(),
        expected_state,
        actual_state: updated.state,
        prompt_verified,
    })
}

fn compat_actor() -> StateActorMetadata {
    StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: Some("loom-compat".to_string()),
        agent_model: Some("compat-harness".to_string()),
        agent_version: Some("1".to_string()),
    }
}

fn run_loom(loom_bin: Option<&Path>, cwd: &Path, args: &[&str]) -> Result<String, AppError> {
    let binary = loom_bin
        .map(|path| path.as_os_str().to_owned())
        .or_else(|| std::env::var_os("KNOTS_LOOM_BIN"))
        .unwrap_or_else(|| "loom".into());
    let output = Command::new(binary).current_dir(cwd).args(args).output();
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::InvalidArgument(
                "loom is not discoverable on PATH".to_string(),
            ));
        }
        Err(err) => {
            return Err(AppError::InvalidArgument(format!(
                "failed to execute loom {}: {err}",
                args.join(" ")
            )));
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::InvalidArgument(format!(
            "loom {} failed{}",
            args.join(" "),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        )));
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_| {
        AppError::InvalidArgument(format!("loom {} produced invalid UTF-8", args.join(" ")))
    })?;
    Ok(match args {
        ["--version"] => stdout.trim().to_string(),
        _ if stdout.trim().is_empty() => "ok".to_string(),
        _ => stdout,
    })
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), AppError> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_contents(&path, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn unique_workspace() -> PathBuf {
    let path = std::env::temp_dir().join(format!("knots-loom-compat-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("compat workspace should be creatable");
    path
}

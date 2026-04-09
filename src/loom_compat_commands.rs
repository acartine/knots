use std::io::IsTerminal;
use std::path::Path;

use crate::app::AppError;
use crate::cli::{LoomArgs, LoomCompatModeArg, LoomSubcommands};
use crate::loom_compat_harness::{
    self, CompatTestConfig, CompatTestMode, ProgressUpdate, ProgressUpdateKind, TestResult,
};

pub(crate) fn run_loom_command(args: &LoomArgs, _repo_root: &Path) -> Result<(), AppError> {
    match &args.command {
        LoomSubcommands::CompatTest(inner) => {
            let config = CompatTestConfig {
                mode: match inner.mode {
                    LoomCompatModeArg::Smoke => CompatTestMode::Smoke,
                    LoomCompatModeArg::Matrix => CompatTestMode::Matrix,
                },
                keep_artifacts: inner.keep_artifacts,
                loom_bin: std::env::var_os("KNOTS_LOOM_BIN").map(std::path::PathBuf::from),
            };
            let result = if inner.json {
                loom_compat_harness::run_compat_test(&config)?
            } else {
                let color =
                    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
                loom_compat_harness::run_compat_test_with_progress(&config, |update| {
                    println!("{}", render_progress(&update, color));
                })?
            };
            if inner.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).expect("json serialization should work")
                );
            } else {
                print!("{}", render_text(&result));
            }
        }
    }
    Ok(())
}

fn render_progress(update: &ProgressUpdate, color: bool) -> String {
    let status = match update.kind {
        ProgressUpdateKind::Started => paint(color, "36", "…"),
        ProgressUpdateKind::Succeeded => paint(color, "32", "✓"),
        ProgressUpdateKind::Failed => paint(color, "31", "x"),
    };
    let step = update.step_name.replace('_', " ");
    if update.detail.trim().is_empty() {
        format!("{status} {step}")
    } else {
        format!("{status} {step}: {}", update.detail.trim())
    }
}

fn render_text(result: &TestResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "loom compat-test {} {}\n",
        result.workflow_id, result.mode
    ));
    out.push_str(&format!("source: {}\n", result.source.display()));
    if let Some(path) = result.workspace_path.as_deref() {
        out.push_str(&format!("workspace: {}\n", path.display()));
    }
    out.push_str("steps:\n");
    for step in &result.steps {
        out.push_str(&format!("- {}: {}\n", step.name, step.detail.trim()));
    }
    out.push_str("scenarios:\n");
    for scenario in &result.scenarios {
        out.push_str(&format!(
            "- {} -> {} ({}){}\n",
            scenario.outcome,
            scenario.actual_state,
            scenario.expected_state,
            if scenario.prompt_verified {
                ""
            } else {
                " prompt-mismatch"
            }
        ));
    }
    out
}

fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{render_progress, render_text};
    use crate::loom_compat_harness::{CompatTestMode, ScenarioResult, StepResult, TestResult};
    use crate::loom_compat_harness::{ProgressUpdate, ProgressUpdateKind};
    use std::path::PathBuf;

    #[test]
    fn render_text_includes_workspace_and_steps() {
        let rendered = render_text(&TestResult {
            success: true,
            mode: CompatTestMode::Matrix,
            source: PathBuf::from("<builtin:work_sdlc>"),
            workflow_id: "custom_flow".to_string(),
            workspace_path: Some(PathBuf::from("/tmp/workspace")),
            steps: vec![StepResult {
                name: "build".to_string(),
                detail: "done".to_string(),
            }],
            scenarios: vec![ScenarioResult {
                outcome: "success".to_string(),
                expected_state: "ready_for_review".to_string(),
                actual_state: "ready_for_review".to_string(),
                prompt_verified: true,
            }],
        });
        assert!(rendered.contains("workspace: /tmp/workspace"));
        assert!(rendered.contains("- build: done"));
        assert!(rendered.contains("success -> ready_for_review"));
    }

    #[test]
    fn render_text_marks_prompt_mismatches_without_workspace() {
        let rendered = render_text(&TestResult {
            success: true,
            mode: CompatTestMode::Smoke,
            source: PathBuf::from("<builtin:work_sdlc>"),
            workflow_id: "custom_flow".to_string(),
            workspace_path: None,
            steps: Vec::new(),
            scenarios: vec![ScenarioResult {
                outcome: "blocked".to_string(),
                expected_state: "deferred".to_string(),
                actual_state: "deferred".to_string(),
                prompt_verified: false,
            }],
        });
        assert!(!rendered.contains("workspace:"));
        assert!(rendered.contains("prompt-mismatch"));
    }

    #[test]
    fn render_progress_uses_status_markers_and_step_labels() {
        let started = render_progress(
            &ProgressUpdate {
                kind: ProgressUpdateKind::Started,
                step_name: "check_loom".to_string(),
                detail: String::new(),
            },
            false,
        );
        let failed = render_progress(
            &ProgressUpdate {
                kind: ProgressUpdateKind::Failed,
                step_name: "validate".to_string(),
                detail: "bad path".to_string(),
            },
            false,
        );
        assert_eq!(started, "… check loom");
        assert_eq!(failed, "x validate: bad path");
    }
}

use std::path::Path;

use crate::app::AppError;
use crate::cli::{LoomArgs, LoomCompatModeArg, LoomSubcommands};
use crate::loom_compat_harness::{self, CompatTestConfig, CompatTestMode, TestResult};

pub(crate) fn run_loom_command(args: &LoomArgs, repo_root: &Path) -> Result<(), AppError> {
    match &args.command {
        LoomSubcommands::CompatTest(inner) => {
            let source = if inner.source.is_absolute() {
                inner.source.clone()
            } else {
                repo_root.join(&inner.source)
            };
            let result = loom_compat_harness::run_compat_test(&CompatTestConfig {
                source,
                mode: match inner.mode {
                    LoomCompatModeArg::Smoke => CompatTestMode::Smoke,
                    LoomCompatModeArg::Matrix => CompatTestMode::Matrix,
                },
                keep_artifacts: inner.keep_artifacts,
                loom_bin: std::env::var_os("KNOTS_LOOM_BIN").map(std::path::PathBuf::from),
            })?;
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

#[cfg(test)]
mod tests {
    use super::render_text;
    use crate::loom_compat_harness::{CompatTestMode, ScenarioResult, StepResult, TestResult};
    use std::path::PathBuf;

    #[test]
    fn render_text_includes_workspace_and_steps() {
        let rendered = render_text(&TestResult {
            success: true,
            mode: CompatTestMode::Matrix,
            source: PathBuf::from("/tmp/pkg"),
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
            source: PathBuf::from("/tmp/pkg"),
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
}

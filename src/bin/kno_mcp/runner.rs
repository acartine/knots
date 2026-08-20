use std::path::{Path, PathBuf};
use std::process::Command;

use rmcp::model::{CallToolResult, Content};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct KnoRunner {
    kno_bin: PathBuf,
    repo: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnoFailure {
    pub exit_code: Option<i32>,
    pub stderr: String,
}

impl KnoRunner {
    pub fn new(kno_bin: PathBuf, repo: PathBuf) -> Self {
        Self { kno_bin, repo }
    }

    pub fn run(&self, subcommand: &str, args: &[String]) -> Result<Value, KnoFailure> {
        self.run_with_env(subcommand, args, false)
    }

    pub fn run_allowing_active_leases(
        &self,
        subcommand: &str,
        args: &[String],
    ) -> Result<Value, KnoFailure> {
        self.run_with_env(subcommand, args, true)
    }

    fn run_with_env(
        &self,
        subcommand: &str,
        args: &[String],
        allow_active_leases: bool,
    ) -> Result<Value, KnoFailure> {
        let mut command = Command::new(&self.kno_bin);
        command.args(build_argv(&self.repo, subcommand, args));
        if allow_active_leases {
            command.env("KNOTS_ALLOW_ACTIVE_LEASE_REPLICATION", "1");
        }
        let output = command.output().map_err(|err| KnoFailure {
            exit_code: None,
            stderr: err.to_string(),
        })?;
        if !output.status.success() {
            return Err(KnoFailure {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|err| KnoFailure {
            exit_code: Some(0),
            stderr: format!("failed to parse kno JSON output: {err}"),
        })
    }

    pub fn run_tool(&self, subcommand: &str, args: &[String]) -> CallToolResult {
        match self.run(subcommand, args) {
            Ok(value) => CallToolResult::structured(value),
            Err(err) => failure_result(&err),
        }
    }
}

pub fn build_argv(repo: &Path, subcommand: &str, args: &[String]) -> Vec<String> {
    let mut argv = vec![
        "-C".to_string(),
        repo.display().to_string(),
        subcommand.to_string(),
    ];
    argv.extend(args.iter().cloned());
    argv.push("--json".to_string());
    argv
}

pub fn resolve_default_kno_bin() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(resolve_sibling_kno_bin))
        .unwrap_or_else(|| PathBuf::from("kno"))
}

fn resolve_sibling_kno_bin(parent: &Path) -> PathBuf {
    let kno = parent.join("kno");
    if kno.exists() {
        return kno;
    }
    let knots = parent.join("knots");
    if knots.exists() {
        return knots;
    }
    PathBuf::from("kno")
}

pub fn failure_result(err: &KnoFailure) -> CallToolResult {
    let message = match err.exit_code {
        Some(code) => format!("kno exited with code {code}: {}", err.stderr),
        None => format!("failed to run kno: {}", err.stderr),
    };
    let mut result = CallToolResult::structured_error(json!({
            "exit_code": err.exit_code,
            "stderr": err.stderr,
    }));
    result.content = vec![Content::text(message)];
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn build_argv_prefixes_repo_and_appends_json() {
        let args = vec!["--state".to_string(), "ready".to_string()];
        assert_eq!(
            build_argv(Path::new("/r"), "ls", &args),
            ["-C", "/r", "ls", "--state", "ready", "--json"]
        );
    }

    #[test]
    fn failure_result_marks_tool_error() {
        let result = failure_result(&KnoFailure {
            exit_code: Some(1),
            stderr: "error: not found".to_string(),
        });
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content.unwrap()["exit_code"], 1);
        assert!(result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("not found"));
    }

    #[test]
    fn failure_result_covers_spawn_error() {
        let result = failure_result(&KnoFailure {
            exit_code: None,
            stderr: "missing binary".to_string(),
        });
        assert_eq!(result.is_error, Some(true));
        assert!(result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("failed to run kno"));
    }

    #[test]
    fn runner_executes_successful_json_command() {
        let runner = KnoRunner::new(fixture_kno(), PathBuf::from("/example/repo"));
        let value = runner.run("show", &["k1".to_string()]).expect("show json");
        assert_eq!(value["id"], "k1");

        let result = runner.run_tool("ls", &[]);
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.structured_content.unwrap()["total"], 1);
    }

    #[test]
    fn runner_can_allow_active_lease_replication() {
        let runner = KnoRunner::new(fixture_kno(), PathBuf::from("/example/repo"));
        let value = runner
            .run_allowing_active_leases("push", &[])
            .expect("push json");
        assert_eq!(value["allow_active_leases"], true);
    }

    #[test]
    fn runner_maps_command_and_json_parse_failures() {
        let runner = KnoRunner::new(fixture_kno(), PathBuf::from("/example/repo"));
        let err = runner
            .run("show", &["MISSING".to_string()])
            .expect_err("missing knot should fail");
        assert_eq!(err.exit_code, Some(1));
        assert!(err.stderr.contains("not found"));

        let (_script_ws, script) = temp_script("bad-json", "printf 'not-json\\n'\n");
        let runner = KnoRunner::new(script.clone(), PathBuf::from("/example/repo"));
        let err = runner
            .run("show", &[])
            .expect_err("invalid json should fail");
        assert_eq!(err.exit_code, Some(0));
        assert!(err.stderr.contains("failed to parse kno JSON output"));
        let _ = fs::remove_file(script);
    }

    #[test]
    fn runner_maps_spawn_failures() {
        let runner = KnoRunner::new(
            PathBuf::from("/example/kno-mcp-missing-binary"),
            PathBuf::from("/example/repo"),
        );
        let err = runner
            .run("show", &[])
            .expect_err("missing binary should fail");
        assert_eq!(err.exit_code, None);
        assert!(!err.stderr.is_empty());
    }

    #[test]
    fn default_binary_is_next_to_current_exe_or_kno() {
        let path = resolve_default_kno_bin();
        assert!(matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("kno" | "knots")
        ));
    }

    #[test]
    fn sibling_resolver_prefers_kno_then_knots_then_path() {
        let dir_ws = knots_test_support::workspace("knots-kno-mcp-bin");
        let dir = dir_ws.path().to_path_buf();
        assert_eq!(resolve_sibling_kno_bin(&dir), PathBuf::from("kno"));

        fs::write(dir.join("knots"), "").expect("write knots");
        assert_eq!(resolve_sibling_kno_bin(&dir), dir.join("knots"));

        fs::write(dir.join("kno"), "").expect("write kno");
        assert_eq!(resolve_sibling_kno_bin(&dir), dir.join("kno"));
    }

    fn fixture_kno() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kno-stub.sh")
    }

    fn temp_script(name: &str, body: &str) -> (knots_test_support::TestWorkspace, PathBuf) {
        let ws = knots_test_support::workspace(&format!("knots-kno-mcp-{name}"));
        let path = ws.path().join("script.sh");
        fs::write(
            &path,
            format!("#!/usr/bin/env bash\nset -euo pipefail\n{body}"),
        )
        .expect("write script");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).expect("chmod");
        }
        (ws, path)
    }
}

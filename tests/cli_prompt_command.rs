mod cli_dispatch_helpers;

use cli_dispatch_helpers::*;

const DEPRECATION_WARNING: &str = "warning: 'kno skill' is deprecated; use 'kno prompt'\n";

#[test]
fn prompt_command_prints_action_state_prompt() {
    let root_ws = unique_workspace("knots-cli-prompt");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let prompt = run_knots(&root, &db, &["prompt", "planning"]);
    assert_success(&prompt);
    let stdout = String::from_utf8_lossy(&prompt.stdout);
    assert!(stdout.contains("# Planning"), "planning: {stdout}");

    assert!(
        prompt.stderr.is_empty(),
        "`kno prompt` should not emit any stderr output, got: {}",
        String::from_utf8_lossy(&prompt.stderr)
    );
}

#[test]
fn prompt_output_matches_deprecated_skill_alias_byte_for_byte() {
    let root_ws = unique_workspace("knots-cli-prompt-parity");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let prompt = run_knots(&root, &db, &["prompt", "implementation"]);
    assert_success(&prompt);

    let skill = run_knots(&root, &db, &["skill", "implementation"]);
    assert_success(&skill);

    assert_eq!(
        prompt.stdout, skill.stdout,
        "deprecated `kno skill` stdout must match `kno prompt` byte-for-byte"
    );
}

#[test]
fn deprecated_skill_alias_emits_exact_deprecation_warning() {
    let root_ws = unique_workspace("knots-cli-skill-deprecated");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let skill = run_knots(&root, &db, &["skill", "planning"]);
    assert_success(&skill);
    let stderr = String::from_utf8_lossy(&skill.stderr);
    assert_eq!(
        stderr, DEPRECATION_WARNING,
        "`kno skill` must emit exactly the deprecation warning to stderr"
    );
}

#[test]
fn prompt_subcommand_appears_in_top_level_help() {
    let root_ws = unique_workspace("knots-cli-help-prompt");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    // `kno --help` uses the custom help printer (not clap's). Invoke it via the
    // binary so we exercise the exact path users hit on the CLI.
    let help = run_knots(&root, &db, &["--help"]);
    assert_success(&help);
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(
        stdout.contains("prompt"),
        "`kno --help` should list `prompt`: {stdout}"
    );
    assert!(
        !stdout.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("skill ") || trimmed.starts_with("skill\t")
        }),
        "`kno --help` must not show the deprecated `skill` subcommand: {stdout}"
    );
}

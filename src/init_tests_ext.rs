use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use crate::app::AppError;
use crate::workflow::{WorkflowDefinition, WorkflowTransition};

use super::{
    choose_default_workflow, choose_default_workflow_with_io, ensure_knots_gitignore,
    uninit_local_store, warn_if_beads_hooks_present, KNOTS_IGNORE_RULE,
};

fn workflow(id: &str, description: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: id.to_string(),
        aliases: Vec::new(),
        description: Some(description.to_string()),
        initial_state: "idea".to_string(),
        states: vec![
            "idea".to_string(),
            "work_item".to_string(),
            "done".to_string(),
        ],
        terminal_states: vec!["done".to_string()],
        transitions: vec![
            WorkflowTransition {
                from: "idea".to_string(),
                to: "work_item".to_string(),
            },
            WorkflowTransition {
                from: "work_item".to_string(),
                to: "done".to_string(),
            },
        ],
    }
}

fn sample_workflows() -> Vec<WorkflowDefinition> {
    vec![
        workflow("automation_granular", "Granular automation"),
        workflow("human_pr_gate", "Human PR gate"),
    ]
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
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

#[test]
fn choose_default_workflow_rejects_empty_workflow_list() {
    let err = choose_default_workflow(&[], None).expect_err("empty workflow list should fail");
    match err {
        AppError::InvalidArgument(message) => {
            assert!(message.contains("no workflows are available"));
        }
        other => panic!("unexpected error variant: {other}"),
    }
}

#[test]
fn choose_default_workflow_with_io_non_interactive_uses_fallback_index() {
    let workflows = sample_workflows();
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::<u8>::new();

    let selected = choose_default_workflow_with_io(
        &workflows,
        Some("human_pr_gate"),
        false,
        &mut input,
        &mut output,
    )
    .expect("non-interactive selection should succeed");
    assert_eq!(selected, "human_pr_gate");

    let mut input = Cursor::new(Vec::<u8>::new());
    let selected_unknown = choose_default_workflow_with_io(
        &workflows,
        Some("missing"),
        false,
        &mut input,
        &mut output,
    )
    .expect("unknown default should fall back to first option");
    assert_eq!(selected_unknown, "automation_granular");
}

#[test]
fn choose_default_workflow_with_io_interactive_reprompts_until_valid_selection() {
    let workflows = sample_workflows();
    let mut input = Cursor::new("9\n2\n".as_bytes().to_vec());
    let mut output = Vec::<u8>::new();

    let selected = choose_default_workflow_with_io(&workflows, None, true, &mut input, &mut output)
        .expect("interactive selection should succeed");

    assert_eq!(selected, "human_pr_gate");
    let printed = String::from_utf8(output).expect("prompt output should be UTF-8");
    assert!(printed.contains("Select default workflow for this repo:"));
    assert!(printed.contains("enter a number between 1 and 2"));
}

#[test]
fn choose_default_workflow_with_io_interactive_accepts_enter_for_current_default() {
    let workflows = sample_workflows();
    let mut input = Cursor::new("\n".as_bytes().to_vec());
    let mut output = Vec::<u8>::new();

    let selected = choose_default_workflow_with_io(
        &workflows,
        Some("human_pr_gate"),
        true,
        &mut input,
        &mut output,
    )
    .expect("interactive default selection should succeed");

    assert_eq!(selected, "human_pr_gate");
    let printed = String::from_utf8(output).expect("prompt output should be UTF-8");
    assert!(printed.contains("current default: human_pr_gate"));
}

#[test]
fn warn_if_beads_hooks_present_handles_config_without_matching_hook_files() {
    let root = std::env::temp_dir().join(format!("knots-init-hooks-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    run_git(&root, &["init"]);
    run_git(&root, &["config", "user.email", "knots@example.com"]);
    run_git(&root, &["config", "user.name", "Knots Test"]);
    run_git(&root, &["config", "beads.role", "maintainer"]);
    let hooks_dir = root.join(".git/hooks");
    std::fs::create_dir_all(&hooks_dir).expect("hooks dir should be creatable");
    std::fs::write(hooks_dir.join("pre-push"), "#!/bin/sh\necho plain\n")
        .expect("non-beads pre-push should be writable");

    warn_if_beads_hooks_present(&root).expect("beads warning path should be non-fatal");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gitignore_helpers_cover_append_and_noop_removal_paths() {
    let root = std::env::temp_dir().join(format!("knots-init-gitignore-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    let db_path = root.join(".knots/cache/state.sqlite");
    let gitignore = root.join(".gitignore");
    std::fs::write(&gitignore, "target").expect("gitignore fixture should write");

    ensure_knots_gitignore(&root).expect("ensure gitignore should succeed");
    let contents = std::fs::read_to_string(&gitignore).expect("gitignore should read");
    assert!(contents.contains("target\n"));
    assert!(contents.lines().any(|line| line == KNOTS_IGNORE_RULE));

    std::fs::write(&gitignore, "target\n").expect("gitignore reset should write");
    uninit_local_store(&root, db_path.to_str().expect("utf8 db path"))
        .expect("uninit should no-op when knots rule is absent");
    let unchanged = std::fs::read_to_string(&gitignore).expect("gitignore should read");
    assert_eq!(unchanged, "target\n");

    let _ = std::fs::remove_dir_all(root);
}

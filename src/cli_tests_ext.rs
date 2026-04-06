use clap::Parser;

use super::{Cli, Commands};

fn parse(args: &[&str]) -> Cli {
    Cli::parse_from(args)
}

#[test]
fn rollback_parses() {
    let cli = parse(&["kno", "rollback", "abc123"]);
    match cli.command {
        Commands::Rollback(args) => {
            assert_eq!(args.id, "abc123");
            assert!(!args.dry_run);
            assert!(args.actor_kind.is_none());
        }
        other => panic!("expected Rollback, got {:?}", other),
    }
}

#[test]
fn rollback_alias_parses() {
    let cli = parse(&["kno", "rb", "abc123", "--dry-run"]);
    match cli.command {
        Commands::Rollback(args) => {
            assert_eq!(args.id, "abc123");
            assert!(args.dry_run);
        }
        other => panic!("expected Rollback alias, got {:?}", other),
    }
}

#[test]
fn rollback_parses_actor_metadata_flags() {
    let cli = parse(&[
        "kno",
        "rollback",
        "abc123",
        "--actor-kind",
        "agent",
        "--agent-name",
        "codex",
        "--agent-model",
        "gpt-5",
        "--agent-version",
        "1.0",
    ]);
    match cli.command {
        Commands::Rollback(args) => {
            assert_eq!(args.actor_kind.as_deref(), Some("agent"));
            assert_eq!(args.agent_name.as_deref(), Some("codex"));
            assert_eq!(args.agent_model.as_deref(), Some("gpt-5"));
            assert_eq!(args.agent_version.as_deref(), Some("1.0"));
        }
        other => panic!("expected Rollback, got {:?}", other),
    }
}

#[test]
fn completions_parses_with_shell() {
    let cli = parse(&["kno", "completions", "bash"]);
    match cli.command {
        Commands::Completions(args) => {
            assert_eq!(args.shell.as_deref(), Some("bash"));
            assert!(!args.install);
        }
        other => panic!("expected Completions, got {:?}", other),
    }
}

#[test]
fn completions_install_flag_parses() {
    let cli = parse(&["kno", "completions", "--install"]);
    match cli.command {
        Commands::Completions(args) => {
            assert!(args.shell.is_none());
            assert!(args.install);
        }
        other => panic!("expected Completions, got {:?}", other),
    }
}

#[test]
fn skill_parses() {
    let cli = parse(&["kno", "skill", "abc123"]);
    match cli.command {
        Commands::Skill(args) => assert_eq!(args.id, "abc123"),
        other => panic!("expected Skill, got {:?}", other),
    }
}

#[test]
fn ready_parses_without_type() {
    let cli = parse(&["kno", "ready"]);
    match cli.command {
        Commands::Ready(args) => {
            assert!(args.ready_type.is_none());
            assert!(!args.json);
        }
        other => panic!("expected Ready, got {:?}", other),
    }
}

#[test]
fn ready_parses_with_type() {
    let cli = parse(&["kno", "ready", "plan"]);
    match cli.command {
        Commands::Ready(args) => {
            assert_eq!(args.ready_type.as_deref(), Some("plan"));
        }
        other => panic!("expected Ready, got {:?}", other),
    }
}

#[test]
fn ready_parses_with_json_flag() {
    let cli = parse(&["kno", "ready", "--json"]);
    match cli.command {
        Commands::Ready(args) => {
            assert!(args.ready_type.is_none());
            assert!(args.json);
        }
        other => panic!("expected Ready, got {:?}", other),
    }
}

#[test]
fn ready_parses_with_type_and_json() {
    let cli = parse(&["kno", "ready", "implementation", "--json"]);
    match cli.command {
        Commands::Ready(args) => {
            assert_eq!(args.ready_type.as_deref(), Some("implementation"));
            assert!(args.json);
        }
        other => panic!("expected Ready, got {:?}", other),
    }
}

#[test]
fn claim_peek_flag_parses() {
    let cli = parse(&["kno", "claim", "abc123", "--peek"]);
    match cli.command {
        Commands::Claim(args) => {
            assert_eq!(args.id, "abc123");
            assert!(args.peek);
        }
        other => panic!("expected Claim, got {:?}", other),
    }
}

#[test]
fn claim_without_peek_defaults_false() {
    let cli = parse(&["kno", "claim", "abc123"]);
    match cli.command {
        Commands::Claim(args) => {
            assert!(!args.peek);
        }
        other => panic!("expected Claim, got {:?}", other),
    }
}

#[test]
fn update_add_invariant_repeatable() {
    let cli = parse(&[
        "kno",
        "update",
        "abc123",
        "--add-invariant",
        "Scope:first condition",
        "--add-invariant",
        "State:second condition",
    ]);
    match cli.command {
        Commands::Update(args) => {
            assert_eq!(args.add_invariants.len(), 2);
            assert_eq!(args.add_invariants[0], "Scope:first condition");
            assert_eq!(args.add_invariants[1], "State:second condition");
            assert!(args.remove_invariants.is_empty());
            assert!(!args.clear_invariants);
        }
        other => panic!("expected Update, got {:?}", other),
    }
}

#[test]
fn update_remove_invariant_repeatable() {
    let cli = parse(&[
        "kno",
        "update",
        "abc123",
        "--remove-invariant",
        "Scope:old scope rule",
        "--remove-invariant",
        "State:old state rule",
    ]);
    match cli.command {
        Commands::Update(args) => {
            assert!(args.add_invariants.is_empty());
            assert_eq!(args.remove_invariants.len(), 2);
            assert_eq!(args.remove_invariants[0], "Scope:old scope rule");
            assert_eq!(args.remove_invariants[1], "State:old state rule");
            assert!(!args.clear_invariants);
        }
        other => panic!("expected Update, got {:?}", other),
    }
}

#[test]
fn update_clear_invariants_alone() {
    let cli = parse(&["kno", "update", "abc123", "--clear-invariants"]);
    match cli.command {
        Commands::Update(args) => {
            assert!(args.add_invariants.is_empty());
            assert!(args.remove_invariants.is_empty());
            assert!(args.clear_invariants);
        }
        other => panic!("expected Update, got {:?}", other),
    }
}

#[test]
fn update_defaults_no_invariant_flags() {
    let cli = parse(&["kno", "update", "abc123", "-t", "new title"]);
    match cli.command {
        Commands::Update(args) => {
            assert!(args.add_invariants.is_empty());
            assert!(args.remove_invariants.is_empty());
            assert!(!args.clear_invariants);
        }
        other => panic!("expected Update, got {:?}", other),
    }
}

#[test]
fn update_help_includes_invariant_flags() {
    use clap::CommandFactory;

    let mut root = Cli::command();
    let update = root
        .find_subcommand_mut("update")
        .expect("update subcommand should exist");
    let mut buf = Vec::new();
    update
        .write_long_help(&mut buf)
        .expect("update help should render");
    let help = String::from_utf8(buf).expect("utf-8");
    assert!(
        help.contains("--add-invariant"),
        "update help should mention --add-invariant: {help}"
    );
    assert!(
        help.contains("--remove-invariant"),
        "update help should mention --remove-invariant: {help}"
    );
    assert!(
        help.contains("--clear-invariants"),
        "update help should mention --clear-invariants: {help}"
    );
}

#[test]
fn trace_flag_parses_as_global_option() {
    let cli = parse(&["kno", "--trace", "ls"]);
    assert!(cli.trace);
    assert!(matches!(cli.command, Commands::Ls(_)));
}

#[test]
fn ls_limit_and_offset_parse() {
    let cli = parse(&["kno", "ls", "--limit", "25", "--offset", "10"]);
    match cli.command {
        Commands::Ls(args) => {
            assert_eq!(args.limit, Some(25));
            assert_eq!(args.offset, Some(10));
        }
        other => panic!("expected Ls, got {:?}", other),
    }
}

#[test]
fn ls_limit_short_flag_parses() {
    let cli = parse(&["kno", "ls", "-l", "10"]);
    match cli.command {
        Commands::Ls(args) => {
            assert_eq!(args.limit, Some(10));
            assert_eq!(args.offset, None);
        }
        other => panic!("expected Ls, got {:?}", other),
    }
}

#[test]
fn ls_offset_short_flag_parses() {
    let cli = parse(&["kno", "ls", "-o", "5"]);
    match cli.command {
        Commands::Ls(args) => {
            assert_eq!(args.limit, None);
            assert_eq!(args.offset, Some(5));
        }
        other => panic!("expected Ls, got {:?}", other),
    }
}

#[test]
fn ls_without_pagination_defaults_none() {
    let cli = parse(&["kno", "ls"]);
    match cli.command {
        Commands::Ls(args) => {
            assert_eq!(args.limit, None);
            assert_eq!(args.offset, None);
        }
        other => panic!("expected Ls, got {:?}", other),
    }
}

#[test]
fn ls_limit_combines_with_filters() {
    let cli = parse(&[
        "kno", "ls", "-s", "planning", "-l", "5", "-o", "2", "--json",
    ]);
    match cli.command {
        Commands::Ls(args) => {
            assert_eq!(args.state.as_deref(), Some("planning"));
            assert_eq!(args.limit, Some(5));
            assert_eq!(args.offset, Some(2));
            assert!(args.json);
        }
        other => panic!("expected Ls, got {:?}", other),
    }
}

use clap::Parser;

use super::{Cli, Commands, LeaseSubcommands};

fn parse(args: &[&str]) -> Cli {
    Cli::parse_from(args)
}

#[test]
fn lease_create_parses_with_nickname() {
    let cli = parse(&["kno", "lease", "create", "--nickname", "my-session"]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::Create(create) => {
                assert_eq!(create.nickname, "my-session");
                assert_eq!(create.lease_type, "agent");
                assert!(create.agent_type.is_none());
                assert!(create.provider.is_none());
                assert!(create.agent_name.is_none());
                assert!(create.model.is_none());
                assert!(create.model_version.is_none());
            }
            other => {
                panic!("expected Create, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_create_parses_all_agent_fields() {
    let cli = parse(&[
        "kno",
        "lease",
        "create",
        "--nickname",
        "agent-session",
        "--type",
        "agent",
        "--agent-type",
        "cli",
        "--provider",
        "Anthropic",
        "--agent-name",
        "claude",
        "--model",
        "opus",
        "--model-version",
        "4.6",
    ]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::Create(create) => {
                assert_eq!(create.nickname, "agent-session");
                assert_eq!(create.lease_type, "agent");
                assert_eq!(create.agent_type.as_deref(), Some("cli"));
                assert_eq!(create.provider.as_deref(), Some("Anthropic"));
                assert_eq!(create.agent_name.as_deref(), Some("claude"));
                assert_eq!(create.model.as_deref(), Some("opus"));
                assert_eq!(create.model_version.as_deref(), Some("4.6"));
            }
            other => {
                panic!("expected Create, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_create_manual_type() {
    let cli = parse(&[
        "kno",
        "lease",
        "create",
        "--nickname",
        "human-session",
        "--type",
        "manual",
    ]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::Create(create) => {
                assert_eq!(create.lease_type, "manual");
            }
            other => {
                panic!("expected Create, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_show_parses_id() {
    let cli = parse(&["kno", "lease", "show", "knot-abc123"]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::Show(show) => {
                assert_eq!(show.id, "knot-abc123");
                assert!(!show.json);
            }
            other => {
                panic!("expected Show, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_show_parses_json_flag() {
    let cli = parse(&["kno", "lease", "show", "knot-abc123", "-j"]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::Show(show) => {
                assert_eq!(show.id, "knot-abc123");
                assert!(show.json);
            }
            other => {
                panic!("expected Show, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_terminate_parses_id() {
    let cli = parse(&["kno", "lease", "terminate", "knot-abc123"]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::Terminate(term) => {
                assert_eq!(term.id, "knot-abc123");
            }
            other => {
                panic!("expected Terminate, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_list_defaults_to_active_only() {
    let cli = parse(&["kno", "lease", "list"]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::List(list) => {
                assert!(!list.all);
                assert!(!list.json);
            }
            other => {
                panic!("expected List, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_list_parses_all_and_json_flags() {
    let cli = parse(&["kno", "lease", "list", "-a", "-j"]);
    match cli.command {
        Commands::Lease(args) => match args.command {
            LeaseSubcommands::List(list) => {
                assert!(list.all);
                assert!(list.json);
            }
            other => {
                panic!("expected List, got {:?}", other)
            }
        },
        other => panic!("expected Lease, got {:?}", other),
    }
}

#[test]
fn lease_list_alias_ls() {
    let cli = parse(&["kno", "lease", "ls"]);
    match cli.command {
        Commands::Lease(args) => {
            assert!(matches!(args.command, LeaseSubcommands::List(_)));
        }
        other => panic!("expected Lease, got {:?}", other),
    }
}

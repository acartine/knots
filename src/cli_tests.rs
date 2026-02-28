use clap::Parser;

use super::{Cli, Commands, ProfileSubcommands};

fn parse(args: &[&str]) -> Cli {
    Cli::parse_from(args)
}

#[test]
fn profile_set_default_quick_parses() {
    let cli = parse(&[
        "kno",
        "profile",
        "set-default-quick",
        "autopilot_no_planning",
    ]);
    match cli.command {
        Commands::Profile(args) => match args.command {
            ProfileSubcommands::SetDefaultQuick(sda) => {
                assert_eq!(sda.id, "autopilot_no_planning");
            }
            other => panic!("expected SetDefaultQuick, got {:?}", other),
        },
        other => panic!("expected Profile, got {:?}", other),
    }
}

#[test]
fn profile_list_parses() {
    let cli = parse(&["kno", "profile", "list"]);
    match cli.command {
        Commands::Profile(args) => {
            assert!(matches!(args.command, ProfileSubcommands::List(_)));
        }
        other => panic!("expected Profile, got {:?}", other),
    }
}

#[test]
fn profile_show_parses_with_id() {
    let cli = parse(&["kno", "profile", "show", "autopilot"]);
    match cli.command {
        Commands::Profile(args) => match args.command {
            ProfileSubcommands::Show(show_args) => {
                assert_eq!(show_args.id, "autopilot");
            }
            other => panic!("expected Show, got {:?}", other),
        },
        other => panic!("expected Profile, got {:?}", other),
    }
}

#[test]
fn doctor_json_flag_parses() {
    let cli = parse(&["kno", "doctor", "--json"]);
    match cli.command {
        Commands::Doctor(args) => assert!(args.json),
        other => panic!("expected Doctor, got {:?}", other),
    }
}

#[test]
fn new_desc_flag_parses() {
    let cli = parse(&["kno", "new", "My title", "--desc", "A description"]);
    match cli.command {
        Commands::New(args) => {
            assert_eq!(args.title, "My title");
            assert_eq!(args.desc.as_deref(), Some("A description"));
        }
        other => panic!("expected New, got {:?}", other),
    }
}

#[test]
fn new_short_d_flag_parses() {
    let cli = parse(&["kno", "new", "Title", "-d", "Short desc"]);
    match cli.command {
        Commands::New(args) => {
            assert_eq!(args.desc.as_deref(), Some("Short desc"));
        }
        other => panic!("expected New, got {:?}", other),
    }
}

#[test]
fn new_fast_flag_parses() {
    let cli = parse(&["kno", "new", "Quick task", "-f"]);
    match cli.command {
        Commands::New(args) => {
            assert_eq!(args.title, "Quick task");
            assert!(args.fast);
        }
        other => panic!("expected New, got {:?}", other),
    }
}

#[test]
fn q_command_parses() {
    let cli = parse(&["kno", "q", "Fast task"]);
    match cli.command {
        Commands::Q(args) => {
            assert_eq!(args.title, "Fast task");
        }
        other => panic!("expected Q, got {:?}", other),
    }
}

#[test]
fn next_parses() {
    let cli = parse(&["kno", "next", "abc123"]);
    match cli.command {
        Commands::Next(args) => {
            assert_eq!(args.id, "abc123");
            assert!(args.actor_kind.is_none());
            assert!(args.agent_name.is_none());
            assert!(args.agent_model.is_none());
            assert!(args.agent_version.is_none());
        }
        other => panic!("expected Next, got {:?}", other),
    }
}

#[test]
fn next_parses_actor_metadata_flags() {
    let cli = parse(&[
        "kno",
        "next",
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
        Commands::Next(args) => {
            assert_eq!(args.id, "abc123");
            assert_eq!(args.actor_kind.as_deref(), Some("agent"));
            assert_eq!(args.agent_name.as_deref(), Some("codex"));
            assert_eq!(args.agent_model.as_deref(), Some("gpt-5"));
            assert_eq!(args.agent_version.as_deref(), Some("1.0"));
        }
        other => panic!("expected Next, got {:?}", other),
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

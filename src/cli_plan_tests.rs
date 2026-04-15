use clap::CommandFactory;
use clap::Parser;

use super::*;

#[test]
fn parse_plan_wave_add() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "wave",
        "add",
        "knots-123",
        "--name",
        "Wave 1",
        "--objective",
        "Ship it",
        "--at",
        "2",
    ]);
    match cli.command {
        crate::cli::Commands::Plan(PlanArgs {
            command:
                PlanSubcommands::Wave(PlanWaveArgs {
                    command: PlanWaveSubcommands::Add(args),
                }),
        }) => {
            assert_eq!(args.id, "knots-123");
            assert_eq!(args.name, "Wave 1");
            assert_eq!(args.objective, "Ship it");
            assert_eq!(args.at, Some(2));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_plan_wave_remove() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "wave",
        "remove",
        "knots-123",
        "--wave",
        "3",
        "--force",
    ]);
    match cli.command {
        crate::cli::Commands::Plan(PlanArgs {
            command:
                PlanSubcommands::Wave(PlanWaveArgs {
                    command: PlanWaveSubcommands::Remove(args),
                }),
        }) => {
            assert_eq!(args.id, "knots-123");
            assert_eq!(args.wave, 3);
            assert!(args.force);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_plan_wave_move() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "wave",
        "move",
        "knots-123",
        "--from",
        "2",
        "--to",
        "1",
    ]);
    match cli.command {
        crate::cli::Commands::Plan(PlanArgs {
            command:
                PlanSubcommands::Wave(PlanWaveArgs {
                    command: PlanWaveSubcommands::Move(args),
                }),
        }) => {
            assert_eq!(args.from_index, 2);
            assert_eq!(args.to_index, 1);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_plan_step_add() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "step",
        "add",
        "knots-123",
        "--wave",
        "1",
        "--knot-ids",
        "knots-a,knots-b",
        "--notes",
        "land the change",
        "--at",
        "2",
    ]);
    match cli.command {
        crate::cli::Commands::Plan(PlanArgs {
            command:
                PlanSubcommands::Step(PlanStepArgs {
                    command: PlanStepSubcommands::Add(args),
                }),
        }) => {
            assert_eq!(args.wave, 1);
            assert_eq!(args.knot_ids, vec!["knots-a", "knots-b"]);
            assert_eq!(args.notes.as_deref(), Some("land the change"));
            assert_eq!(args.at, Some(2));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_plan_step_remove() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "step",
        "remove",
        "knots-123",
        "--wave",
        "1",
        "--step",
        "4",
        "--force",
    ]);
    match cli.command {
        crate::cli::Commands::Plan(PlanArgs {
            command:
                PlanSubcommands::Step(PlanStepArgs {
                    command: PlanStepSubcommands::Remove(args),
                }),
        }) => {
            assert_eq!(args.wave, 1);
            assert_eq!(args.step, 4);
            assert!(args.force);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_plan_step_move() {
    let cli = crate::cli::Cli::parse_from([
        "kno",
        "plan",
        "step",
        "move",
        "knots-123",
        "--wave",
        "1",
        "--from",
        "1",
        "--to",
        "2",
    ]);
    match cli.command {
        crate::cli::Commands::Plan(PlanArgs {
            command:
                PlanSubcommands::Step(PlanStepArgs {
                    command: PlanStepSubcommands::Move(args),
                }),
        }) => {
            assert_eq!(args.wave, 1);
            assert_eq!(args.from_index, 1);
            assert_eq!(args.to_index, 2);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn plan_help_explains_taxonomy() {
    let mut root = crate::cli::Cli::command();
    let plan = root
        .find_subcommand_mut("plan")
        .expect("plan subcommand should exist");
    let mut buf = Vec::new();
    plan.write_long_help(&mut buf)
        .expect("plan help should render");
    let help = String::from_utf8(buf).expect("utf-8");
    assert!(
        help.contains("waves: top-level phases that run in sequence"),
        "plan help should explain waves: {help}"
    );
    assert!(
        help.contains("kno plan step add"),
        "plan help should include a CLI walkthrough: {help}"
    );
}

#[test]
fn plan_step_add_help_mentions_concurrency() {
    let mut root = crate::cli::Cli::command();
    let plan = root
        .find_subcommand_mut("plan")
        .expect("plan subcommand should exist");
    let step = plan
        .find_subcommand_mut("step")
        .expect("step subcommand should exist");
    let add = step
        .find_subcommand_mut("add")
        .expect("step add should exist");
    let mut buf = Vec::new();
    add.write_long_help(&mut buf)
        .expect("step add help should render");
    let help = String::from_utf8(buf).expect("utf-8");
    assert!(
        help.contains("Knots listed in one step are grouped as concurrent work."),
        "step add help should explain concurrency: {help}"
    );
    assert!(
        help.contains("--knot-ids knots-a,knots-b"),
        "step add help should include an example: {help}"
    );
}

#[test]
fn plan_wave_help_mentions_sequence() {
    let mut root = crate::cli::Cli::command();
    let plan = root
        .find_subcommand_mut("plan")
        .expect("plan subcommand should exist");
    let wave = plan
        .find_subcommand_mut("wave")
        .expect("wave subcommand should exist");
    let mut buf = Vec::new();
    wave.write_long_help(&mut buf)
        .expect("wave help should render");
    let help = String::from_utf8(buf).expect("utf-8");
    assert!(
        help.contains("Wave 1 happens before wave 2"),
        "wave help should explain ordering: {help}"
    );
    assert!(
        help.contains("kno plan wave add"),
        "wave help should include an example: {help}"
    );
}

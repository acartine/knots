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

use clap::{Args, Subcommand};

const PLAN_LONG_ABOUT: &str = "\
Manage the ordered structure of an `execution_plan` knot.
\n\
\n\
Execution plans are organized as:
\n\
- waves: top-level phases that run in sequence
\n\
- steps: ordered checkpoints within each wave
\n\
- knot_ids: knots attached to a step; knots in the same step are meant to run concurrently
\n\
\n\
Use `kno plan wave ...` to shape the high-level phases, then use
`kno plan step ...` to define the ordered work inside each wave.";

const PLAN_AFTER_HELP: &str = "\
Examples:
\n\
  kno new \"Ship execution plan\" --type execution_plan
\n\
  kno plan wave add knots-123 --name \"Wave 1\" --objective \"Foundations\"
\n\
  kno plan step add knots-123 --wave 1 --knot-ids knots-a,knots-b
\n\
  kno plan step add knots-123 --wave 1 --knot-ids knots-c --notes \"After the first pair lands\"
";

const PLAN_WAVE_LONG_ABOUT: &str = "\
Manage waves in an execution plan.
\n\
\n\
Waves are the top-level sequence of the plan. Wave 1 happens before wave 2,
and each wave contains its own ordered steps.";

const PLAN_WAVE_AFTER_HELP: &str = "\
Examples:
\n\
  kno plan wave add knots-123 --name \"Wave 1\" --objective \"Lay the groundwork\"
\n\
  kno plan wave move knots-123 --from 3 --to 2
\n\
  kno plan wave remove knots-123 --wave 2
";

const PLAN_STEP_LONG_ABOUT: &str = "\
Manage steps inside an execution plan wave.
\n\
\n\
Steps are ordered within a wave. Step 1 must finish before step 2 starts.
Multiple knots attached to the same step are intended to be executable in
parallel.";

const PLAN_STEP_AFTER_HELP: &str = "\
Examples:
\n\
  kno plan step add knots-123 --wave 1 --knot-ids knots-a,knots-b
\n\
  kno plan step add knots-123 --wave 1 --knot-ids knots-c --at 1
\n\
  kno plan step move knots-123 --wave 1 --from 3 --to 2
";

const PLAN_WAVE_ADD_AFTER_HELP: &str = "\
Examples:
\n\
  kno plan wave add knots-123 --name \"Wave 1\" --objective \"Build the data model\"
\n\
  kno plan wave add knots-123 --name \"Wave 2\" --objective \"Polish the UX\" --at 2
";

const PLAN_WAVE_REMOVE_AFTER_HELP: &str = "\
Removes the selected wave and any steps it contains. Use `--force` to skip the
cascade confirmation prompt when steps or referenced knots would be affected.";

const PLAN_WAVE_MOVE_AFTER_HELP: &str = "\
Examples:
\n\
  kno plan wave move knots-123 --from 3 --to 1
\n\
  kno plan wave move knots-123 --from 1 --to 2
";

const PLAN_STEP_ADD_AFTER_HELP: &str = "\
Knots listed in one step are grouped as concurrent work.
\n\
\n\
Examples:
\n\
  kno plan step add knots-123 --wave 1 --knot-ids knots-a,knots-b
\n\
  kno plan step add knots-123 --wave 2 --knot-ids knots-c --notes \"Depends on step 1\"
\n\
  kno plan step add knots-123 --wave 2 --knot-ids knots-d --at 1
";

const PLAN_STEP_REMOVE_AFTER_HELP: &str = "\
Removes the selected step. Use `--force` to skip the cascade confirmation
prompt when referenced knots would be affected.";

const PLAN_STEP_MOVE_AFTER_HELP: &str = "\
Examples:
\n\
  kno plan step move knots-123 --wave 1 --from 2 --to 1
\n\
  kno plan step move knots-123 --wave 2 --from 1 --to 3
";

#[derive(Debug, Args)]
#[command(
    about = "Manage execution plan structure.",
    long_about = PLAN_LONG_ABOUT,
    after_help = PLAN_AFTER_HELP
)]
pub struct PlanArgs {
    #[command(subcommand)]
    pub command: PlanSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum PlanSubcommands {
    #[command(about = "Manage waves in an execution plan.")]
    Wave(PlanWaveArgs),
    #[command(about = "Manage steps in an execution plan.")]
    Step(PlanStepArgs),
}

#[derive(Debug, Args)]
#[command(
    about = "Wave commands.",
    long_about = PLAN_WAVE_LONG_ABOUT,
    after_help = PLAN_WAVE_AFTER_HELP
)]
pub struct PlanWaveArgs {
    #[command(subcommand)]
    pub command: PlanWaveSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum PlanWaveSubcommands {
    #[command(about = "Add a wave to an execution plan.", after_help = PLAN_WAVE_ADD_AFTER_HELP)]
    Add(PlanWaveAddArgs),
    #[command(
        about = "Remove a wave from an execution plan.",
        after_help = PLAN_WAVE_REMOVE_AFTER_HELP
    )]
    Remove(PlanWaveRemoveArgs),
    #[command(
        about = "Move a wave within an execution plan.",
        after_help = PLAN_WAVE_MOVE_AFTER_HELP
    )]
    Move(PlanWaveMoveArgs),
}

#[derive(Debug, Args)]
pub struct PlanWaveAddArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(long, help = "Wave name.")]
    pub name: String,
    #[arg(long, help = "Wave objective.")]
    pub objective: String,
    #[arg(long, help = "Insert before the given 1-based wave index.")]
    pub at: Option<u32>,
}

#[derive(Debug, Args)]
pub struct PlanWaveRemoveArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(long, help = "1-based wave index to remove.")]
    pub wave: u32,
    #[arg(long, help = "Skip the interactive cascade confirmation prompt.")]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct PlanWaveMoveArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(long = "from", help = "1-based wave index to move.")]
    pub from_index: u32,
    #[arg(long = "to", help = "Destination 1-based wave index.")]
    pub to_index: u32,
}

#[derive(Debug, Args)]
#[command(
    about = "Step commands.",
    long_about = PLAN_STEP_LONG_ABOUT,
    after_help = PLAN_STEP_AFTER_HELP
)]
pub struct PlanStepArgs {
    #[command(subcommand)]
    pub command: PlanStepSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum PlanStepSubcommands {
    #[command(
        about = "Add a step to an execution plan wave.",
        after_help = PLAN_STEP_ADD_AFTER_HELP
    )]
    Add(PlanStepAddArgs),
    #[command(
        about = "Remove a step from an execution plan wave.",
        after_help = PLAN_STEP_REMOVE_AFTER_HELP
    )]
    Remove(PlanStepRemoveArgs),
    #[command(
        about = "Move a step within an execution plan wave.",
        after_help = PLAN_STEP_MOVE_AFTER_HELP
    )]
    Move(PlanStepMoveArgs),
}

#[derive(Debug, Args)]
pub struct PlanStepAddArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(long, help = "1-based wave index.")]
    pub wave: u32,
    #[arg(
        long = "knot-ids",
        value_delimiter = ',',
        num_args = 1..,
        help = "Comma-separated knot ids to attach to the new step."
    )]
    pub knot_ids: Vec<String>,
    #[arg(long, help = "Optional step notes.")]
    pub notes: Option<String>,
    #[arg(long, help = "Insert before the given 1-based step index.")]
    pub at: Option<u32>,
}

#[derive(Debug, Args)]
pub struct PlanStepRemoveArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(long, help = "1-based wave index.")]
    pub wave: u32,
    #[arg(long, help = "1-based step index to remove.")]
    pub step: u32,
    #[arg(long, help = "Skip the interactive cascade confirmation prompt.")]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct PlanStepMoveArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(long, help = "1-based wave index.")]
    pub wave: u32,
    #[arg(long = "from", help = "1-based step index to move.")]
    pub from_index: u32,
    #[arg(long = "to", help = "Destination 1-based step index.")]
    pub to_index: u32,
}

#[cfg(test)]
#[path = "cli_plan_tests.rs"]
mod tests;

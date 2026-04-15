use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(about = "Manage execution plan structure.")]
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
#[command(about = "Wave commands.")]
pub struct PlanWaveArgs {
    #[command(subcommand)]
    pub command: PlanWaveSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum PlanWaveSubcommands {
    #[command(about = "Add a wave to an execution plan.")]
    Add(PlanWaveAddArgs),
    #[command(about = "Remove a wave from an execution plan.")]
    Remove(PlanWaveRemoveArgs),
    #[command(about = "Move a wave within an execution plan.")]
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
#[command(about = "Step commands.")]
pub struct PlanStepArgs {
    #[command(subcommand)]
    pub command: PlanStepSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum PlanStepSubcommands {
    #[command(about = "Add a step to an execution plan wave.")]
    Add(PlanStepAddArgs),
    #[command(about = "Remove a step from an execution plan wave.")]
    Remove(PlanStepRemoveArgs),
    #[command(about = "Move a step within an execution plan wave.")]
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

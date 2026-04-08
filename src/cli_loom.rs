use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Args)]
#[command(about = "Manage Loom workflows.")]
pub struct LoomArgs {
    #[command(subcommand)]
    pub command: LoomSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum LoomSubcommands {
    #[command(about = "Run the Loom workflow harness.")]
    CompatTest(LoomCompatTestArgs),
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum LoomCompatModeArg {
    Smoke,
    Matrix,
}

#[derive(Debug, Args)]
#[command(about = "Run the Loom workflow harness.")]
pub struct LoomCompatTestArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = LoomCompatModeArg::Smoke,
        help = "Harness depth. Smoke validates the happy path; matrix also exercises failures."
    )]
    pub mode: LoomCompatModeArg,

    #[arg(
        long,
        help = "Keep the generated temp workspace instead of removing it after the run."
    )]
    pub keep_artifacts: bool,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

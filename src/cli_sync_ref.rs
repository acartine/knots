use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(about = "Inspect and migrate the Git ref used for Knots sync data.")]
pub struct SyncRefArgs {
    #[command(subcommand)]
    pub command: SyncRefSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum SyncRefSubcommands {
    #[command(about = "Union existing Knots data into a configured remote ref.")]
    Migrate(SyncRefMigrateArgs),
}

#[derive(Debug, Args)]
#[command(about = "Union Knots event files into a remote ref without force-pushing.")]
pub struct SyncRefMigrateArgs {
    #[arg(
        long = "source",
        required = true,
        help = "Source to read: local or <remote>:<ref>."
    )]
    pub sources: Vec<String>,

    #[arg(long = "target", help = "Target remote ref as <remote>:<ref>.")]
    pub target: Option<String>,
}

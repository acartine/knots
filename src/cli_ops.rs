use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(about = "Update kno binary.")]
pub struct SelfUpdateArgs {
    #[arg(short = 'v', long, help = "Version to install (defaults to latest).")]
    pub version: Option<String>,

    #[arg(
        short = 'r',
        long,
        help = "Repository slug (owner/name) used by installer."
    )]
    pub repo: Option<String>,

    #[arg(short = 'i', long, help = "Install destination directory.")]
    pub install_dir: Option<PathBuf>,

    #[arg(
        short = 'u',
        long,
        default_value = "https://raw.githubusercontent.com/acartine/knots/main/install.sh",
        help = "Installer script URL."
    )]
    pub script_url: String,
}

#[derive(Debug, Args)]
#[command(about = "Uninstall kno binary.")]
pub struct SelfUninstallArgs {
    #[arg(short = 'b', long, help = "Explicit path to installed kno binary.")]
    pub bin_path: Option<PathBuf>,

    #[arg(
        short = 'p',
        long,
        help = "Also remove kno.previous and knots.previous backups."
    )]
    pub remove_previous: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Edge commands.",
    long_about = "Add, remove, or list knot edges."
)]
pub struct EdgeArgs {
    #[command(subcommand)]
    pub command: EdgeSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum EdgeSubcommands {
    #[command(about = "Add an edge: src -[kind]-> dst.")]
    Add(EdgeAddArgs),
    #[command(about = "Remove an edge: src -[kind]-> dst.")]
    Remove(EdgeRemoveArgs),
    #[command(about = "List edges for a knot.")]
    List(EdgeListArgs),
}

#[derive(Debug, Args)]
pub struct EdgeAddArgs {
    #[arg(help = "Source knot full id, stripped id, or hierarchical alias.")]
    pub src: String,
    #[arg(help = "Edge kind, for example parent_of or blocked_by.")]
    pub kind: String,
    #[arg(help = "Destination knot full id, stripped id, or hierarchical alias.")]
    pub dst: String,
}

#[derive(Debug, Args)]
pub struct EdgeRemoveArgs {
    #[arg(help = "Source knot full id, stripped id, or hierarchical alias.")]
    pub src: String,
    #[arg(help = "Edge kind, for example parent_of or blocked_by.")]
    pub kind: String,
    #[arg(help = "Destination knot full id, stripped id, or hierarchical alias.")]
    pub dst: String,
}

#[derive(Debug, Args)]
#[command(about = "List edges for a knot.")]
pub struct EdgeListArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,

    #[arg(
        short = 'd',
        long,
        default_value = "both",
        help = "Edge direction: incoming, outgoing, or both."
    )]
    pub direction: String,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Cold-tier commands.")]
pub struct ColdArgs {
    #[command(subcommand)]
    pub command: ColdSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ColdSubcommands {
    #[command(about = "Pull cold-tier updates from remote.")]
    Sync(crate::cli::SyncArgs),
    #[command(about = "Search cold catalog by term.")]
    Search(ColdSearchArgs),
}

#[derive(Debug, Args)]
#[command(about = "Search cold catalog.")]
pub struct ColdSearchArgs {
    #[arg(help = "Search term.")]
    pub term: String,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Validate event/index files.",
    long_about = "Run fsck checks over .knots data."
)]
pub struct FsckArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Run repository diagnostics.")]
pub struct DoctorArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Run performance harness.")]
pub struct PerfArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,

    #[arg(
        short = 'n',
        long,
        default_value_t = 5,
        help = "Number of harness iterations."
    )]
    pub iterations: u32,

    #[arg(short = 'S', long, help = "Fail when any measurement is over budget.")]
    pub strict: bool,
}

#[derive(Debug, Args)]
#[command(about = "Run compaction operations.")]
pub struct CompactArgs {
    #[arg(
        short = 'w',
        long = "write-snapshots",
        help = "Write snapshot manifests/files."
    )]
    pub write_snapshots: bool,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Rehydrate one knot.")]
pub struct RehydrateArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

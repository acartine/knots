use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};

fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::BrightCyan.on_default() | Effects::BOLD)
        .usage(AnsiColor::BrightYellow.on_default() | Effects::BOLD)
        .literal(AnsiColor::BrightGreen.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::BrightMagenta.on_default())
}

#[derive(Debug, Parser)]
#[command(name = "kno")]
#[command(bin_name = "kno")]
#[command(version)]
#[command(about = "A local-first, git-backed agent memory manager")]
#[command(styles = cli_styles())]
pub struct Cli {
    #[arg(
        short = 'd',
        long,
        env = "KNOTS_DB_PATH",
        default_value = ".knots/cache/state.sqlite",
        help = "Path to the local SQLite cache database."
    )]
    pub db: String,

    #[arg(
        short = 'C',
        long,
        env = "KNOTS_REPO_ROOT",
        default_value = ".",
        help = "Repository root that contains .knots/."
    )]
    pub repo_root: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Create a new knot.")]
    New(NewArgs),
    #[command(about = "Set a knot state with transition validation.")]
    State(StateArgs),
    #[command(about = "Update knot fields and metadata in one command.")]
    Update(UpdateArgs),
    #[command(about = "Self-update the kno binary.")]
    Upgrade(SelfUpdateArgs),
    #[command(about = "Uninstall kno from the system.")]
    Uninstall(SelfUninstallArgs),
    #[command(about = "List knots with filtering and layout.")]
    Ls(ListArgs),
    #[command(about = "Show one knot by id or alias.")]
    Show(ShowArgs),
    #[command(about = "Inspect workflow definitions.")]
    Workflow(WorkflowArgs),
    #[command(about = "Pull knot updates from the remote knots branch.")]
    Pull(SyncArgs),
    #[command(about = "Push local knot updates to the remote knots branch.")]
    Push(SyncArgs),
    #[command(about = "Push then pull knot updates.")]
    Sync(SyncArgs),
    #[command(about = "Initialize local store, add .knots gitignore entries, and init remote.")]
    Init,
    #[command(about = "Remove local knots store artifacts and delete remote knots branch.")]
    Uninit,
    #[command(about = "Create and bootstrap the remote knots branch.")]
    InitRemote,
    #[command(about = "Validate on-disk knots event/index data.")]
    Fsck(FsckArgs),
    #[command(about = "Run repository health diagnostics.")]
    Doctor(DoctorArgs),
    #[command(about = "Run performance harness checks.")]
    Perf(PerfArgs),
    #[command(about = "Run compaction operations.")]
    Compact(CompactArgs),
    #[command(about = "Cold-tier operations.")]
    Cold(ColdArgs),
    #[command(about = "Rehydrate one knot from warm/cold/event data.")]
    Rehydrate(RehydrateArgs),
    #[command(about = "Manage knot edges.")]
    Edge(EdgeArgs),
    #[command(about = "Import knots from external sources.")]
    Import(ImportArgs),
    #[command(name = "self")]
    #[command(about = "Self-management commands.")]
    SelfManage(SelfArgs),
}

#[derive(Debug, Args)]
#[command(about = "Create a new knot.")]
pub struct NewArgs {
    #[arg(help = "Knot title.")]
    pub title: String,

    #[arg(short = 'b', long, help = "Optional body/description text.")]
    pub body: Option<String>,

    #[arg(
        short = 's',
        long,
        help = "Initial knot state (defaults to workflow initial_state)."
    )]
    pub state: Option<String>,

    #[arg(short = 'w', long, help = "Workflow id.")]
    pub workflow: String,
}

#[derive(Debug, Args)]
#[command(about = "Set knot state.")]
pub struct StateArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(help = "Target state.")]
    pub state: String,

    #[arg(short = 'f', long, help = "Force an otherwise invalid transition.")]
    pub force: bool,

    #[arg(
        short = 'm',
        long = "if-match",
        help = "Require this workflow etag to match before writing."
    )]
    pub if_match: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Update knot fields and metadata.")]
pub struct UpdateArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,

    #[arg(short = 't', long, help = "Set title.")]
    pub title: Option<String>,

    #[arg(short = 'd', long, help = "Set description.")]
    pub description: Option<String>,

    #[arg(short = 'p', long, help = "Set priority (0-4).")]
    pub priority: Option<i64>,

    #[arg(short = 's', long, help = "Set state.")]
    pub status: Option<String>,

    #[arg(short = 'k', long = "type", help = "Set knot type.")]
    pub knot_type: Option<String>,

    #[arg(short = 'a', long = "add-tag", help = "Add tag (repeatable).")]
    pub add_tags: Vec<String>,

    #[arg(short = 'r', long = "remove-tag", help = "Remove tag (repeatable).")]
    pub remove_tags: Vec<String>,

    #[arg(short = 'n', long = "add-note", help = "Add note content.")]
    pub add_note: Option<String>,

    #[arg(long = "note-username", help = "Note author username.")]
    pub note_username: Option<String>,

    #[arg(long = "note-datetime", help = "Note datetime (RFC3339).")]
    pub note_datetime: Option<String>,

    #[arg(long = "note-agentname", help = "Agent name for note metadata.")]
    pub note_agentname: Option<String>,

    #[arg(long = "note-model", help = "Model name for note metadata.")]
    pub note_model: Option<String>,

    #[arg(long = "note-version", help = "Model/version tag for note metadata.")]
    pub note_version: Option<String>,

    #[arg(
        short = 'H',
        long = "add-handoff-capsule",
        help = "Add handoff capsule content."
    )]
    pub add_handoff_capsule: Option<String>,

    #[arg(long = "handoff-username", help = "Handoff author username.")]
    pub handoff_username: Option<String>,

    #[arg(long = "handoff-datetime", help = "Handoff datetime (RFC3339).")]
    pub handoff_datetime: Option<String>,

    #[arg(long = "handoff-agentname", help = "Agent name for handoff metadata.")]
    pub handoff_agentname: Option<String>,

    #[arg(long = "handoff-model", help = "Model name for handoff metadata.")]
    pub handoff_model: Option<String>,

    #[arg(
        long = "handoff-version",
        help = "Model/version tag for handoff metadata."
    )]
    pub handoff_version: Option<String>,

    #[arg(
        short = 'm',
        long = "if-match",
        help = "Require this workflow etag to match before writing."
    )]
    pub if_match: Option<String>,

    #[arg(
        short = 'f',
        long,
        help = "Force invalid state transitions when --status is used."
    )]
    pub force: bool,
}

#[derive(Debug, Args)]
#[command(about = "List knots.")]
pub struct ListArgs {
    #[arg(short = 'a', long = "all", help = "Include shipped knots.")]
    pub all: bool,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,

    #[arg(short = 's', long, help = "Filter by state.")]
    pub state: Option<String>,

    #[arg(short = 't', long = "type", help = "Filter by knot type.")]
    pub knot_type: Option<String>,

    #[arg(short = 'w', long = "workflow", help = "Filter by workflow id.")]
    pub workflow_id: Option<String>,

    #[arg(short = 'g', long = "tag", help = "Require tag (repeatable).")]
    pub tags: Vec<String>,

    #[arg(
        short = 'q',
        long,
        help = "Text query over id, alias, title, and description."
    )]
    pub query: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Show one knot.")]
pub struct ShowArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Workflow commands.")]
pub struct WorkflowArgs {
    #[command(subcommand)]
    pub command: WorkflowSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum WorkflowSubcommands {
    #[command(about = "List available workflows.", alias = "ls")]
    List(WorkflowListArgs),
    #[command(about = "Show one workflow definition.")]
    Show(WorkflowShowArgs),
}

#[derive(Debug, Args)]
#[command(about = "List workflows.")]
pub struct WorkflowListArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Show one workflow definition.")]
pub struct WorkflowShowArgs {
    #[arg(help = "Workflow id.")]
    pub id: String,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Replication output options.")]
pub struct SyncArgs {
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
#[command(about = "Cold-tier commands.")]
pub struct ColdArgs {
    #[command(subcommand)]
    pub command: ColdSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ColdSubcommands {
    #[command(about = "Pull cold-tier updates from remote.")]
    Sync(SyncArgs),
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
#[command(about = "Rehydrate one knot.")]
pub struct RehydrateArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
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
#[command(about = "Import commands.")]
pub struct ImportArgs {
    #[command(subcommand)]
    pub command: ImportSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ImportSubcommands {
    #[command(about = "Import from JSONL event records.")]
    Jsonl(ImportJsonlArgs),
    #[command(about = "Import from a Dolt repository.")]
    Dolt(ImportDoltArgs),
    #[command(about = "Show import run status history.")]
    Status(ImportStatusArgs),
}

#[derive(Debug, Args)]
#[command(about = "Import from JSONL.")]
pub struct ImportJsonlArgs {
    #[arg(short = 'f', long, help = "Path to JSONL input file.")]
    pub file: String,

    #[arg(
        short = 'n',
        long,
        help = "Run import validation without writing events."
    )]
    pub dry_run: bool,

    #[arg(
        short = 's',
        long,
        help = "Only import records on/after this checkpoint."
    )]
    pub since: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Import from Dolt.")]
pub struct ImportDoltArgs {
    #[arg(short = 'r', long, help = "Dolt repository URL or path.")]
    pub repo: String,

    #[arg(
        short = 'n',
        long,
        help = "Run import validation without writing events."
    )]
    pub dry_run: bool,

    #[arg(
        short = 's',
        long,
        help = "Only import records on/after this checkpoint."
    )]
    pub since: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Show import statuses.")]
pub struct ImportStatusArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Self-management commands.")]
pub struct SelfArgs {
    #[command(subcommand)]
    pub command: SelfSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum SelfSubcommands {
    #[command(about = "Update kno binary.")]
    Update(SelfUpdateArgs),
    #[command(about = "Uninstall kno binary and aliases.")]
    Uninstall(SelfUninstallArgs),
}

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

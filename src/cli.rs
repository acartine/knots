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
        long,
        env = "KNOTS_DB_PATH",
        default_value = ".knots/cache/state.sqlite"
    )]
    pub db: String,

    #[arg(long, env = "KNOTS_REPO_ROOT", default_value = ".")]
    pub repo_root: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    New(NewArgs),
    State(StateArgs),
    Update(UpdateArgs),
    Upgrade(SelfUpdateArgs),
    Uninstall(SelfUninstallArgs),
    Ls(ListArgs),
    Show(ShowArgs),
    Sync(SyncArgs),
    Edge(EdgeArgs),
    Import(ImportArgs),
    #[command(name = "self")]
    SelfManage(SelfArgs),
}

#[derive(Debug, Args)]
pub struct NewArgs {
    pub title: String,

    #[arg(long)]
    pub body: Option<String>,

    #[arg(long, default_value = "idea")]
    pub state: String,
}

#[derive(Debug, Args)]
pub struct StateArgs {
    pub id: String,
    pub state: String,

    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    pub id: String,

    #[arg(long)]
    pub title: Option<String>,

    #[arg(long)]
    pub description: Option<String>,

    #[arg(long)]
    pub priority: Option<i64>,

    #[arg(long)]
    pub status: Option<String>,

    #[arg(long = "type")]
    pub knot_type: Option<String>,

    #[arg(long = "add-tag")]
    pub add_tags: Vec<String>,

    #[arg(long = "remove-tag")]
    pub remove_tags: Vec<String>,

    #[arg(long = "add-note")]
    pub add_note: Option<String>,

    #[arg(long = "note-username")]
    pub note_username: Option<String>,

    #[arg(long = "note-datetime")]
    pub note_datetime: Option<String>,

    #[arg(long = "note-agentname")]
    pub note_agentname: Option<String>,

    #[arg(long = "note-model")]
    pub note_model: Option<String>,

    #[arg(long = "note-version")]
    pub note_version: Option<String>,

    #[arg(long = "add-handoff-capsule")]
    pub add_handoff_capsule: Option<String>,

    #[arg(long = "handoff-username")]
    pub handoff_username: Option<String>,

    #[arg(long = "handoff-datetime")]
    pub handoff_datetime: Option<String>,

    #[arg(long = "handoff-agentname")]
    pub handoff_agentname: Option<String>,

    #[arg(long = "handoff-model")]
    pub handoff_model: Option<String>,

    #[arg(long = "handoff-version")]
    pub handoff_version: Option<String>,

    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(long)]
    pub state: Option<String>,

    #[arg(long = "type")]
    pub knot_type: Option<String>,

    #[arg(long = "tag")]
    pub tags: Vec<String>,

    #[arg(long)]
    pub query: Option<String>,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    pub id: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct EdgeArgs {
    #[command(subcommand)]
    pub command: EdgeSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum EdgeSubcommands {
    Add(EdgeAddArgs),
    Remove(EdgeRemoveArgs),
    List(EdgeListArgs),
}

#[derive(Debug, Args)]
pub struct EdgeAddArgs {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Args)]
pub struct EdgeRemoveArgs {
    pub src: String,
    pub kind: String,
    pub dst: String,
}

#[derive(Debug, Args)]
pub struct EdgeListArgs {
    pub id: String,

    #[arg(long, default_value = "both")]
    pub direction: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    #[command(subcommand)]
    pub command: ImportSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ImportSubcommands {
    Jsonl(ImportJsonlArgs),
    Dolt(ImportDoltArgs),
    Status(ImportStatusArgs),
}

#[derive(Debug, Args)]
pub struct ImportJsonlArgs {
    #[arg(long)]
    pub file: String,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Debug, Args)]
pub struct ImportDoltArgs {
    #[arg(long)]
    pub repo: String,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Debug, Args)]
pub struct ImportStatusArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SelfArgs {
    #[command(subcommand)]
    pub command: SelfSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum SelfSubcommands {
    Update(SelfUpdateArgs),
    Uninstall(SelfUninstallArgs),
}

#[derive(Debug, Args)]
pub struct SelfUpdateArgs {
    #[arg(long)]
    pub version: Option<String>,

    #[arg(long)]
    pub repo: Option<String>,

    #[arg(long)]
    pub install_dir: Option<PathBuf>,

    #[arg(
        long,
        default_value = "https://raw.githubusercontent.com/acartine/knots/main/install.sh"
    )]
    pub script_url: String,
}

#[derive(Debug, Args)]
pub struct SelfUninstallArgs {
    #[arg(long)]
    pub bin_path: Option<PathBuf>,

    #[arg(long)]
    pub remove_previous: bool,
}

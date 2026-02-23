use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "knots")]
#[command(version)]
#[command(about = "A local-first, git-backed issue tracker")]
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
    Ls(ListArgs),
    Show(ShowArgs),
    Sync(SyncArgs),
    Edge(EdgeArgs),
    Import(ImportArgs),
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
pub struct ListArgs {
    #[arg(long)]
    pub json: bool,
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

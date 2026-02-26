use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};

pub use crate::cli_ops::*;

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
#[allow(clippy::large_enum_variant)]
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
    #[command(about = "Inspect and manage workflow profiles.")]
    Profile(ProfileArgs),
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
    #[command(about = "Create remote knots branch and ensure .knots is gitignored.")]
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
    #[command(about = "Advance a knot to its next happy-path state.")]
    Next(NextArgs),
    #[command(about = "Print the skill prompt for a knot's next action state.")]
    Skill(SkillArgs),
    #[command(about = "Quick-create a knot using the default quick profile.")]
    Q(QuickNewArgs),
    #[command(about = "Generate or install shell completions.")]
    Completions(CompletionsArgs),
}

#[derive(Debug, Args)]
#[command(about = "Quick-create a knot.")]
pub struct QuickNewArgs {
    #[arg(help = "Knot title.")]
    pub title: String,

    #[arg(short = 'd', long = "desc", help = "Optional description text.")]
    pub desc: Option<String>,

    #[arg(
        short = 's',
        long,
        help = "Initial knot state (defaults to profile initial_state)."
    )]
    pub state: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Generate or install shell completions.")]
pub struct CompletionsArgs {
    #[arg(help = "Shell name (bash, zsh, fish). Auto-detected if omitted.")]
    pub shell: Option<String>,

    #[arg(
        short = 'i',
        long = "install",
        help = "Write completions to the canonical path for the shell."
    )]
    pub install: bool,
}

#[derive(Debug, Args)]
#[command(about = "Create a new knot.")]
pub struct NewArgs {
    #[arg(help = "Knot title.")]
    pub title: String,

    #[arg(short = 'd', long = "desc", help = "Optional description text.")]
    pub desc: Option<String>,

    #[arg(
        short = 's',
        long,
        help = "Initial knot state (defaults to profile initial_state)."
    )]
    pub state: Option<String>,

    #[arg(
        short = 'p',
        long = "profile",
        help = "Profile id (defaults to the user default profile)."
    )]
    pub profile: Option<String>,

    #[arg(
        short = 'f',
        long = "fast",
        help = "Use the default quick profile (skips planning)."
    )]
    pub fast: bool,
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
        help = "Require this profile etag to match before writing."
    )]
    pub if_match: Option<String>,

    #[arg(long = "actor-kind", help = "Actor kind for the step: human or agent.")]
    pub actor_kind: Option<String>,

    #[arg(long = "agent-name", help = "Agent name for step metadata.")]
    pub agent_name: Option<String>,

    #[arg(long = "agent-model", help = "Agent model for step metadata.")]
    pub agent_model: Option<String>,

    #[arg(long = "agent-version", help = "Agent version for step metadata.")]
    pub agent_version: Option<String>,
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
        help = "Require this profile etag to match before writing."
    )]
    pub if_match: Option<String>,

    #[arg(long = "actor-kind", help = "Actor kind for the step: human or agent.")]
    pub actor_kind: Option<String>,

    #[arg(long = "agent-name", help = "Agent name for step metadata.")]
    pub agent_name: Option<String>,

    #[arg(long = "agent-model", help = "Agent model for step metadata.")]
    pub agent_model: Option<String>,

    #[arg(long = "agent-version", help = "Agent version for step metadata.")]
    pub agent_version: Option<String>,

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
    #[arg(
        short = 'a',
        long = "all",
        help = "Include shipped, abandoned, and deferred knots."
    )]
    pub all: bool,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,

    #[arg(short = 's', long, help = "Filter by state.")]
    pub state: Option<String>,

    #[arg(short = 't', long = "type", help = "Filter by knot type.")]
    pub knot_type: Option<String>,

    #[arg(short = 'p', long = "profile", help = "Filter by profile id.")]
    pub profile_id: Option<String>,

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
#[command(about = "Profile commands.")]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub command: ProfileSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum ProfileSubcommands {
    #[command(about = "List available profiles.", alias = "ls")]
    List(ProfileListArgs),
    #[command(about = "Show one profile definition.")]
    Show(ProfileShowArgs),
    #[command(about = "Set the user default profile id.")]
    SetDefault(ProfileSetDefaultArgs),
    #[command(about = "Set the user default quick profile id.")]
    SetDefaultQuick(ProfileSetDefaultArgs),
    #[command(about = "Set one knot profile and optionally remap state.")]
    Set(ProfileSetArgs),
}

#[derive(Debug, Args)]
#[command(about = "List profiles.")]
pub struct ProfileListArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Show one profile definition.")]
pub struct ProfileShowArgs {
    #[arg(help = "Profile id.")]
    pub id: String,

    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Set the user default profile.")]
pub struct ProfileSetDefaultArgs {
    #[arg(help = "Profile id.")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(about = "Set one knot profile.")]
pub struct ProfileSetArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,

    #[arg(help = "Target profile id.")]
    pub profile: String,

    #[arg(short = 's', long, help = "Target state in the new profile.")]
    pub state: Option<String>,

    #[arg(
        short = 'm',
        long = "if-match",
        help = "Require this profile etag to match before writing."
    )]
    pub if_match: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Replication output options.")]
pub struct SyncArgs {
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Advance knot to next state.")]
pub struct NextArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(about = "Print skill for knot's next state.")]
pub struct SkillArgs {
    #[arg(help = "Knot id/alias, or a state name (e.g. planning).")]
    pub id: String,
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;

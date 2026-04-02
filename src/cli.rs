use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};

use clap::CommandFactory;

pub use crate::cli_agent::*;
pub use crate::cli_loom::*;
pub use crate::cli_ops::*;
pub use crate::cli_skills::*;
pub use crate::cli_workflow::*;

pub fn styled_command() -> clap::Command {
    Cli::command()
}

fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::BrightCyan.on_default() | Effects::BOLD)
        .usage(AnsiColor::BrightYellow.on_default() | Effects::BOLD)
        .literal(AnsiColor::BrightGreen.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::BrightMagenta.on_default())
        .valid(AnsiColor::Cyan.on_default() | Effects::BOLD)
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
        help = "Path to the local SQLite cache database."
    )]
    pub db: Option<String>,

    #[arg(
        short = 'C',
        long,
        env = "KNOTS_REPO_ROOT",
        help = "Repository root to target for this command."
    )]
    pub repo_root: Option<PathBuf>,

    #[arg(
        long,
        env = "KNOTS_PROJECT",
        help = "Named Knots project to target for this command."
    )]
    pub project: Option<String>,

    #[arg(
        long,
        env = "KNO_TRACE",
        global = true,
        help = "Emit per-command trace timings to stderr."
    )]
    pub trace: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    #[command(about = "Create a new knot.", alias = "create")]
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
    #[command(about = "Manage installed workflows.")]
    Workflow(WorkflowArgs),
    #[command(about = "Manage named Knots projects.")]
    Project(ProjectArgs),
    #[command(about = "Manage Loom compatibility checks.")]
    Loom(LoomArgs),
    #[command(about = "Pull knot updates from the remote knots branch.")]
    Pull(SyncArgs),
    #[command(about = "Push local knot updates to the remote knots branch.")]
    Push(SyncArgs),
    #[command(about = "Push then pull knot updates.")]
    Sync(SyncArgs),
    #[command(about = "Initialize local store and remote or named project state.")]
    Init,
    #[command(about = "Remove local knots store artifacts and delete remote branch.")]
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
    #[command(about = "Manage gate decisions and metadata.")]
    Gate(GateArgs),
    #[command(about = "Advance a knot to its next happy-path state.")]
    Next(NextArgs),
    #[command(
        about = "Roll back a knot from an action state to its prior ready state.",
        alias = "rb"
    )]
    Rollback(RollbackArgs),
    #[command(about = "Print the skill prompt for a knot's next action state.")]
    Skill(SkillArgs),
    #[command(about = "Manage Knots-managed agent skills.")]
    Skills(SkillsArgs),
    #[command(about = "Quick-create a knot using the default quick profile.")]
    Q(QuickNewArgs),
    #[command(about = "Generate or install shell completions.")]
    Completions(CompletionsArgs),
    #[command(about = "Peek at the highest-priority claimable knot.")]
    Poll(PollArgs),
    #[command(about = "Claim a knot and get its action prompt.")]
    Claim(ClaimArgs),
    #[command(about = "List knots queued for action (ready_for_* states).")]
    Ready(ReadyArgs),
    #[command(about = "Manage step execution history.")]
    Step(StepArgs),
    #[command(about = "Manage lease sessions.")]
    Lease(LeaseArgs),
    #[command(about = "Manage git sync hooks (post-merge).")]
    Hooks(HooksArgs),
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

    #[arg(long, help = "Optional acceptance criteria.")]
    pub acceptance: Option<String>,

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
        short = 'w',
        long = "workflow",
        help = "Workflow id (defaults to the repo default workflow)."
    )]
    pub workflow: Option<String>,

    #[arg(short = 'k', long = "type", help = "Knot type (work or gate).")]
    pub knot_type: Option<String>,

    #[arg(long = "gate-owner-kind", help = "Gate owner kind: human or agent.")]
    pub gate_owner_kind: Option<String>,

    #[arg(
        long = "gate-failure-mode",
        help = "Gate failure mapping '<invariant>=<knot-id[,knot-id...]>' (repeatable)."
    )]
    pub gate_failure_modes: Vec<String>,

    #[arg(
        short = 'f',
        long = "fast",
        help = "Use the default quick profile (skips planning)."
    )]
    pub fast: bool,

    #[arg(long, help = "Bind a lease to this knot.")]
    pub lease: Option<String>,
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
        long = "cascade-terminal-descendants",
        help = "Approve cascading a terminal state to all descendants."
    )]
    pub cascade_terminal_descendants: bool,

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
#[command(about = "List knots.")]
pub struct ListArgs {
    #[arg(
        short = 'a',
        long = "all",
        help = "Include shipped and abandoned knots."
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

    #[arg(short = 'v', long, help = "Show all notes and handoff capsules.")]
    pub verbose: bool,
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

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cli_tests_ext.rs"]
mod tests_ext;

#[cfg(test)]
#[path = "cli_lease_tests.rs"]
mod lease_tests;

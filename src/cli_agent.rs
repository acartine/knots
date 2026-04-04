use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[command(about = "Advance knot to next state.")]
pub struct NextArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(
        value_name = "currentState",
        help = "Legacy positional expected state for optimistic progression."
    )]
    pub current_state: Option<String>,
    #[arg(
        long = "expected-state",
        value_name = "STATE",
        help = "Reject transition unless the knot is currently in this state."
    )]
    pub expected_state: Option<String>,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
    #[arg(long = "actor-kind", help = "Actor kind for the step: human or agent.")]
    pub actor_kind: Option<String>,
    #[arg(long = "agent-name", help = "Agent name for step metadata.")]
    pub agent_name: Option<String>,
    #[arg(long = "agent-model", help = "Agent model for step metadata.")]
    pub agent_model: Option<String>,
    #[arg(long = "agent-version", help = "Agent version for step metadata.")]
    pub agent_version: Option<String>,
    #[arg(
        long = "cascade-terminal-descendants",
        help = "Approve cascading a terminal state to all descendants."
    )]
    pub cascade_terminal_descendants: bool,
    #[arg(long, help = "Validate lease ownership before advancing.")]
    pub lease: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Roll back a knot from an action state to its prior ready state.")]
pub struct RollbackArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(
        long = "dry-run",
        help = "Preview the rollback target without mutating state."
    )]
    pub dry_run: bool,
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
#[command(about = "Print skill for knot's next state.")]
pub struct SkillArgs {
    #[arg(help = "Knot id/alias, or a state name (e.g. planning).")]
    pub id: String,
}

#[derive(Debug, Args)]
#[command(about = "Peek at the highest-priority claimable knot.")]
pub struct PollArgs {
    #[arg(help = "Optional stage filter (e.g. implementation).")]
    pub stage: Option<String>,
    #[arg(
        short = 'o',
        long = "owner",
        help = "Owner kind filter (default: agent)."
    )]
    pub owner: Option<String>,
    #[arg(long = "claim", help = "Atomically claim the top item.")]
    pub claim: bool,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
    #[arg(long = "agent-name", help = "Agent name for claim metadata.")]
    pub agent_name: Option<String>,
    #[arg(long = "agent-model", help = "Agent model for claim metadata.")]
    pub agent_model: Option<String>,
    #[arg(long = "agent-version", help = "Agent version for claim metadata.")]
    pub agent_version: Option<String>,
    #[arg(long, help = "Lease timeout in seconds (default: 600).")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Args)]
#[command(about = "Claim a knot and get its action prompt.")]
pub struct ClaimArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
    #[arg(long = "agent-name", help = "Agent name for claim metadata.")]
    pub agent_name: Option<String>,
    #[arg(long = "agent-model", help = "Agent model for claim metadata.")]
    pub agent_model: Option<String>,
    #[arg(long = "agent-version", help = "Agent version for claim metadata.")]
    pub agent_version: Option<String>,
    #[arg(long, help = "Show claim output without advancing state.")]
    pub peek: bool,
    #[arg(short = 'v', long, help = "Show all notes and handoff capsules.")]
    pub verbose: bool,
    #[arg(long, help = "Bind an existing lease instead of creating a new one.")]
    pub lease: Option<String>,
    #[arg(long, help = "Lease timeout in seconds (default: 600).")]
    pub timeout_seconds: Option<u64>,
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
    #[arg(long, help = "Set acceptance criteria.")]
    pub acceptance: Option<String>,
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
    #[arg(
        long = "add-invariant",
        help = "Add invariant '<Scope|State>:<condition>' (repeatable)."
    )]
    pub add_invariants: Vec<String>,
    #[arg(
        long = "remove-invariant",
        help = "Remove invariant '<Scope|State>:<condition>' (repeatable)."
    )]
    pub remove_invariants: Vec<String>,
    #[arg(long = "clear-invariants", help = "Clear all invariants.")]
    pub clear_invariants: bool,
    #[arg(long = "gate-owner-kind", help = "Gate owner kind: human or agent.")]
    pub gate_owner_kind: Option<String>,
    #[arg(
        long = "gate-failure-mode",
        help = "Replace gate failure mappings (repeatable)."
    )]
    pub gate_failure_modes: Vec<String>,
    #[arg(
        long = "clear-gate-failure-modes",
        help = "Clear all gate failure-mode mappings."
    )]
    pub clear_gate_failure_modes: bool,
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
    #[arg(
        long = "cascade-terminal-descendants",
        help = "Approve cascading a terminal state to all descendants."
    )]
    pub cascade_terminal_descendants: bool,
    #[arg(long, help = "Bind a lease to this knot.")]
    pub lease: Option<String>,
}

#[derive(Debug, Args)]
#[command(about = "Manage step execution history.")]
pub struct StepArgs {
    #[command(subcommand)]
    pub command: StepSubcommands,
}
#[derive(Debug, Subcommand)]
pub enum StepSubcommands {
    #[command(about = "Record a new agent on the active step.")]
    Annotate(StepAnnotateArgs),
}
#[derive(Debug, Args)]
#[command(about = "Annotate active step with new agent metadata.")]
pub struct StepAnnotateArgs {
    #[arg(help = "Knot full id, stripped id, or hierarchical alias.")]
    pub id: String,
    #[arg(long = "agent-name", help = "Agent name.")]
    pub agent_name: Option<String>,
    #[arg(long = "agent-model", help = "Agent model.")]
    pub agent_model: Option<String>,
    #[arg(long = "agent-version", help = "Agent version.")]
    pub agent_version: Option<String>,
    #[arg(long = "actor-kind", help = "Actor kind: human or agent.")]
    pub actor_kind: Option<String>,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

#[derive(Debug, Args)]
#[command(about = "Manage lease sessions.")]
pub struct LeaseArgs {
    #[command(subcommand)]
    pub command: LeaseSubcommands,
}
#[derive(Debug, Subcommand)]
pub enum LeaseSubcommands {
    #[command(about = "Create a new lease.")]
    Create(LeaseCreateArgs),
    #[command(about = "Show a lease.")]
    Show(LeaseShowArgs),
    #[command(about = "Terminate an active lease.")]
    Terminate(LeaseTerminateArgs),
    #[command(about = "Extend an active lease.")]
    Extend(LeaseExtendArgs),
    #[command(about = "List leases.", alias = "ls")]
    List(LeaseListArgs),
}
#[derive(Debug, Args)]
#[command(about = "Create a new lease.")]
pub struct LeaseCreateArgs {
    #[arg(long, help = "Nickname for the lease session.")]
    pub nickname: String,
    #[arg(
        long = "type",
        default_value = "agent",
        help = "Lease type: agent or manual."
    )]
    pub lease_type: String,
    #[arg(long = "agent-type", help = "Agent type: cli or api.")]
    pub agent_type: Option<String>,
    #[arg(long, help = "Agent provider (e.g. Anthropic).")]
    pub provider: Option<String>,
    #[arg(long = "agent-name", help = "Agent name (e.g. claude).")]
    pub agent_name: Option<String>,
    #[arg(long, help = "Model name (e.g. opus).")]
    pub model: Option<String>,
    #[arg(long = "model-version", help = "Model version (e.g. 4.6).")]
    pub model_version: Option<String>,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
    #[arg(long, help = "Lease timeout in seconds (default: 600).")]
    pub timeout_seconds: Option<u64>,
}
#[derive(Debug, Args)]
#[command(about = "Show a lease.")]
pub struct LeaseShowArgs {
    #[arg(help = "Lease knot id.")]
    pub id: String,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}
#[derive(Debug, Args)]
#[command(about = "Terminate a lease.")]
pub struct LeaseTerminateArgs {
    #[arg(help = "Lease knot id.")]
    pub id: String,
}
#[derive(Debug, Args)]
#[command(about = "Extend an active lease.")]
pub struct LeaseExtendArgs {
    #[arg(long = "lease-id", help = "Lease knot id to extend.")]
    pub lease_id: String,
    #[arg(long, help = "New timeout in seconds (default: 600).")]
    pub timeout_seconds: Option<u64>,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}
#[derive(Debug, Args)]
#[command(about = "List leases.")]
pub struct LeaseListArgs {
    #[arg(short = 'a', long = "all", help = "Include terminated leases.")]
    pub all: bool,
    #[arg(short = 'j', long, help = "Render machine-readable JSON.")]
    pub json: bool,
}

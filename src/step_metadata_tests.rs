use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::knot_type::KnotType;
use crate::installed_workflows::bundle_toml::parse_bundle_toml;
use crate::workflow::{OwnerKind, ProfileRegistry};
use crate::workflow_runtime::step_metadata_for_state;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    crate::installed_workflows::ensure_builtin_workflows_registered(&path)
        .expect("builtin workflows should register");
    path
}

// ── Pattern 1: Built-in autopilot (all-agent) ────────────

#[test]
fn builtin_autopilot_resolves_agent_owner_for_implementation() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(
        &registry,
        "autopilot",
        KnotType::Work,
        &gate,
        "implementation",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "implementation");
    assert_eq!(
        meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Agent)
    );
    assert!(meta.output.is_some());
    assert_eq!(
        meta.output.as_ref().map(|o| o.artifact_type.as_str()),
        Some("remote_main"),
    );
}

#[test]
fn builtin_autopilot_queue_state_resolves_through_action() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(
        &registry,
        "autopilot",
        KnotType::Work,
        &gate,
        "ready_for_implementation",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "implementation");
    assert_eq!(
        meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Agent)
    );
}

#[test]
fn builtin_autopilot_terminal_returns_none() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(&registry, "autopilot", KnotType::Work, &gate, "shipped")
        .expect("should resolve");
    assert!(meta.is_none());
}

// ── Pattern 2: Custom bundle with review_hint ─────────────

const BUNDLE_WITH_REVIEW_HINT: &str = r#"
[workflow]
name = "review_flow"
version = 1
default_profile = "reviewed"

[states.ready_for_work]
kind = "queue"

[states.work]
kind = "action"
executor = "agent"
prompt = "work"
output = "branch"
output_hint = "git log"

[states.ready_for_review]
kind = "queue"

[states.review]
kind = "action"
executor = "human"
prompt = "review"
output = "approval"
review_hint = "Check tests pass and coverage meets threshold"

[states.done]
kind = "terminal"

[states.deferred]
kind = "escape"

[states.abandoned]
kind = "terminal"

[steps.impl]
queue = "ready_for_work"
action = "work"

[steps.rev]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "impl"
gate = "rev"

[profiles.reviewed]
phases = ["main"]

[prompts.work]
accept = ["Built output"]
body = "Ship it."

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "deferred"

[prompts.review]
accept = ["Reviewed"]
body = "Review it."

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

#[test]
fn custom_bundle_review_hint_attached_to_review_state() {
    let workflow = parse_bundle_toml(BUNDLE_WITH_REVIEW_HINT).expect("bundle should parse");
    let profile = workflow
        .require_profile("reviewed")
        .expect("profile should exist");
    assert_eq!(
        profile.review_hints.get("review").map(String::as_str),
        Some("Check tests pass and coverage meets threshold"),
    );
    assert!(!profile.review_hints.contains_key("work"));
}

#[test]
fn custom_bundle_step_metadata_includes_review_hint() {
    let workspace = unique_workspace("knots-stepmeta-review");
    let bundle = workspace.join("review_flow.toml");
    std::fs::write(&bundle, BUNDLE_WITH_REVIEW_HINT).expect("bundle should write");
    crate::installed_workflows::install_bundle(&workspace, &bundle).expect("install bundle");

    let registry = ProfileRegistry::load_for_repo(&workspace).expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(
        &registry,
        "review_flow/reviewed",
        KnotType::Work,
        &gate,
        "review",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "review");
    assert_eq!(
        meta.review_hint.as_deref(),
        Some("Check tests pass and coverage meets threshold"),
    );
    assert_eq!(
        meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Human)
    );
    assert_eq!(
        meta.output.as_ref().map(|o| o.artifact_type.as_str()),
        Some("approval"),
    );

    let work_meta = step_metadata_for_state(
        &registry,
        "review_flow/reviewed",
        KnotType::Work,
        &gate,
        "work",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(work_meta.action_state, "work");
    assert!(work_meta.review_hint.is_none());
    assert_eq!(
        work_meta.output.as_ref().map(|o| o.artifact_type.as_str()),
        Some("branch"),
    );
    assert_eq!(
        work_meta
            .output
            .as_ref()
            .and_then(|o| o.access_hint.as_deref()),
        Some("git log"),
    );

    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn custom_bundle_review_hint_round_trips_json() {
    use crate::installed_workflows::bundle_json::parse_bundle_json;
    use crate::installed_workflows::bundle_toml::render_json_bundle_from_toml;

    let json =
        render_json_bundle_from_toml(BUNDLE_WITH_REVIEW_HINT).expect("json render should work");
    let workflow = parse_bundle_json(&json).expect("json should parse");
    let profile = workflow
        .require_profile("reviewed")
        .expect("profile should exist");
    assert_eq!(
        profile.review_hints.get("review").map(String::as_str),
        Some("Check tests pass and coverage meets threshold"),
    );
}

// ── Pattern 3: Semiauto (human-gated reviews) ─────────────

#[test]
fn semiauto_implementation_review_is_human_owned() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(
        &registry,
        "semiauto",
        KnotType::Work,
        &gate,
        "implementation_review",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "implementation_review");
    assert_eq!(
        meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Human)
    );
}

#[test]
fn semiauto_implementation_is_agent_owned() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(
        &registry,
        "semiauto",
        KnotType::Work,
        &gate,
        "implementation",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "implementation");
    assert_eq!(
        meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Agent)
    );
}

#[test]
fn semiauto_queue_resolves_to_next_action_owner() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(
        &registry,
        "semiauto",
        KnotType::Work,
        &gate,
        "ready_for_implementation_review",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "implementation_review");
    assert_eq!(
        meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Human)
    );
}

// ── Gate knot metadata ────────────────────────────────────

#[test]
fn gate_knot_step_metadata_reflects_gate_owner() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData {
        owner_kind: GateOwnerKind::Human,
        ..Default::default()
    };
    let meta = step_metadata_for_state(
        &registry,
        "evaluate",
        KnotType::Gate,
        &gate,
        "ready_to_evaluate",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "evaluating");
    assert_eq!(
        meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Human)
    );
    assert_eq!(meta.action_kind.as_deref(), Some("gate"));

    let agent_gate = GateData {
        owner_kind: GateOwnerKind::Agent,
        ..Default::default()
    };
    let agent_meta = step_metadata_for_state(
        &registry,
        "evaluate",
        KnotType::Gate,
        &agent_gate,
        "evaluating",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(
        agent_meta.owner.as_ref().map(|o| &o.kind),
        Some(&OwnerKind::Agent)
    );
}

// ── Lease knot metadata ───────────────────────────────────

#[test]
fn lease_knot_step_metadata_for_active_state() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let meta = step_metadata_for_state(&registry, "lease", KnotType::Lease, &gate, "lease_ready")
        .expect("should resolve")
        .expect("should have metadata");
    assert_eq!(meta.action_state, "lease_active");

    let terminated = step_metadata_for_state(
        &registry,
        "lease",
        KnotType::Lease,
        &gate,
        "lease_terminated",
    )
    .expect("should resolve");
    assert!(terminated.is_none());
}

// ── Metadata persists through state transitions ───────────

#[test]
fn step_metadata_consistent_across_action_and_queue_pairs() {
    let registry = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    let queue_meta = step_metadata_for_state(
        &registry,
        "autopilot",
        KnotType::Work,
        &gate,
        "ready_for_planning",
    )
    .expect("should resolve")
    .expect("should have metadata");
    let action_meta =
        step_metadata_for_state(&registry, "autopilot", KnotType::Work, &gate, "planning")
            .expect("should resolve")
            .expect("should have metadata");
    assert_eq!(queue_meta.action_state, action_meta.action_state);
    assert_eq!(queue_meta.owner, action_meta.owner);
    assert_eq!(queue_meta.output, action_meta.output);
}

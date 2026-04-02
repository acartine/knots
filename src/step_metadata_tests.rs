use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::knot_type::KnotType;
use crate::installed_workflows::bundle_toml::parse_bundle_toml;
use crate::workflow::{OwnerKind, ProfileRegistry};
use crate::workflow_runtime::step_metadata_for_state;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
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
    let wf_root = workspace.join(".knots/workflows/review_flow/1");
    std::fs::create_dir_all(&wf_root).expect("dir should create");
    std::fs::write(wf_root.join("bundle.toml"), BUNDLE_WITH_REVIEW_HINT)
        .expect("bundle should write");
    std::fs::create_dir_all(workspace.join(".knots/workflows"))
        .expect("workflows dir should exist");
    std::fs::write(
        workspace.join(".knots/workflows/current"),
        "current_workflow = \"review_flow\"\ncurrent_version = 1\n",
    )
    .expect("config should write");

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
        "autopilot",
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
        "autopilot",
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
    let meta = step_metadata_for_state(
        &registry,
        "autopilot",
        KnotType::Lease,
        &gate,
        "lease_ready",
    )
    .expect("should resolve")
    .expect("should have metadata");
    assert_eq!(meta.action_state, "lease_active");

    let terminated = step_metadata_for_state(
        &registry,
        "autopilot",
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

const MULTI_OUTPUT_BUNDLE: &str = concat!(
    "[workflow]\nname = \"multi_output\"\nversion = 1\ndefault_profile = \"varied\"\n",
    "[states]\nready_for_code = { kind = \"queue\" }\nready_for_review = { kind = \"queue\" }\n",
    "ready_for_deploy = { kind = \"queue\" }\nready_for_signoff = { kind = \"queue\" }\n",
    "done = { kind = \"terminal\" }\nblocked = { kind = \"escape\" }\nabandoned = { kind = \"terminal\" }\n",
    "[states.code]\nkind = \"action\"\nexecutor = \"agent\"\nprompt = \"code\"\noutput = \"branch\"\noutput_hint = \"git log --oneline\"\n",
    "[states.review]\nkind = \"action\"\nexecutor = \"human\"\nprompt = \"review\"\noutput = \"pr\"\noutput_hint = \"gh pr view\"\n",
    "[states.deploy]\nkind = \"action\"\nexecutor = \"agent\"\nprompt = \"deploy\"\noutput = \"live_deployment\"\n",
    "[states.signoff]\nkind = \"action\"\nexecutor = \"human\"\nprompt = \"signoff\"\n",
    "[steps]\ncode_step = { queue = \"ready_for_code\", action = \"code\" }\n",
    "review_step = { queue = \"ready_for_review\", action = \"review\" }\n",
    "deploy_step = { queue = \"ready_for_deploy\", action = \"deploy\" }\n",
    "signoff_step = { queue = \"ready_for_signoff\", action = \"signoff\" }\n",
    "[phases]\ndevelop = { produce = \"code_step\", gate = \"review_step\" }\n",
    "release = { produce = \"deploy_step\", gate = \"signoff_step\" }\n",
    "[profiles.varied]\nphases = [\"develop\", \"release\"]\n",
    "[prompts.code]\naccept = [\"Code ready\"]\nbody = \"Write code.\"\n",
    "[prompts.code.success]\ncomplete = \"ready_for_review\"\n[prompts.code.failure]\nblocked = \"blocked\"\n",
    "[prompts.review]\naccept = [\"Reviewed\"]\nbody = \"Review.\"\n",
    "[prompts.review.success]\napproved = \"ready_for_deploy\"\n[prompts.review.failure]\nchanges = \"ready_for_code\"\n",
    "[prompts.deploy]\naccept = [\"Deployed\"]\nbody = \"Deploy.\"\n",
    "[prompts.deploy.success]\ncomplete = \"ready_for_signoff\"\n[prompts.deploy.failure]\nblocked = \"blocked\"\n",
    "[prompts.signoff]\naccept = [\"Signed off\"]\nbody = \"Sign off.\"\n",
    "[prompts.signoff.success]\napproved = \"done\"\n[prompts.signoff.failure]\nchanges = \"ready_for_deploy\"\n",
);

fn setup_multi_output_workspace(tag: &str) -> std::path::PathBuf {
    let ws = unique_workspace(tag);
    let root = ws.join(".knots/workflows/multi_output/1");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("bundle.toml"), MULTI_OUTPUT_BUNDLE).unwrap();
    std::fs::write(
        ws.join(".knots/workflows/current"),
        "current_workflow = \"multi_output\"\ncurrent_version = 1\n",
    )
    .unwrap();
    ws
}

fn assert_output(meta: &crate::profile::StepMetadata, ty: &str, hint: Option<&str>) {
    let out = meta.output.as_ref().expect("output should exist");
    assert_eq!(out.artifact_type, ty);
    assert_eq!(out.access_hint.as_deref(), hint);
}

#[test]
fn multi_output_bundle_resolves_distinct_action_outputs() {
    let ws = setup_multi_output_workspace("stepmeta-multi-out");
    let reg = ProfileRegistry::load_for_repo(&ws).unwrap();
    let (g, pid) = (GateData::default(), "multi_output/varied");
    let m = |s| {
        step_metadata_for_state(&reg, pid, KnotType::Work, &g, s)
            .unwrap()
            .unwrap()
    };
    let code = m("code");
    assert_eq!(code.action_state, "code");
    assert_output(&code, "branch", Some("git log --oneline"));
    let rev = m("review");
    assert_eq!(rev.action_state, "review");
    assert_output(&rev, "pr", Some("gh pr view"));
    let dep = m("deploy");
    assert_eq!(dep.action_state, "deploy");
    assert_output(&dep, "live_deployment", None);
    let sign = m("signoff");
    assert_eq!(sign.action_state, "signoff");
    assert_eq!(
        sign.output.as_ref().map(|o| o.artifact_type.as_str()),
        Some("")
    );
    let _ = std::fs::remove_dir_all(ws);
}

#[test]
fn multi_output_queue_states_resolve_through_action_metadata() {
    let ws = setup_multi_output_workspace("stepmeta-multi-q");
    let reg = ProfileRegistry::load_for_repo(&ws).unwrap();
    let (g, pid) = (GateData::default(), "multi_output/varied");
    let m = |s| step_metadata_for_state(&reg, pid, KnotType::Work, &g, s).unwrap();
    let q = m("ready_for_code").unwrap();
    assert_eq!(q.action_state, "code");
    assert_output(&q, "branch", Some("git log --oneline"));
    let d = m("ready_for_deploy").unwrap();
    assert_eq!(d.action_state, "deploy");
    assert_output(&d, "live_deployment", None);
    let done = m("done");
    assert!(done.is_none());
    let _ = std::fs::remove_dir_all(ws);
}

#[test]
fn legacy_profile_without_outputs_resolves_without_panic() {
    let reg = ProfileRegistry::load().expect("registry should load");
    let gate = GateData::default();
    for pid in ["autopilot", "semiauto", "autopilot_with_pr"] {
        let profile = reg.require(pid).expect("profile exists");
        for state in &profile.states {
            let r = step_metadata_for_state(&reg, pid, KnotType::Work, &gate, state);
            assert!(r.is_ok(), "{pid} state {state} panicked");
        }
    }
}

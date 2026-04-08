use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

use crate::artifact_target::ArtifactTarget;
use crate::installed_workflows;
use crate::profile::{
    normalize_profile_id, ActionOutputDef, GateMode, OutputMode, ProfileDefinition, ProfileError,
    ProfileOwners, WorkflowTransition, ABANDONED, BLOCKED, DEFERRED, IMPLEMENTATION,
    IMPLEMENTATION_REVIEW, PLANNING, PLAN_REVIEW, READY_FOR_IMPLEMENTATION,
    READY_FOR_IMPLEMENTATION_REVIEW, READY_FOR_PLANNING, READY_FOR_PLAN_REVIEW, READY_FOR_SHIPMENT,
    READY_FOR_SHIPMENT_REVIEW, SHIPMENT, SHIPMENT_REVIEW, SHIPPED,
};

const WILDCARD_STATE: &str = "*";
const ALL_STATES: [&str; 16] = [
    READY_FOR_PLANNING,
    PLANNING,
    READY_FOR_PLAN_REVIEW,
    PLAN_REVIEW,
    READY_FOR_IMPLEMENTATION,
    IMPLEMENTATION,
    READY_FOR_IMPLEMENTATION_REVIEW,
    IMPLEMENTATION_REVIEW,
    READY_FOR_SHIPMENT,
    SHIPMENT,
    READY_FOR_SHIPMENT_REVIEW,
    SHIPMENT_REVIEW,
    SHIPPED,
    BLOCKED,
    DEFERRED,
    ABANDONED,
];
const PLANNING_STATES: [&str; 4] = [
    READY_FOR_PLANNING,
    PLANNING,
    READY_FOR_PLAN_REVIEW,
    PLAN_REVIEW,
];
const IMPL_REVIEW_STATES: [&str; 2] = [READY_FOR_IMPLEMENTATION_REVIEW, IMPLEMENTATION_REVIEW];

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct RawProfileFile {
    #[serde(default)]
    pub(crate) profiles: Vec<RawProfileDefinition>,
}
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct RawProfileDefinition {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    pub(crate) planning_mode: GateMode,
    pub(crate) implementation_review_mode: GateMode,
    pub(crate) output: OutputMode,
    pub(crate) owners: ProfileOwners,
}

pub(crate) fn normalize(raw: RawProfileDefinition) -> Result<ProfileDefinition, ProfileError> {
    let id = normalize_profile_id(raw.id.as_str())
        .ok_or_else(|| ProfileError::InvalidDefinition("profile id is required".into()))?;
    let aliases: Vec<_> = legacy_aliases(&id)
        .iter()
        .map(|a| (*a).to_string())
        .collect();
    let states = compute_states(&raw);
    let state_set: HashSet<String> = states.iter().cloned().collect();
    let transitions = compute_transitions(&raw, &state_set);
    let initial = compute_initial_state(&raw);
    debug_assert!(state_set.contains(&initial));
    let action_states = compute_action_states(&states);
    let owners = build_owner_states(&raw.owners);
    Ok(ProfileDefinition {
        id,
        workflow_id: builtin_wf_id(),
        aliases,
        description: raw.description.and_then(|v| norm_scalar(&v)),
        planning_mode: raw.planning_mode,
        implementation_review_mode: raw.implementation_review_mode,
        outputs: legacy_outputs(&raw.output, &action_states),
        owners,
        initial_state: initial,
        queue_states: queue_states(&states),
        action_states,
        queue_actions: queue_actions(),
        action_kinds: action_kinds(),
        escape_states: vec![DEFERRED.to_string()],
        terminal_states: vec![SHIPPED.to_string(), ABANDONED.to_string()],
        transitions,
        states,
        action_prompts: BTreeMap::new(),
        prompt_acceptance: BTreeMap::new(),
        review_hints: BTreeMap::new(),
    })
}

fn compute_states(raw: &RawProfileDefinition) -> Vec<String> {
    let mut s: Vec<String> = ALL_STATES.iter().map(|v| (*v).to_string()).collect();
    if raw.planning_mode == GateMode::Skipped {
        s.retain(|x| !PLANNING_STATES.contains(&x.as_str()));
    }
    if raw.implementation_review_mode == GateMode::Skipped {
        s.retain(|x| !IMPL_REVIEW_STATES.contains(&x.as_str()));
    }
    s
}

fn compute_transitions(
    raw: &RawProfileDefinition,
    ss: &HashSet<String>,
) -> Vec<WorkflowTransition> {
    let mut t = canonical_transitions();
    if matches!(raw.planning_mode, GateMode::Optional | GateMode::Skipped) {
        t.push(wt(READY_FOR_PLANNING, READY_FOR_IMPLEMENTATION));
    }
    if matches!(
        raw.implementation_review_mode,
        GateMode::Optional | GateMode::Skipped
    ) {
        t.push(wt(IMPLEMENTATION, READY_FOR_SHIPMENT));
    }
    t.retain(|x| (x.from == WILDCARD_STATE || ss.contains(&x.from)) && ss.contains(&x.to));
    t.sort_by(|l, r| l.from.cmp(&r.from).then_with(|| l.to.cmp(&r.to)));
    t.dedup_by(|l, r| l.from == r.from && l.to == r.to);
    t
}

fn compute_initial_state(raw: &RawProfileDefinition) -> String {
    if raw.planning_mode == GateMode::Skipped {
        READY_FOR_IMPLEMENTATION.into()
    } else {
        READY_FOR_PLANNING.into()
    }
}
fn queue_states(s: &[String]) -> Vec<String> {
    s.iter()
        .filter(|x| x.starts_with("ready_for_"))
        .cloned()
        .collect()
}
fn compute_action_states(s: &[String]) -> Vec<String> {
    s.iter()
        .filter(|x| {
            matches!(
                x.as_str(),
                PLANNING
                    | PLAN_REVIEW
                    | IMPLEMENTATION
                    | IMPLEMENTATION_REVIEW
                    | SHIPMENT
                    | SHIPMENT_REVIEW
            )
        })
        .cloned()
        .collect()
}
fn queue_actions() -> BTreeMap<String, String> {
    BTreeMap::from([
        (READY_FOR_PLANNING.into(), PLANNING.into()),
        (READY_FOR_PLAN_REVIEW.into(), PLAN_REVIEW.into()),
        (READY_FOR_IMPLEMENTATION.into(), IMPLEMENTATION.into()),
        (
            READY_FOR_IMPLEMENTATION_REVIEW.into(),
            IMPLEMENTATION_REVIEW.into(),
        ),
        (READY_FOR_SHIPMENT.into(), SHIPMENT.into()),
        (READY_FOR_SHIPMENT_REVIEW.into(), SHIPMENT_REVIEW.into()),
    ])
}
fn action_kinds() -> BTreeMap<String, String> {
    BTreeMap::from([
        (PLANNING.into(), "produce".into()),
        (PLAN_REVIEW.into(), "gate".into()),
        (IMPLEMENTATION.into(), "produce".into()),
        (IMPLEMENTATION_REVIEW.into(), "gate".into()),
        (SHIPMENT.into(), "produce".into()),
        (SHIPMENT_REVIEW.into(), "gate".into()),
    ])
}
fn build_owner_states(o: &ProfileOwners) -> ProfileOwners {
    let mut m = BTreeMap::new();
    m.insert(READY_FOR_PLANNING.into(), o.planning.clone());
    m.insert(PLANNING.into(), o.planning.clone());
    m.insert(READY_FOR_PLAN_REVIEW.into(), o.plan_review.clone());
    m.insert(PLAN_REVIEW.into(), o.plan_review.clone());
    m.insert(READY_FOR_IMPLEMENTATION.into(), o.implementation.clone());
    m.insert(IMPLEMENTATION.into(), o.implementation.clone());
    m.insert(
        READY_FOR_IMPLEMENTATION_REVIEW.into(),
        o.implementation_review.clone(),
    );
    m.insert(
        IMPLEMENTATION_REVIEW.into(),
        o.implementation_review.clone(),
    );
    m.insert(READY_FOR_SHIPMENT.into(), o.shipment.clone());
    m.insert(SHIPMENT.into(), o.shipment.clone());
    m.insert(READY_FOR_SHIPMENT_REVIEW.into(), o.shipment_review.clone());
    m.insert(SHIPMENT_REVIEW.into(), o.shipment_review.clone());
    ProfileOwners {
        states: m,
        ..o.clone()
    }
}
fn legacy_outputs(
    mode: &OutputMode,
    action_states: &[String],
) -> BTreeMap<String, ActionOutputDef> {
    let target = match mode {
        OutputMode::Local => ArtifactTarget::Local,
        OutputMode::Remote => ArtifactTarget::Remote,
        OutputMode::Pr => ArtifactTarget::Pr,
        OutputMode::RemoteMain => ArtifactTarget::RemoteMain,
    };
    let at = target.as_str();
    debug_assert!(ArtifactTarget::from_str(at).is_ok());
    action_states
        .iter()
        .map(|s| {
            let def = ActionOutputDef {
                artifact_type: at.into(),
                access_hint: None,
            };
            (s.clone(), def)
        })
        .collect()
}
fn norm_scalar(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_ascii_lowercase())
    }
}
fn builtin_wf_id() -> String {
    installed_workflows::BUILTIN_WORKFLOW_ID.into()
}
fn canonical_transitions() -> Vec<WorkflowTransition> {
    vec![
        wt(READY_FOR_PLANNING, PLANNING),
        wt(PLANNING, READY_FOR_PLAN_REVIEW),
        wt(READY_FOR_PLAN_REVIEW, PLAN_REVIEW),
        wt(PLAN_REVIEW, READY_FOR_IMPLEMENTATION),
        wt(PLAN_REVIEW, READY_FOR_PLANNING),
        wt(READY_FOR_IMPLEMENTATION, IMPLEMENTATION),
        wt(IMPLEMENTATION, READY_FOR_IMPLEMENTATION_REVIEW),
        wt(READY_FOR_IMPLEMENTATION_REVIEW, IMPLEMENTATION_REVIEW),
        wt(IMPLEMENTATION_REVIEW, READY_FOR_SHIPMENT),
        wt(IMPLEMENTATION_REVIEW, READY_FOR_IMPLEMENTATION),
        wt(READY_FOR_SHIPMENT, SHIPMENT),
        wt(SHIPMENT, READY_FOR_SHIPMENT_REVIEW),
        wt(READY_FOR_SHIPMENT_REVIEW, SHIPMENT_REVIEW),
        wt(SHIPMENT_REVIEW, SHIPPED),
        wt(SHIPMENT_REVIEW, READY_FOR_IMPLEMENTATION),
        wt(SHIPMENT_REVIEW, READY_FOR_SHIPMENT),
        wt(WILDCARD_STATE, DEFERRED),
        wt(WILDCARD_STATE, ABANDONED),
    ]
}
fn wt(from: &str, to: &str) -> WorkflowTransition {
    WorkflowTransition {
        from: from.into(),
        to: to.into(),
    }
}
fn legacy_aliases(id: &str) -> &'static [&'static str] {
    match id {
        "autopilot" => &[
            "automation_granular",
            "default",
            "delivery",
            "automation",
            "granular",
        ],
        "semiauto" => &["human_gate", "human", "coarse", "pr_human_gate"],
        _ => &[],
    }
}

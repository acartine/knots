use std::collections::BTreeMap;
use std::str::FromStr;

use crate::artifact_target::ArtifactTarget;
use crate::profile::{
    normalize_profile_id, ActionOutputDef, GateMode, OwnerKind, ProfileDefinition, ProfileError,
    ProfileOwners, WorkflowTransition,
};

use super::bundle_json::{
    JsonOutputEntry, JsonPhaseSection, JsonProfileSection, JsonPromptSection, JsonStateSection,
    JsonStepSection,
};
use super::bundle_toml::default_owner;

pub(crate) struct BundleIndexes<'a> {
    pub states_by_id: BTreeMap<&'a str, &'a JsonStateSection>,
    pub steps_by_id: BTreeMap<&'a str, &'a JsonStepSection>,
    pub phases_by_id: BTreeMap<&'a str, &'a JsonPhaseSection>,
    pub prompts_by_name: BTreeMap<&'a str, &'a JsonPromptSection>,
}

impl<'a> BundleIndexes<'a> {
    pub fn build(
        states: &'a [JsonStateSection],
        steps: &'a [JsonStepSection],
        phases: &'a [JsonPhaseSection],
        prompts: &'a [JsonPromptSection],
    ) -> Self {
        Self {
            states_by_id: states.iter().map(|s| (s.id.as_str(), s)).collect(),
            steps_by_id: steps.iter().map(|s| (s.id.as_str(), s)).collect(),
            phases_by_id: phases.iter().map(|p| (p.id.as_str(), p)).collect(),
            prompts_by_name: prompts.iter().map(|p| (p.name.as_str(), p)).collect(),
        }
    }
}

pub(crate) fn build_json_profile(
    workflow_id: &str,
    profile: &JsonProfileSection,
    indexes: &BundleIndexes<'_>,
    all_states: &[JsonStateSection],
) -> Result<(ProfileDefinition, BTreeMap<String, String>), ProfileError> {
    let profile_id = normalize_profile_id(&profile.id)
        .ok_or_else(|| ProfileError::InvalidBundle("profile id is required".to_string()))?;
    let mut ctx = JsonProfileBuildContext::default();
    for phase_name in &profile.phases {
        process_json_phase(phase_name, profile, indexes, &mut ctx)?;
    }
    collect_json_terminal_escape(all_states, &mut ctx);
    assemble_json_profile(profile_id, workflow_id, profile, indexes, ctx)
}

fn assemble_json_profile(
    profile_id: String,
    workflow_id: &str,
    profile: &JsonProfileSection,
    indexes: &BundleIndexes<'_>,
    ctx: JsonProfileBuildContext,
) -> Result<(ProfileDefinition, BTreeMap<String, String>), ProfileError> {
    let action_prompts = ctx.action_prompts.clone();
    let owners = ProfileOwners {
        planning: default_owner(OwnerKind::Agent),
        plan_review: default_owner(OwnerKind::Human),
        implementation: default_owner(OwnerKind::Agent),
        implementation_review: default_owner(OwnerKind::Human),
        shipment: default_owner(OwnerKind::Agent),
        shipment_review: default_owner(OwnerKind::Human),
        states: ctx.owner_states,
    };
    let built = ProfileDefinition {
        id: profile_id,
        workflow_id: workflow_id.to_string(),
        aliases: Vec::new(),
        description: profile
            .description
            .clone()
            .or_else(|| profile.display_name.clone()),
        planning_mode: GateMode::Required,
        implementation_review_mode: GateMode::Required,
        outputs: build_outputs_from_json_profile(
            &profile.outputs,
            &ctx.action_states,
            &indexes.states_by_id,
        ),
        owners,
        initial_state: ctx.first_queue.ok_or_else(|| {
            ProfileError::InvalidBundle("profile has no initial queue state".to_string())
        })?,
        states: ctx.ordered_states,
        queue_states: ctx.queue_states,
        action_states: ctx.action_states,
        queue_actions: ctx.queue_actions,
        action_kinds: ctx.action_kinds,
        escape_states: ctx.escape_states,
        terminal_states: ctx.terminal_states,
        transitions: ctx.transitions,
        action_prompts: ctx.prompt_bodies,
        prompt_acceptance: ctx.prompt_acceptance,
        review_hints: ctx.review_hints,
    };
    Ok((built, action_prompts))
}

#[derive(Default)]
struct JsonProfileBuildContext {
    ordered_states: Vec<String>,
    queue_states: Vec<String>,
    action_states: Vec<String>,
    queue_actions: BTreeMap<String, String>,
    action_kinds: BTreeMap<String, String>,
    transitions: Vec<WorkflowTransition>,
    owner_states: BTreeMap<String, crate::profile::StepOwner>,
    prompt_bodies: BTreeMap<String, String>,
    prompt_acceptance: BTreeMap<String, Vec<String>>,
    action_prompts: BTreeMap<String, String>,
    review_hints: BTreeMap<String, String>,
    first_queue: Option<String>,
    terminal_states: Vec<String>,
    escape_states: Vec<String>,
}

fn process_json_phase(
    phase_name: &str,
    profile: &JsonProfileSection,
    indexes: &BundleIndexes<'_>,
    ctx: &mut JsonProfileBuildContext,
) -> Result<(), ProfileError> {
    let phase = indexes
        .phases_by_id
        .get(phase_name)
        .ok_or_else(|| ProfileError::InvalidBundle(format!("unknown phase '{}'", phase_name)))?;
    let mut step_names = vec![(&phase.produce_step, false)];
    if let Some(gate_step) = phase.gate_step.as_ref() {
        step_names.push((gate_step, true));
    }
    for (step_name, is_gate) in step_names {
        process_json_step(step_name, is_gate, profile, indexes, ctx)?;
    }
    Ok(())
}

fn process_json_step(
    step_name: &str,
    is_gate: bool,
    profile: &JsonProfileSection,
    indexes: &BundleIndexes<'_>,
    ctx: &mut JsonProfileBuildContext,
) -> Result<(), ProfileError> {
    let step = indexes
        .steps_by_id
        .get(step_name)
        .ok_or_else(|| ProfileError::InvalidBundle(format!("unknown step '{}'", step_name)))?;
    apply_json_step(step, is_gate, profile, ctx);
    if let Some(state) = indexes.states_by_id.get(step.action.as_str()) {
        collect_step_prompt(&step.action, state, indexes, ctx);
    }
    Ok(())
}

fn apply_json_step(
    step: &JsonStepSection,
    is_gate: bool,
    profile: &JsonProfileSection,
    ctx: &mut JsonProfileBuildContext,
) {
    let queue = step.queue.as_str();
    let action = step.action.as_str();
    super::push_unique(&mut ctx.ordered_states, queue.to_string());
    super::push_unique(&mut ctx.ordered_states, action.to_string());
    super::push_unique(&mut ctx.queue_states, queue.to_string());
    super::push_unique(&mut ctx.action_states, action.to_string());
    ctx.queue_actions
        .insert(queue.to_string(), action.to_string());
    let kind = if is_gate { "gate" } else { "produce" };
    ctx.action_kinds
        .insert(action.to_string(), kind.to_string());
    ctx.transitions.push(WorkflowTransition {
        from: queue.to_string(),
        to: action.to_string(),
    });
    let owner = default_owner(
        match profile
            .executors
            .get(action)
            .map(|v| v.as_str())
            .unwrap_or("agent")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "human" => OwnerKind::Human,
            _ => OwnerKind::Agent,
        },
    );
    ctx.owner_states.insert(queue.to_string(), owner.clone());
    ctx.owner_states.insert(action.to_string(), owner);
    ctx.first_queue.get_or_insert_with(|| queue.to_string());
}

fn collect_step_prompt(
    action_name: &str,
    state: &JsonStateSection,
    indexes: &BundleIndexes<'_>,
    ctx: &mut JsonProfileBuildContext,
) {
    if let Some(hint) = state.review_hint.as_ref() {
        ctx.review_hints
            .insert(action_name.to_string(), hint.clone());
    }
    let Some(prompt_name) = state.prompt.as_deref() else {
        return;
    };
    ctx.action_prompts
        .insert(action_name.to_string(), prompt_name.to_string());
    let Some(prompt) = indexes.prompts_by_name.get(prompt_name) else {
        return;
    };
    ctx.prompt_bodies
        .insert(action_name.to_string(), prompt.body.clone());
    ctx.prompt_acceptance
        .insert(action_name.to_string(), prompt.accept.clone());
    for outcome in &prompt.outcomes {
        ctx.transitions.push(WorkflowTransition {
            from: action_name.to_string(),
            to: outcome.target.clone(),
        });
        super::push_unique(&mut ctx.ordered_states, outcome.target.clone());
    }
}

fn collect_json_terminal_escape(
    all_states: &[JsonStateSection],
    ctx: &mut JsonProfileBuildContext,
) {
    for state in all_states {
        if state.kind == "terminal" {
            super::push_unique(&mut ctx.ordered_states, state.id.clone());
            super::push_unique(&mut ctx.terminal_states, state.id.clone());
            // "abandoned" gets a wildcard (reachable from any state)
            // but "shipped" does not (only reachable via normal flow)
            if state.id == "abandoned" {
                ctx.transitions.push(WorkflowTransition {
                    from: "*".to_string(),
                    to: state.id.clone(),
                });
            }
        } else if state.kind == "escape" {
            super::push_unique(&mut ctx.ordered_states, state.id.clone());
            super::push_unique(&mut ctx.escape_states, state.id.clone());
            ctx.transitions.push(WorkflowTransition {
                from: "*".to_string(),
                to: state.id.clone(),
            });
        }
    }
}

pub(crate) fn build_outputs_from_json_profile(
    profile_outputs: &BTreeMap<String, JsonOutputEntry>,
    action_states: &[String],
    states_by_id: &BTreeMap<&str, &JsonStateSection>,
) -> BTreeMap<String, ActionOutputDef> {
    let mut outputs = BTreeMap::new();
    for action in action_states {
        let def = if let Some(entry) = profile_outputs.get(action) {
            ActionOutputDef {
                artifact_type: entry.artifact_type.clone(),
                access_hint: entry.access_hint.clone(),
            }
        } else if let Some(state) = states_by_id.get(action.as_str()) {
            ActionOutputDef {
                artifact_type: state.output.clone().unwrap_or_default(),
                access_hint: state.output_hint.clone(),
            }
        } else {
            continue;
        };
        if !def.artifact_type.is_empty() {
            if let Err(e) = ArtifactTarget::from_str(&def.artifact_type) {
                eprintln!("warning: {action}: {e}");
            }
        }
        outputs.insert(action.clone(), def);
    }
    outputs
}

use std::collections::BTreeMap;
use std::str::FromStr;

use crate::artifact_target::ArtifactTarget;
use crate::profile::{
    normalize_profile_id, ActionOutputDef, GateMode, OwnerKind, ProfileDefinition, ProfileError,
    ProfileOwners, StepOwner, WorkflowTransition,
};

use super::bundle_toml::{
    default_owner, BundleOutputEntry, BundlePhaseSection, BundleProfileSection,
    BundlePromptSection, BundleStateSection, BundleStepSection,
};

pub(super) fn build_profile_definition(
    workflow_id: &str,
    profile_name: &str,
    profile_section: &BundleProfileSection,
    states: &BTreeMap<String, BundleStateSection>,
    steps: &BTreeMap<String, BundleStepSection>,
    phases: &BTreeMap<String, BundlePhaseSection>,
    prompts: &BTreeMap<String, BundlePromptSection>,
) -> Result<ProfileDefinition, ProfileError> {
    let id = normalize_profile_id(profile_name)
        .ok_or_else(|| ProfileError::InvalidBundle("profile id is required".to_string()))?;
    if profile_section.phases.is_empty() {
        return Err(ProfileError::InvalidBundle(format!(
            "profile '{}' must define at least one phase",
            profile_name
        )));
    }

    let mut ctx = ProfileBuildContext::default();
    for phase_name in &profile_section.phases {
        process_toml_phase(
            profile_name,
            phase_name,
            profile_section,
            states,
            steps,
            phases,
            &mut ctx,
        )?;
    }

    collect_terminal_and_escape_states(states, &mut ctx);
    collect_prompt_transitions(
        &ctx.action_states,
        states,
        prompts,
        &mut ctx.ordered_states,
        &mut ctx.transitions,
    )?;

    ctx.transitions.push(WorkflowTransition {
        from: "*".to_string(),
        to: "deferred".to_string(),
    });
    ctx.transitions.push(WorkflowTransition {
        from: "*".to_string(),
        to: "abandoned".to_string(),
    });

    assemble_profile(
        id,
        workflow_id,
        profile_name,
        profile_section,
        states,
        prompts,
        ctx,
    )
}

fn assemble_profile(
    id: String,
    workflow_id: &str,
    profile_name: &str,
    profile_section: &BundleProfileSection,
    states: &BTreeMap<String, BundleStateSection>,
    prompts: &BTreeMap<String, BundlePromptSection>,
    ctx: ProfileBuildContext,
) -> Result<ProfileDefinition, ProfileError> {
    let outputs =
        build_outputs_from_toml_profile(&profile_section.outputs, &ctx.action_states, states);
    let review_hints = build_review_hints(&ctx.action_states, states);
    let planning_mode = if has_phase_states(&ctx, "planning", "ready_for_planning")
        || has_phase_states(&ctx, "plan_review", "ready_for_plan_review")
    {
        GateMode::Required
    } else {
        GateMode::Skipped
    };
    let implementation_review_mode = if has_phase_states(
        &ctx,
        "implementation_review",
        "ready_for_implementation_review",
    ) {
        GateMode::Required
    } else {
        GateMode::Skipped
    };
    let owners = ProfileOwners::from_states(ctx.owner_states);
    let action_prompts = build_action_prompt_bodies(&ctx.action_states, states, prompts);
    let prompt_acceptance = build_prompt_acceptance(&ctx.action_states, states, prompts);

    Ok(ProfileDefinition {
        id,
        workflow_id: workflow_id.to_string(),
        aliases: Vec::new(),
        description: profile_section.description.clone(),
        planning_mode,
        implementation_review_mode,
        outputs,
        owners,
        initial_state: ctx.first_queue.ok_or_else(|| {
            ProfileError::InvalidBundle(format!(
                "profile '{}' could not determine \
                 an initial queue state",
                profile_name
            ))
        })?,
        states: ctx.ordered_states,
        queue_states: ctx.queue_states,
        action_states: ctx.action_states,
        queue_actions: ctx.queue_actions,
        action_kinds: ctx.action_kinds,
        escape_states: ctx.escape_states,
        terminal_states: ctx.terminal_states,
        transitions: ctx.transitions,
        action_prompts,
        prompt_acceptance,
        review_hints,
    })
}

fn has_phase_states(ctx: &ProfileBuildContext, action_state: &str, queue_state: &str) -> bool {
    ctx.action_states.iter().any(|state| state == action_state)
        || ctx.queue_states.iter().any(|state| state == queue_state)
}

#[derive(Default)]
struct ProfileBuildContext {
    ordered_states: Vec<String>,
    queue_states: Vec<String>,
    action_states: Vec<String>,
    queue_actions: BTreeMap<String, String>,
    action_kinds: BTreeMap<String, String>,
    transitions: Vec<WorkflowTransition>,
    owner_states: BTreeMap<String, StepOwner>,
    first_queue: Option<String>,
    terminal_states: Vec<String>,
    escape_states: Vec<String>,
}

fn process_toml_phase(
    profile_name: &str,
    phase_name: &str,
    profile_section: &BundleProfileSection,
    states: &BTreeMap<String, BundleStateSection>,
    steps: &BTreeMap<String, BundleStepSection>,
    phases: &BTreeMap<String, BundlePhaseSection>,
    ctx: &mut ProfileBuildContext,
) -> Result<(), ProfileError> {
    let phase = phases.get(phase_name).ok_or_else(|| {
        ProfileError::InvalidBundle(format!(
            "profile '{}' references unknown phase '{}'",
            profile_name, phase_name
        ))
    })?;
    let mut step_names = vec![(&phase.produce, false)];
    if let Some(gate_step) = phase.gate.as_ref() {
        step_names.push((gate_step, true));
    }
    for (step_name, is_gate) in step_names {
        process_toml_step(
            profile_name,
            step_name,
            is_gate,
            profile_section,
            states,
            steps,
            ctx,
        )?;
    }
    Ok(())
}

fn process_toml_step(
    profile_name: &str,
    step_name: &str,
    is_gate: bool,
    profile_section: &BundleProfileSection,
    states: &BTreeMap<String, BundleStateSection>,
    steps: &BTreeMap<String, BundleStepSection>,
    ctx: &mut ProfileBuildContext,
) -> Result<(), ProfileError> {
    let step = steps.get(step_name).ok_or_else(|| {
        ProfileError::InvalidBundle(format!(
            "profile '{}' references unknown step '{}'",
            profile_name, step_name
        ))
    })?;
    validate_step_states(step_name, step, states)?;
    apply_step_to_context(step, is_gate, profile_name, profile_section, states, ctx)?;
    Ok(())
}

fn validate_step_states(
    step_name: &str,
    step: &BundleStepSection,
    states: &BTreeMap<String, BundleStateSection>,
) -> Result<(), ProfileError> {
    let queue_state = states.get(&step.queue).ok_or_else(|| {
        ProfileError::InvalidBundle(format!(
            "step '{}' references unknown queue state '{}'",
            step_name, step.queue
        ))
    })?;
    let action_state = states.get(&step.action).ok_or_else(|| {
        ProfileError::InvalidBundle(format!(
            "step '{}' references unknown action state '{}'",
            step_name, step.action
        ))
    })?;
    if queue_state.kind != "queue" {
        return Err(ProfileError::InvalidBundle(format!(
            "state '{}' must be a queue state",
            step.queue
        )));
    }
    if action_state.kind != "action" {
        return Err(ProfileError::InvalidBundle(format!(
            "state '{}' must be an action state",
            step.action
        )));
    }
    Ok(())
}

fn apply_step_to_context(
    step: &BundleStepSection,
    is_gate: bool,
    profile_name: &str,
    profile_section: &BundleProfileSection,
    states: &BTreeMap<String, BundleStateSection>,
    ctx: &mut ProfileBuildContext,
) -> Result<(), ProfileError> {
    super::push_unique(&mut ctx.ordered_states, step.queue.clone());
    super::push_unique(&mut ctx.ordered_states, step.action.clone());
    super::push_unique(&mut ctx.queue_states, step.queue.clone());
    super::push_unique(&mut ctx.action_states, step.action.clone());
    ctx.queue_actions
        .insert(step.queue.clone(), step.action.clone());
    let kind = if is_gate { "gate" } else { "produce" };
    ctx.action_kinds
        .insert(step.action.clone(), kind.to_string());
    ctx.transitions.push(WorkflowTransition {
        from: step.queue.clone(),
        to: step.action.clone(),
    });
    let action_state = states.get(&step.action).ok_or_else(|| {
        ProfileError::InvalidBundle(format!("missing action state '{}'", step.action))
    })?;
    let owner = owner_for_action_state(action_state, profile_section, profile_name, &step.action)?;
    ctx.owner_states.insert(step.queue.clone(), owner.clone());
    ctx.owner_states.insert(step.action.clone(), owner);
    if ctx.first_queue.is_none() {
        ctx.first_queue = Some(step.queue.clone());
    }
    Ok(())
}

fn collect_terminal_and_escape_states(
    states: &BTreeMap<String, BundleStateSection>,
    ctx: &mut ProfileBuildContext,
) {
    for (name, state) in states {
        if state.kind == "terminal" {
            super::push_unique(&mut ctx.ordered_states, name.clone());
            super::push_unique(&mut ctx.terminal_states, name.clone());
        } else if state.kind == "escape" {
            super::push_unique(&mut ctx.ordered_states, name.clone());
            super::push_unique(&mut ctx.escape_states, name.clone());
        }
    }
}

fn collect_prompt_transitions(
    action_states: &[String],
    states: &BTreeMap<String, BundleStateSection>,
    prompts: &BTreeMap<String, BundlePromptSection>,
    ordered_states: &mut Vec<String>,
    transitions: &mut Vec<WorkflowTransition>,
) -> Result<(), ProfileError> {
    for action_state in action_states {
        add_prompt_transitions_for_action(
            action_state,
            states,
            prompts,
            ordered_states,
            transitions,
        )?;
    }
    Ok(())
}

fn add_prompt_transitions_for_action(
    action_state: &str,
    states: &BTreeMap<String, BundleStateSection>,
    prompts: &BTreeMap<String, BundlePromptSection>,
    ordered_states: &mut Vec<String>,
    transitions: &mut Vec<WorkflowTransition>,
) -> Result<(), ProfileError> {
    let state = states.get(action_state).ok_or_else(|| {
        ProfileError::InvalidBundle(format!("missing action state '{}'", action_state))
    })?;
    let prompt_name = state.prompt.as_ref().ok_or_else(|| {
        ProfileError::InvalidBundle(format!("action '{}' is missing prompt", action_state))
    })?;
    let prompt = prompts.get(prompt_name).ok_or_else(|| {
        ProfileError::InvalidBundle(format!(
            "action '{}' references unknown prompt '{}'",
            action_state, prompt_name
        ))
    })?;
    let Some(success_target) = prompt.success.values().next() else {
        return Err(ProfileError::InvalidBundle(format!(
            "prompt '{}' must define one success target",
            prompt_name
        )));
    };
    transitions.push(WorkflowTransition {
        from: action_state.to_string(),
        to: success_target.clone(),
    });
    super::push_unique(ordered_states, success_target.clone());
    for target in prompt.failure.values() {
        transitions.push(WorkflowTransition {
            from: action_state.to_string(),
            to: target.clone(),
        });
        super::push_unique(ordered_states, target.clone());
    }
    Ok(())
}

fn owner_for_action_state(
    state: &BundleStateSection,
    profile: &BundleProfileSection,
    profile_name: &str,
    action_state: &str,
) -> Result<StepOwner, ProfileError> {
    let raw_executor = profile
        .overrides
        .get(action_state)
        .or(state.executor.as_ref())
        .map(|value| value.trim().to_ascii_lowercase())
        .ok_or_else(|| {
            ProfileError::InvalidBundle(format!(
                "profile '{}' action '{}' is missing executor",
                profile_name, action_state
            ))
        })?;
    let kind = match raw_executor.as_str() {
        "human" => OwnerKind::Human,
        "agent" => OwnerKind::Agent,
        other => {
            return Err(ProfileError::InvalidBundle(format!(
                "profile '{}' action '{}' has invalid executor '{}'",
                profile_name, action_state, other
            )));
        }
    };
    Ok(default_owner(kind))
}

pub(crate) fn build_outputs_from_toml_profile(
    profile_outputs: &BTreeMap<String, BundleOutputEntry>,
    action_states: &[String],
    states: &BTreeMap<String, BundleStateSection>,
) -> BTreeMap<String, ActionOutputDef> {
    let mut outputs = BTreeMap::new();
    for action in action_states {
        let def = if let Some(entry) = profile_outputs.get(action) {
            ActionOutputDef {
                artifact_type: entry.artifact_type.clone(),
                access_hint: entry.access_hint.clone(),
            }
        } else if let Some(state) = states.get(action) {
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

fn build_action_prompt_bodies(
    action_states: &[String],
    states: &BTreeMap<String, BundleStateSection>,
    prompts: &BTreeMap<String, BundlePromptSection>,
) -> BTreeMap<String, String> {
    action_states
        .iter()
        .filter_map(|state| {
            let prompt_name = states.get(state).and_then(|def| def.prompt.as_ref())?;
            let definition = prompts.get(prompt_name)?;
            Some((state.clone(), definition.body.clone()))
        })
        .collect()
}

fn build_prompt_acceptance(
    action_states: &[String],
    states: &BTreeMap<String, BundleStateSection>,
    prompts: &BTreeMap<String, BundlePromptSection>,
) -> BTreeMap<String, Vec<String>> {
    action_states
        .iter()
        .filter_map(|state| {
            let prompt = states.get(state).and_then(|def| def.prompt.as_ref())?;
            let definition = prompts.get(prompt)?;
            Some((state.clone(), definition.accept.clone()))
        })
        .collect()
}

fn build_review_hints(
    action_states: &[String],
    states: &BTreeMap<String, BundleStateSection>,
) -> BTreeMap<String, String> {
    action_states
        .iter()
        .filter_map(|state| {
            let hint = states.get(state)?.review_hint.as_ref()?;
            Some((state.clone(), hint.clone()))
        })
        .collect()
}

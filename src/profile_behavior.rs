use crate::profile::{
    InvalidWorkflowTransition, OwnerKind, ProfileDefinition, ProfileError, ProfileOwners,
    StepMetadata, StepOwner, IMPLEMENTATION, IMPLEMENTATION_REVIEW, PLANNING, PLAN_REVIEW,
    READY_FOR_IMPLEMENTATION, READY_FOR_IMPLEMENTATION_REVIEW, READY_FOR_PLANNING,
    READY_FOR_PLAN_REVIEW, READY_FOR_SHIPMENT, READY_FOR_SHIPMENT_REVIEW, SHIPMENT,
    SHIPMENT_REVIEW,
};

const WILDCARD_STATE: &str = "*";

impl ProfileOwners {
    pub fn for_action_state(&self, state: &str) -> Option<&StepOwner> {
        match state {
            PLANNING => Some(&self.planning),
            PLAN_REVIEW => Some(&self.plan_review),
            IMPLEMENTATION => Some(&self.implementation),
            IMPLEMENTATION_REVIEW => Some(&self.implementation_review),
            SHIPMENT => Some(&self.shipment),
            SHIPMENT_REVIEW => Some(&self.shipment_review),
            _ => None,
        }
    }

    pub fn owner_kind_for_state(&self, state: &str) -> Option<&OwnerKind> {
        if let Some(owner) = self.states.get(state) {
            return Some(&owner.kind);
        }
        let action = match state {
            READY_FOR_PLANNING | PLANNING => PLANNING,
            READY_FOR_PLAN_REVIEW | PLAN_REVIEW => PLAN_REVIEW,
            READY_FOR_IMPLEMENTATION | IMPLEMENTATION => IMPLEMENTATION,
            READY_FOR_IMPLEMENTATION_REVIEW | IMPLEMENTATION_REVIEW => IMPLEMENTATION_REVIEW,
            READY_FOR_SHIPMENT | SHIPMENT => SHIPMENT,
            READY_FOR_SHIPMENT_REVIEW | SHIPMENT_REVIEW => SHIPMENT_REVIEW,
            _ => return None,
        };
        self.for_action_state(action).map(|o| &o.kind)
    }
}

impl ProfileDefinition {
    pub fn is_queue_state(&self, state: &str) -> bool {
        if !self.queue_states.is_empty() {
            return self.queue_states.iter().any(|candidate| candidate == state);
        }
        state.starts_with("ready_for_") || state == "ready_to_evaluate"
    }

    #[allow(dead_code)]
    pub fn is_action_state(&self, state: &str) -> bool {
        if self.is_escape_state(state) {
            return false;
        }
        if !self.action_states.is_empty() {
            return self
                .action_states
                .iter()
                .any(|candidate| candidate == state);
        }
        self.owners.for_action_state(state).is_some() || state == "evaluating"
    }

    pub fn action_for_queue_state(&self, state: &str) -> Option<&str> {
        self.queue_actions.get(state).map(String::as_str)
    }

    pub fn is_gate_action_state(&self, state: &str) -> bool {
        matches!(
            self.action_kinds.get(state).map(String::as_str),
            Some("gate") | Some("review")
        )
    }

    pub fn is_escape_state(&self, state: &str) -> bool {
        let s = normalize_state_alias(state);
        self.escape_states.iter().any(|c| c == s)
    }

    pub fn prompt_for_action_state(&self, s: &str) -> Option<&str> {
        self.action_prompts.get(s).map(String::as_str)
    }

    pub fn acceptance_for_action_state(&self, s: &str) -> &[String] {
        self.prompt_acceptance
            .get(s)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn step_metadata_for(&self, action_state: &str) -> StepMetadata {
        let owner = self
            .owners
            .states
            .get(action_state)
            .or_else(|| self.owners.for_action_state(action_state))
            .cloned();
        StepMetadata {
            action_state: action_state.to_string(),
            action_kind: self.action_kinds.get(action_state).cloned(),
            owner,
            output: self.outputs.get(action_state).cloned(),
            review_hint: self.review_hints.get(action_state).cloned(),
        }
    }

    pub fn is_terminal_state(&self, state: &str) -> bool {
        self.terminal_states.iter().any(|c| c == state)
    }

    pub fn require_state(&self, state: &str) -> Result<(), ProfileError> {
        let normalized = normalize_state_alias(state);
        if self.states.iter().any(|candidate| candidate == normalized) {
            return Ok(());
        }
        Err(ProfileError::UnknownState {
            profile_id: self.id.clone(),
            state: normalized.to_string(),
        })
    }

    pub fn validate_transition(
        &self,
        from: &str,
        to: &str,
        force: bool,
    ) -> Result<(), ProfileError> {
        let from = normalize_state_alias(from);
        let to = normalize_state_alias(to);
        self.require_state(from)?;
        self.require_state(to)?;

        if force || from == to {
            return Ok(());
        }

        let allowed = self.transitions.iter().any(|transition| {
            (transition.from == from || transition.from == WILDCARD_STATE) && transition.to == to
        });
        if allowed {
            return Ok(());
        }

        Err(InvalidWorkflowTransition {
            profile_id: self.id.clone(),
            from: from.to_string(),
            to: to.to_string(),
        }
        .into())
    }

    pub fn next_happy_path_state(&self, current: &str) -> Option<&str> {
        let current = normalize_state_alias(current);
        let pos = self.states.iter().position(|state| state == current)?;
        for candidate in &self.states[pos + 1..] {
            let valid = self
                .transitions
                .iter()
                .any(|transition| transition.from == current && transition.to == *candidate);
            if valid {
                return Some(candidate.as_str());
            }
        }
        None
    }
}

fn normalize_state_alias(raw: &str) -> &str {
    match raw.trim() {
        "idea" => READY_FOR_PLANNING,
        "work_item" => READY_FOR_IMPLEMENTATION,
        "implementing" => IMPLEMENTATION,
        "implemented" => READY_FOR_IMPLEMENTATION_REVIEW,
        "reviewing" => IMPLEMENTATION_REVIEW,
        "rejected" | "refining" => READY_FOR_IMPLEMENTATION,
        "approved" => READY_FOR_SHIPMENT,
        other => other,
    }
}

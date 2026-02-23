use std::error::Error;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnotState {
    Idea,
    WorkItem,
    Implementing,
    Implemented,
    Reviewing,
    Rejected,
    Refining,
    Approved,
    Shipped,
    Deferred,
    Abandoned,
}

impl KnotState {
    pub const ALL: [KnotState; 11] = [
        KnotState::Idea,
        KnotState::WorkItem,
        KnotState::Implementing,
        KnotState::Implemented,
        KnotState::Reviewing,
        KnotState::Rejected,
        KnotState::Refining,
        KnotState::Approved,
        KnotState::Shipped,
        KnotState::Deferred,
        KnotState::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            KnotState::Idea => "idea",
            KnotState::WorkItem => "work_item",
            KnotState::Implementing => "implementing",
            KnotState::Implemented => "implemented",
            KnotState::Reviewing => "reviewing",
            KnotState::Rejected => "rejected",
            KnotState::Refining => "refining",
            KnotState::Approved => "approved",
            KnotState::Shipped => "shipped",
            KnotState::Deferred => "deferred",
            KnotState::Abandoned => "abandoned",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            KnotState::Shipped | KnotState::Deferred | KnotState::Abandoned
        )
    }

    pub fn can_transition_to(self, next: KnotState) -> bool {
        if self == next {
            return true;
        }

        if matches!(next, KnotState::Deferred | KnotState::Abandoned) {
            return true;
        }

        match (self, next) {
            (KnotState::Idea, KnotState::WorkItem) => true,
            (KnotState::WorkItem, KnotState::Implementing) => true,
            (KnotState::Implementing, KnotState::Implemented) => true,
            (KnotState::Implemented, KnotState::Reviewing) => true,
            (KnotState::Reviewing, KnotState::Approved | KnotState::Rejected) => true,
            (KnotState::Rejected, KnotState::Refining) => true,
            (KnotState::Refining, KnotState::Implemented) => true,
            (KnotState::Approved, KnotState::Shipped) => true,
            _ => false,
        }
    }

    pub fn validate_transition(
        self,
        next: KnotState,
        force: bool,
    ) -> Result<(), InvalidStateTransition> {
        if force || self.can_transition_to(next) {
            return Ok(());
        }

        Err(InvalidStateTransition {
            from: self,
            to: next,
        })
    }
}

impl fmt::Display for KnotState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KnotState {
    type Err = ParseKnotStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();
        let state = match normalized.as_str() {
            "idea" => KnotState::Idea,
            "work_item" | "work-item" => KnotState::WorkItem,
            "implementing" => KnotState::Implementing,
            "implemented" => KnotState::Implemented,
            "reviewing" => KnotState::Reviewing,
            "rejected" => KnotState::Rejected,
            "refining" => KnotState::Refining,
            "approved" => KnotState::Approved,
            "shipped" => KnotState::Shipped,
            "deferred" => KnotState::Deferred,
            "abandoned" => KnotState::Abandoned,
            _ => {
                return Err(ParseKnotStateError {
                    value: value.to_string(),
                });
            }
        };

        Ok(state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKnotStateError {
    value: String,
}

impl fmt::Display for ParseKnotStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid knot state '{}': expected one of {}",
            self.value,
            KnotState::ALL
                .iter()
                .map(|state| state.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Error for ParseKnotStateError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidStateTransition {
    pub from: KnotState,
    pub to: KnotState,
}

impl fmt::Display for InvalidStateTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid state transition: {} -> {}", self.from, self.to)
    }
}

impl Error for InvalidStateTransition {}

#[cfg(test)]
mod tests {
    use super::KnotState;
    use std::str::FromStr;

    #[test]
    fn parses_supported_state_names() {
        assert_eq!(KnotState::from_str("idea").unwrap(), KnotState::Idea);
        assert_eq!(
            KnotState::from_str("work_item").unwrap(),
            KnotState::WorkItem
        );
        assert_eq!(
            KnotState::from_str("work-item").unwrap(),
            KnotState::WorkItem
        );
    }

    #[test]
    fn accepts_common_path_transitions() {
        let transitions = [
            (KnotState::Idea, KnotState::WorkItem),
            (KnotState::WorkItem, KnotState::Implementing),
            (KnotState::Implementing, KnotState::Implemented),
            (KnotState::Implemented, KnotState::Reviewing),
            (KnotState::Reviewing, KnotState::Approved),
            (KnotState::Reviewing, KnotState::Rejected),
            (KnotState::Rejected, KnotState::Refining),
            (KnotState::Refining, KnotState::Implemented),
            (KnotState::Approved, KnotState::Shipped),
        ];

        for (from, to) in transitions {
            assert!(from.validate_transition(to, false).is_ok());
        }
    }

    #[test]
    fn accepts_deferred_and_abandoned_from_all_states() {
        for from in KnotState::ALL {
            assert!(from.validate_transition(KnotState::Deferred, false).is_ok());
            assert!(from
                .validate_transition(KnotState::Abandoned, false)
                .is_ok());
        }
    }

    #[test]
    fn rejects_unlisted_transition_without_force() {
        let result = KnotState::Implementing.validate_transition(KnotState::Reviewing, false);
        assert!(result.is_err());
    }

    #[test]
    fn allows_unlisted_transition_with_force() {
        let result = KnotState::Implementing.validate_transition(KnotState::Reviewing, true);
        assert!(result.is_ok());
    }

    #[test]
    fn marks_terminal_states_for_tiering() {
        assert!(KnotState::Shipped.is_terminal());
        assert!(KnotState::Deferred.is_terminal());
        assert!(KnotState::Abandoned.is_terminal());
        assert!(!KnotState::Implementing.is_terminal());
    }
}

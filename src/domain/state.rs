use std::error::Error;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnotState {
    ReadyForPlanning,
    Planning,
    ReadyForPlanReview,
    PlanReview,
    ReadyForImplementation,
    Implementation,
    ReadyForImplementationReview,
    ImplementationReview,
    ReadyForShipment,
    Shipment,
    ReadyForShipmentReview,
    ShipmentReview,
    Shipped,
    Deferred,
    Abandoned,
}

impl KnotState {
    pub const ALL: [KnotState; 15] = [
        KnotState::ReadyForPlanning,
        KnotState::Planning,
        KnotState::ReadyForPlanReview,
        KnotState::PlanReview,
        KnotState::ReadyForImplementation,
        KnotState::Implementation,
        KnotState::ReadyForImplementationReview,
        KnotState::ImplementationReview,
        KnotState::ReadyForShipment,
        KnotState::Shipment,
        KnotState::ReadyForShipmentReview,
        KnotState::ShipmentReview,
        KnotState::Shipped,
        KnotState::Deferred,
        KnotState::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            KnotState::ReadyForPlanning => "ready_for_planning",
            KnotState::Planning => "planning",
            KnotState::ReadyForPlanReview => "ready_for_plan_review",
            KnotState::PlanReview => "plan_review",
            KnotState::ReadyForImplementation => "ready_for_implementation",
            KnotState::Implementation => "implementation",
            KnotState::ReadyForImplementationReview => "ready_for_implementation_review",
            KnotState::ImplementationReview => "implementation_review",
            KnotState::ReadyForShipment => "ready_for_shipment",
            KnotState::Shipment => "shipment",
            KnotState::ReadyForShipmentReview => "ready_for_shipment_review",
            KnotState::ShipmentReview => "shipment_review",
            KnotState::Shipped => "shipped",
            KnotState::Deferred => "deferred",
            KnotState::Abandoned => "abandoned",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, KnotState::Shipped | KnotState::Abandoned)
    }

    #[allow(dead_code)]
    pub fn can_transition_to(self, next: KnotState) -> bool {
        if self == next {
            return true;
        }

        if next == KnotState::Abandoned || next == KnotState::Deferred {
            return true;
        }

        matches!(
            (self, next),
            (KnotState::ReadyForPlanning, KnotState::Planning)
                | (KnotState::Planning, KnotState::ReadyForPlanReview)
                | (KnotState::ReadyForPlanReview, KnotState::PlanReview)
                | (KnotState::PlanReview, KnotState::ReadyForImplementation)
                | (KnotState::PlanReview, KnotState::ReadyForPlanning)
                | (KnotState::ReadyForImplementation, KnotState::Implementation)
                | (
                    KnotState::Implementation,
                    KnotState::ReadyForImplementationReview
                )
                | (
                    KnotState::ReadyForImplementationReview,
                    KnotState::ImplementationReview
                )
                | (KnotState::ImplementationReview, KnotState::ReadyForShipment)
                | (
                    KnotState::ImplementationReview,
                    KnotState::ReadyForImplementation
                )
                | (KnotState::ReadyForShipment, KnotState::Shipment)
                | (KnotState::Shipment, KnotState::ReadyForShipmentReview)
                | (KnotState::ReadyForShipmentReview, KnotState::ShipmentReview)
                | (KnotState::ShipmentReview, KnotState::Shipped)
                | (KnotState::ShipmentReview, KnotState::ReadyForImplementation)
                | (KnotState::ShipmentReview, KnotState::ReadyForShipment)
        )
    }

    #[allow(dead_code)]
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
        let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
        let state = match normalized.as_str() {
            "ready_for_planning" | "idea" => KnotState::ReadyForPlanning,
            "planning" => KnotState::Planning,
            "ready_for_plan_review" => KnotState::ReadyForPlanReview,
            "plan_review" => KnotState::PlanReview,
            "ready_for_implementation" | "work_item" | "rejected" | "refining" => {
                KnotState::ReadyForImplementation
            }
            "implementation" | "implementing" => KnotState::Implementation,
            "ready_for_implementation_review" | "implemented" => {
                KnotState::ReadyForImplementationReview
            }
            "implementation_review" | "reviewing" => KnotState::ImplementationReview,
            "ready_for_shipment" | "approved" => KnotState::ReadyForShipment,
            "shipment" | "shipping" => KnotState::Shipment,
            "ready_for_shipment_review" => KnotState::ReadyForShipmentReview,
            "shipment_review" => KnotState::ShipmentReview,
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
    fn parses_supported_state_names_and_legacy_aliases() {
        assert_eq!(
            KnotState::from_str("ready_for_planning").unwrap(),
            KnotState::ReadyForPlanning
        );
        assert_eq!(
            KnotState::from_str("idea").unwrap(),
            KnotState::ReadyForPlanning
        );
        assert_eq!(
            KnotState::from_str("work_item").unwrap(),
            KnotState::ReadyForImplementation
        );
    }

    #[test]
    fn accepts_common_path_transitions() {
        let transitions = [
            (KnotState::ReadyForPlanning, KnotState::Planning),
            (KnotState::Planning, KnotState::ReadyForPlanReview),
            (KnotState::ReadyForPlanReview, KnotState::PlanReview),
            (KnotState::PlanReview, KnotState::ReadyForImplementation),
            (KnotState::ReadyForImplementation, KnotState::Implementation),
            (
                KnotState::Implementation,
                KnotState::ReadyForImplementationReview,
            ),
            (
                KnotState::ReadyForImplementationReview,
                KnotState::ImplementationReview,
            ),
            (KnotState::ImplementationReview, KnotState::ReadyForShipment),
            (KnotState::ReadyForShipment, KnotState::Shipment),
            (KnotState::Shipment, KnotState::ReadyForShipmentReview),
            (KnotState::ReadyForShipmentReview, KnotState::ShipmentReview),
            (KnotState::ShipmentReview, KnotState::Shipped),
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
        let result =
            KnotState::Implementation.validate_transition(KnotState::ImplementationReview, false);
        assert!(result.is_err());
    }

    #[test]
    fn allows_unlisted_transition_with_force() {
        let result =
            KnotState::Implementation.validate_transition(KnotState::ImplementationReview, true);
        assert!(result.is_ok());
    }

    #[test]
    fn marks_terminal_states_for_tiering() {
        assert!(KnotState::Shipped.is_terminal());
        assert!(KnotState::Abandoned.is_terminal());
        assert!(!KnotState::Deferred.is_terminal());
    }

    #[test]
    fn parses_implemented_alias() {
        assert_eq!(
            KnotState::from_str("implemented").unwrap(),
            KnotState::ReadyForImplementationReview
        );
    }
}

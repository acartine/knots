use super::ManagedSkill;

const EVALUATING: &str = include_str!("../skills/evaluating.md");
const IMPLEMENTATION: &str = include_str!("../skills/implementation.md");
const IMPLEMENTATION_REVIEW: &str = include_str!("../skills/implementation_review.md");
const PLAN_REVIEW: &str = include_str!("../skills/plan_review.md");
const PLANNING: &str = include_str!("../skills/planning.md");
const SHIPMENT: &str = include_str!("../skills/shipment.md");
const SHIPMENT_REVIEW: &str = include_str!("../skills/shipment_review.md");

pub(super) fn managed_skills() -> &'static [ManagedSkill] {
    &[
        ManagedSkill {
            deploy_name: "planning",
            title: "planning",
            body: PLANNING,
        },
        ManagedSkill {
            deploy_name: "plan-review",
            title: "plan review",
            body: PLAN_REVIEW,
        },
        ManagedSkill {
            deploy_name: "implementation",
            title: "implementation",
            body: IMPLEMENTATION,
        },
        ManagedSkill {
            deploy_name: "implementation-review",
            title: "implementation review",
            body: IMPLEMENTATION_REVIEW,
        },
        ManagedSkill {
            deploy_name: "shipment",
            title: "shipment",
            body: SHIPMENT,
        },
        ManagedSkill {
            deploy_name: "shipment-review",
            title: "shipment review",
            body: SHIPMENT_REVIEW,
        },
        ManagedSkill {
            deploy_name: "evaluating",
            title: "evaluating",
            body: EVALUATING,
        },
    ]
}

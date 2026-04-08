use super::ManagedSkill;

const KNOTS: &str = include_str!("../skills/knots.md");
const KNOTS_E2E: &str = include_str!("../skills/knots_e2e.md");
const KNOTS_CREATE: &str = include_str!("../skills/knots_create.md");

pub(super) fn managed_skills() -> &'static [ManagedSkill] {
    &[
        ManagedSkill {
            deploy_name: "knots",
            contents: KNOTS,
        },
        ManagedSkill {
            deploy_name: "knots-e2e",
            contents: KNOTS_E2E,
        },
        ManagedSkill {
            deploy_name: "knots-create",
            contents: KNOTS_CREATE,
        },
    ]
}

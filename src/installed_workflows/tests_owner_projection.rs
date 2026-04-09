use super::bundle_json::parse_bundle_json;
use super::bundle_toml::{parse_bundle_toml, render_json_bundle_from_toml};
use crate::profile::OwnerKind;

const WORK_SDLC_LIKE_BUNDLE: &str = r#"
[workflow]
name = "work_like"
version = 1
default_profile = "autopilot"

[states.ready_for_planning]
kind = "queue"

[states.planning]
kind = "action"
executor = "agent"
prompt = "planning"
output = "branch"

[states.ready_for_plan_review]
kind = "queue"

[states.plan_review]
kind = "action"
action_type = "gate"
executor = "agent"
prompt = "plan_review"
output = "note"

[states.shipped]
kind = "terminal"

[states.deferred]
kind = "escape"

[states.abandoned]
kind = "terminal"

[steps.planning_step]
queue = "ready_for_planning"
action = "planning"

[steps.plan_review_step]
queue = "ready_for_plan_review"
action = "plan_review"

[phases.main]
produce = "planning_step"
gate = "plan_review_step"

[profiles.autopilot]
phases = ["main"]

[prompts.planning]
accept = ["Plan recorded"]
body = "Plan it."

[prompts.planning.success]
complete = "ready_for_plan_review"

[prompts.plan_review]
accept = ["Plan approved"]
body = "Review it."

[prompts.plan_review.success]
approve = "shipped"
"#;

#[test]
fn work_sdlc_like_review_owner_stays_agent_in_toml_and_json() {
    let workflow = parse_bundle_toml(WORK_SDLC_LIKE_BUNDLE).expect("bundle should parse");
    let profile = workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    assert_eq!(
        profile.owners.owner_kind_for_state("ready_for_plan_review"),
        Some(&OwnerKind::Agent)
    );

    let rendered =
        render_json_bundle_from_toml(WORK_SDLC_LIKE_BUNDLE).expect("json render should work");
    let json_workflow = parse_bundle_json(&rendered).expect("json bundle should parse");
    let json_profile = json_workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    assert_eq!(
        json_profile
            .owners
            .owner_kind_for_state("ready_for_plan_review"),
        Some(&OwnerKind::Agent)
    );
}

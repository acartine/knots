use std::path::Path;

use super::{
    claim_knot, list_queue_candidates, peek_knot, poll_queue, run_claim, run_poll, run_ready,
};
use crate::app::{App, CreateKnotOptions};
use crate::cli::{ClaimArgs, PollArgs, ReadyArgs};
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::invariant::{Invariant, InvariantType};
use crate::domain::knot_type::KnotType;

fn unique_workspace() -> knots_test_support::TestWorkspace {
    knots_test_support::workspace("knots-poll-gate-ext")
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn list_and_poll_gate_candidates_respect_stage_and_owner() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    app.create_knot(
        "Implementation work",
        None,
        Some("ready_for_implementation"),
        None,
    )
    .expect("work knot should be created");
    let gate = app
        .create_knot_with_options(
            "Human gate",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Human,
                    ..Default::default()
                },
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");

    let evaluate_candidates =
        list_queue_candidates(&app, Some("evaluate")).expect("evaluate list should work");
    assert_eq!(evaluate_candidates.len(), 1);
    assert_eq!(evaluate_candidates[0].id, gate.id);

    let human = poll_queue(&app, Some("evaluate"), Some("human"), false)
        .expect("human poll should work")
        .expect("human should see gate");
    assert_eq!(human.knot.id, gate.id);
    assert!(human.skill.contains("# Evaluating"));
    assert!(human.completion_cmd.contains("--expected-state evaluating"));

    let agent =
        poll_queue(&app, Some("evaluate"), Some("agent"), false).expect("agent poll should work");
    assert!(agent.is_none());
}

#[test]
fn peek_and_claim_gate_follow_gate_workflow_states() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    let gate = app
        .create_knot_with_options(
            "Agent gate",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");

    let peeked = peek_knot(&app, &gate.id, false).expect("peek should succeed");
    assert_eq!(
        peeked.knot.state,
        crate::workflow_runtime::READY_TO_EVALUATE
    );
    assert!(peeked.skill.contains("# Evaluating"));
    assert!(peeked
        .completion_cmd
        .contains("--expected-state evaluating"));

    let claimed = claim_knot(&app, &gate.id, Some("agent".to_string()), None, 600, false)
        .expect("claim should succeed");
    assert_eq!(claimed.knot.state, crate::workflow_runtime::EVALUATING);
    assert!(claimed.skill.contains("# Evaluating"));

    let stored = app
        .show_knot(&gate.id)
        .expect("show should work")
        .expect("gate should exist");
    assert_eq!(stored.state, crate::workflow_runtime::EVALUATING);
}

#[test]
fn run_poll_and_claim_cover_json_and_text_rendering_paths() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    let gate = app
        .create_knot_with_options(
            "Poll gate",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");
    run_poll(
        &app,
        PollArgs {
            stage: Some("evaluate".to_string()),
            owner: Some("agent".to_string()),
            claim: false,
            json: true,
            agent_name: None,
            agent_model: None,
            agent_version: None,
            timeout_seconds: None,
            e2e: false,
        },
    )
    .expect("poll should succeed");

    run_ready(
        &app,
        ReadyArgs {
            ready_type: Some("evaluate".to_string()),
            owner: None,
            json: true,
        },
    )
    .expect("ready should succeed");

    let claimable = app
        .create_knot_with_options(
            "Claim gate",
            None,
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData::default(),
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");
    run_claim(
        &app,
        ClaimArgs {
            id: claimable.id,
            json: true,
            agent_name: Some("codex".to_string()),
            agent_model: Some("gpt-5".to_string()),
            agent_version: Some("1.0".to_string()),
            peek: false,
            verbose: false,
            lease: None,
            timeout_seconds: None,
            e2e: false,
        },
    )
    .expect("claim should succeed");

    run_claim(
        &app,
        ClaimArgs {
            id: gate.id,
            json: false,
            agent_name: None,
            agent_model: None,
            agent_version: None,
            peek: true,
            verbose: true,
            lease: None,
            timeout_seconds: None,
            e2e: false,
        },
    )
    .expect("peek claim should succeed");
}

#[test]
fn run_ready_owner_filter_matches_pollable_owner() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    app.create_knot(
        "Implementation work",
        None,
        Some("ready_for_implementation"),
        None,
    )
    .expect("work knot should be created");
    app.create_knot_with_options(
        "Human gate",
        None,
        None,
        None,
        None,
        CreateKnotOptions {
            knot_type: KnotType::Gate,
            gate_data: GateData {
                owner_kind: GateOwnerKind::Human,
                ..Default::default()
            },
            ..CreateKnotOptions::default()
        },
    )
    .expect("gate should be created");

    let agent_candidates =
        list_queue_candidates(&app, None).expect("list candidates should succeed");
    assert_eq!(
        agent_candidates.len(),
        2,
        "unfiltered ready shows both queue items"
    );

    let human_candidates = list_queue_candidates(&app, Some("evaluate"))
        .expect("stage-filtered candidates should succeed");
    assert_eq!(human_candidates.len(), 1, "stage filter isolates the gate");

    let human_poll = poll_queue(&app, Some("evaluate"), Some("human"), false)
        .expect("human poll should work")
        .expect("human should see the gate");
    assert_eq!(human_poll.knot.title, "Human gate");
}

#[test]
fn claim_prompt_for_gate_surfaces_acceptance_context_and_evaluation_rules() {
    use std::collections::BTreeMap;

    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    let target = app
        .create_knot("Blocked work", None, Some("idea"), Some("default"))
        .expect("target should be created");
    let mut failure_modes = BTreeMap::new();
    failure_modes.insert("tests must pass".to_string(), vec![target.id.clone()]);
    let gate = app
        .create_knot_with_options(
            "Release gate",
            Some("NON-TRIVIAL DESCRIPTION TEXT"),
            None,
            None,
            None,
            CreateKnotOptions {
                knot_type: KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Agent,
                    failure_modes,
                },
                acceptance: Some(
                    "NON-TRIVIAL ACCEPTANCE TEXT\n- collect evidence\n- compare outputs"
                        .to_string(),
                ),
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should be created");
    let gate = app
        .update_knot(
            &gate.id,
            crate::app::UpdateKnotPatch {
                add_invariants: vec![Invariant::new(InvariantType::State, "tests must pass")
                    .expect("invariant should build")],
                expected_profile_etag: gate.profile_etag.clone(),
                ..crate::app::UpdateKnotPatch::default()
            },
        )
        .expect("invariants should be added");

    let claimed = claim_knot(&app, &gate.id, Some("agent".to_string()), None, 600, false)
        .expect("claim should succeed");
    let prompt = claimed.skill;
    let full_prompt =
        crate::prompt::render_prompt(&claimed.knot, &prompt, &claimed.completion_cmd, false);
    assert!(!claimed.e2e, "default claim should not carry e2e flag");

    for needle in [
        "## Context",
        "NON-TRIVIAL DESCRIPTION TEXT",
        "## Acceptance Criteria",
        "NON-TRIVIAL ACCEPTANCE TEXT",
        "## Your job",
        "Advancing state is NOT evaluation.",
        "## Exit conditions",
        "On pass:",
        "On fail:",
        "handoff capsule",
        "actual-vs-expected",
        "gate.owner_kind: agent",
        "gate.failure_modes[tests must pass]",
        "## Override of Foolery preamble",
        "completion command is a state transition only",
    ] {
        assert!(
            full_prompt.contains(needle),
            "claim prompt should contain {needle:?}"
        );
    }
}

#[test]
fn claim_with_e2e_emits_e2e_continuation_boundary() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    app.create_knot("E2E claim work", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    let candidates = list_queue_candidates(&app, None).expect("list candidates");
    let knot_id = candidates
        .into_iter()
        .find(|k| k.knot_type == KnotType::Work)
        .expect("work knot in queue")
        .id;

    let claimed = claim_knot(&app, &knot_id, Some("agent".to_string()), None, 600, true)
        .expect("e2e claim should succeed");
    assert!(claimed.e2e, "PollResult must carry the e2e flag through");
    let rendered =
        crate::prompt::render_prompt(&claimed.knot, &claimed.skill, &claimed.completion_cmd, true);
    assert!(rendered.contains("kind: `e2e_continuation`"));
    assert!(rendered.contains("E2E continuation"));
    assert!(rendered.contains("kno claim --e2e"));
    assert!(!rendered.contains("Complete exactly one workflow action, then stop."));
}

#[test]
fn claim_without_e2e_emits_single_action_boundary() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    app.create_knot(
        "Single-action claim work",
        None,
        Some("work_item"),
        Some("default"),
    )
    .expect("work knot should be created");

    let candidates = list_queue_candidates(&app, None).expect("list candidates");
    let knot_id = candidates
        .into_iter()
        .find(|k| k.knot_type == KnotType::Work)
        .expect("work knot in queue")
        .id;

    let claimed = claim_knot(&app, &knot_id, Some("agent".to_string()), None, 600, false)
        .expect("claim should succeed");
    assert!(!claimed.e2e);
    let rendered = crate::prompt::render_prompt(
        &claimed.knot,
        &claimed.skill,
        &claimed.completion_cmd,
        false,
    );
    assert!(rendered.contains("kind: `single_action`"));
    assert!(rendered.contains("Complete exactly one workflow action, then stop."));
    assert!(!rendered.contains("E2E continuation"));
}

#[test]
fn json_render_carries_e2e_signals_through_claim_pipeline() {
    let root_ws = unique_workspace();
    let root = root_ws.path().to_path_buf();
    let app = open_app(&root);
    app.create_knot("JSON e2e", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");

    let candidates = list_queue_candidates(&app, None).expect("list candidates");
    let knot_id = candidates
        .into_iter()
        .find(|k| k.knot_type == KnotType::Work)
        .expect("work knot in queue")
        .id;

    let claimed = claim_knot(&app, &knot_id, Some("agent".to_string()), None, 600, true)
        .expect("e2e claim should succeed");
    let json = super::render_json(&claimed);
    assert_eq!(json["e2e"], serde_json::Value::Bool(true));
    assert_eq!(json["workflow_boundary_kind"], "e2e_continuation");
}

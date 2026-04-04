use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{
    claim_knot, list_queue_candidates, peek_knot, poll_queue, run_claim, run_poll, run_ready,
};
use crate::app::{App, CreateKnotOptions, StateActorMetadata};
use crate::cli::{ClaimArgs, PollArgs, ReadyArgs};
use crate::domain::gate::{GateData, GateOwnerKind};
use crate::domain::knot_type::KnotType;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-poll-gate-ext-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn open_app(root: &Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn list_and_poll_gate_candidates_respect_stage_and_owner() {
    let root = unique_workspace();
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

    let human = poll_queue(&app, Some("evaluate"), Some("human"))
        .expect("human poll should work")
        .expect("human should see gate");
    assert_eq!(human.knot.id, gate.id);
    assert!(human.skill.contains("# Evaluating"));
    assert!(human.completion_cmd.contains("--expected-state evaluating"));

    let agent = poll_queue(&app, Some("evaluate"), Some("agent")).expect("agent poll should work");
    assert!(agent.is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn peek_and_claim_gate_follow_gate_workflow_states() {
    let root = unique_workspace();
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

    let peeked = peek_knot(&app, &gate.id).expect("peek should succeed");
    assert_eq!(
        peeked.knot.state,
        crate::workflow_runtime::READY_TO_EVALUATE
    );
    assert!(peeked.skill.contains("# Evaluating"));
    assert!(peeked
        .completion_cmd
        .contains("--expected-state evaluating"));

    let claimed = claim_knot(&app, &gate.id, StateActorMetadata::default(), None, 600)
        .expect("claim should succeed");
    assert_eq!(claimed.knot.state, crate::workflow_runtime::EVALUATING);
    assert!(claimed.skill.contains("# Evaluating"));

    let stored = app
        .show_knot(&gate.id)
        .expect("show should work")
        .expect("gate should exist");
    assert_eq!(stored.state, crate::workflow_runtime::EVALUATING);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_poll_and_claim_cover_json_and_text_rendering_paths() {
    let root = unique_workspace();
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
        },
    )
    .expect("poll should succeed");

    run_ready(
        &app,
        ReadyArgs {
            ready_type: Some("evaluate".to_string()),
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
        },
    )
    .expect("peek claim should succeed");

    let _ = std::fs::remove_dir_all(root);
}

use super::{App, CreateKnotOptions, UpdateKnotPatch};
use crate::domain::gate::{GateData, GateOwnerKind};
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;

const RESPONSE_REVIEW_FLOW: &str = r#"
[workflow]
name = "review_flow"
version = 1
default_profile = "reviewed"

[states.ready_for_work]
kind = "queue"

[states.work]
kind = "action"
executor = "agent"
prompt = "work"
output = "branch"

[states.ready_for_review]
kind = "queue"

[states.review]
kind = "action"
executor = "human"
prompt = "review"
output = "approval"
review_hint = "Check tests pass and coverage meets threshold"

[states.done]
kind = "terminal"

[states.deferred]
kind = "escape"

[states.abandoned]
kind = "terminal"

[steps.impl]
queue = "ready_for_work"
action = "work"

[steps.rev]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "impl"
gate = "rev"

[profiles.reviewed]
phases = ["main"]

[prompts.work]
accept = ["Built output"]
body = "Ship it."

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "deferred"

[prompts.review]
accept = ["Reviewed"]
body = "Review it."

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

fn unique_workspace() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-step-meta-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    crate::installed_workflows::ensure_builtin_workflows_registered(&root)
        .expect("builtin workflows should register");
    root
}

fn latest_knot_head_payload(root: &Path, knot_id: &str) -> Value {
    let mut paths = Vec::new();
    let mut stack = vec![root.join(".knots/index")];
    while let Some(dir) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir).expect("index dir should be readable") {
            let path = entry.expect("index entry should read").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .into_iter()
        .rev()
        .find_map(|path| {
            let json: Value = serde_json::from_slice(
                &std::fs::read(&path).expect("index event should be readable"),
            )
            .expect("index event should parse");
            let data = json.get("data")?;
            (json["type"] == "idx.knot_head" && data["knot_id"] == knot_id).then(|| data.clone())
        })
        .expect("knot head payload should exist")
}

fn install_review_flow(root: &Path) {
    let wf_root = root.join(".knots/workflows/review_flow/1");
    std::fs::create_dir_all(&wf_root).expect("workflow dir should create");
    std::fs::write(wf_root.join("bundle.toml"), RESPONSE_REVIEW_FLOW).expect("bundle should write");
}

#[test]
fn autopilot_mutation_responses_and_logs_include_step_metadata() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot_with_options(
            "Metadata work",
            Some("desc"),
            Some("implementation"),
            Some("autopilot"),
            None,
            CreateKnotOptions::default(),
        )
        .expect("create should succeed");
    assert_eq!(
        created
            .step_metadata
            .as_ref()
            .map(|meta| meta.action_state.as_str()),
        Some("implementation"),
    );
    assert_eq!(
        created
            .next_step_metadata
            .as_ref()
            .map(|meta| meta.action_state.as_str()),
        Some("implementation_review"),
    );
    let create_payload = latest_knot_head_payload(&root, &created.id);
    assert_eq!(
        create_payload["step_metadata"],
        serde_json::to_value(created.step_metadata.as_ref().expect("step metadata"))
            .expect("step metadata should serialize"),
    );

    let updated = app
        .update_knot(
            &created.id,
            UpdateKnotPatch {
                add_note: Some(crate::domain::metadata::MetadataEntryInput {
                    content: "keep metadata".to_string(),
                    ..Default::default()
                }),
                expected_profile_etag: created.profile_etag.clone(),
                ..UpdateKnotPatch::default()
            },
        )
        .expect("update should succeed");
    assert_eq!(updated.step_metadata, created.step_metadata);
    let update_payload = latest_knot_head_payload(&root, &created.id);
    assert_eq!(
        update_payload["step_metadata"],
        serde_json::to_value(updated.step_metadata.as_ref().expect("step metadata"))
            .expect("step metadata should serialize"),
    );

    let moved = app
        .set_state(
            &created.id,
            "ready_for_implementation_review",
            false,
            updated.profile_etag.as_deref(),
        )
        .expect("transition should succeed");
    assert_eq!(
        moved
            .step_metadata
            .as_ref()
            .map(|meta| meta.action_state.as_str()),
        Some("implementation_review"),
    );
    let moved_payload = latest_knot_head_payload(&root, &created.id);
    assert_eq!(
        moved_payload["step_metadata"],
        serde_json::to_value(moved.step_metadata.as_ref().expect("step metadata"))
            .expect("step metadata should serialize"),
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn custom_workflow_show_response_includes_review_hint_metadata() {
    let root = unique_workspace();
    install_review_flow(&root);
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let created = app
        .create_knot_with_options(
            "Custom review",
            Some("desc"),
            Some("ready_for_review"),
            Some("reviewed"),
            Some("review_flow"),
            CreateKnotOptions::default(),
        )
        .expect("create should succeed");
    let shown = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");

    assert_eq!(
        shown
            .step_metadata
            .as_ref()
            .and_then(|meta| meta.review_hint.as_deref()),
        Some("Check tests pass and coverage meets threshold"),
    );
    assert_eq!(
        shown
            .step_metadata
            .as_ref()
            .and_then(|meta| meta.output.as_ref())
            .map(|output| output.artifact_type.as_str()),
        Some("approval"),
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gate_show_response_includes_owner_metadata() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app =
        App::open(db_path.to_str().expect("utf8 path"), root.clone()).expect("app should open");

    let gate = app
        .create_knot_with_options(
            "Gate metadata",
            Some("desc"),
            Some("ready_to_evaluate"),
            Some("default"),
            None,
            CreateKnotOptions {
                knot_type: crate::domain::knot_type::KnotType::Gate,
                gate_data: GateData {
                    owner_kind: GateOwnerKind::Human,
                    ..Default::default()
                },
                ..CreateKnotOptions::default()
            },
        )
        .expect("gate should create");
    let shown = app
        .show_knot(&gate.id)
        .expect("show should succeed")
        .expect("gate should exist");

    assert_eq!(
        shown
            .step_metadata
            .as_ref()
            .and_then(|meta| meta.owner.as_ref())
            .map(|owner| owner.kind.clone()),
        Some(crate::workflow::OwnerKind::Human),
    );
    assert_eq!(
        shown
            .step_metadata
            .as_ref()
            .and_then(|meta| meta.action_kind.as_deref()),
        Some("gate"),
    );

    let _ = std::fs::remove_dir_all(root);
}

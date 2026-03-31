use std::collections::BTreeMap;

use super::bundle_json::parse_bundle_json;
use super::bundle_toml::{
    render_json_bundle_from_toml, BundleOutputEntry, BundlePhaseSection, BundleProfileSection,
    BundlePromptSection, BundleStateSection, BundleStepSection,
};
use super::profile_toml::build_profile_definition;
use super::tests_helpers::SAMPLE_BUNDLE;
use crate::profile::ActionOutputDef;

#[test]
fn output_builders_prefer_profile_entries() {
    let toml_outputs = super::profile_toml::build_outputs_from_toml_profile(
        &BTreeMap::from([(
            "work".to_string(),
            BundleOutputEntry {
                artifact_type: "artifact".to_string(),
                access_hint: Some("hint".to_string()),
            },
        )]),
        &["work".to_string(), "missing".to_string()],
        &BTreeMap::new(),
    );
    assert_eq!(toml_outputs.len(), 1);
    assert_eq!(
        toml_outputs.get("work"),
        Some(&ActionOutputDef {
            artifact_type: "artifact".to_string(),
            access_hint: Some("hint".to_string()),
        })
    );

    let json_outputs = super::profile_json::build_outputs_from_json_profile(
        &BTreeMap::from([(
            "review".to_string(),
            super::bundle_json::JsonOutputEntry {
                artifact_type: "note".to_string(),
                access_hint: Some("open".to_string()),
            },
        )]),
        &["review".to_string(), "missing".to_string()],
        &BTreeMap::new(),
    );
    assert_eq!(json_outputs.len(), 1);
    assert_eq!(
        json_outputs.get("review"),
        Some(&ActionOutputDef {
            artifact_type: "note".to_string(),
            access_hint: Some("open".to_string()),
        })
    );
}

#[test]
fn json_validates_profile_references_and_shape() {
    let rendered = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
    let mut json: serde_json::Value =
        serde_json::from_str(&rendered).expect("json bundle should parse");

    json["profiles"][0]["phases"] = serde_json::json!(["missing"]);
    let err = parse_bundle_json(&serde_json::to_string(&json).expect("ser"))
        .expect_err("unknown phase should fail");
    assert!(err.to_string().contains("unknown phase"));

    let mut json: serde_json::Value =
        serde_json::from_str(&rendered).expect("json bundle should parse");
    json["phases"][0]["produce_step"] = serde_json::json!("missing");
    let err = parse_bundle_json(&serde_json::to_string(&json).expect("ser"))
        .expect_err("unknown step should fail");
    assert!(err.to_string().contains("unknown step"));

    let mut json: serde_json::Value =
        serde_json::from_str(&rendered).expect("json bundle should parse");
    json["profiles"][0]["phases"] = serde_json::json!([]);
    let err = parse_bundle_json(&serde_json::to_string(&json).expect("ser"))
        .expect_err("empty phases should fail");
    assert!(err
        .to_string()
        .contains("profile has no initial queue state"));

    let mut json: serde_json::Value =
        serde_json::from_str(&rendered).expect("json bundle should parse");
    json["profiles"][0]["id"] = serde_json::json!("   ");
    let err = parse_bundle_json(&serde_json::to_string(&json).expect("ser"))
        .expect_err("blank profile id should fail");
    assert!(err.to_string().contains("profile id is required"));
}

#[test]
fn profile_def_validates_empty_phase() {
    let (states, steps, phases, prompts) = build_minimal_test_data();
    let empty = BundleProfileSection {
        description: None,
        phases: Vec::new(),
        outputs: BTreeMap::new(),
        overrides: BTreeMap::new(),
    };
    let err = build_profile_definition("wf", "empty", &empty, &states, &steps, &phases, &prompts)
        .expect_err("empty profile should fail");
    assert!(err.to_string().contains("must define at least one phase"));
}

fn broken_profile(phases: &[&str]) -> BundleProfileSection {
    BundleProfileSection {
        description: None,
        phases: phases.iter().map(|s| s.to_string()).collect(),
        outputs: BTreeMap::new(),
        overrides: BTreeMap::new(),
    }
}

fn broken_steps_and_phases(
    queue: &str,
    action: &str,
) -> (
    BTreeMap<String, BundleStepSection>,
    BTreeMap<String, BundlePhaseSection>,
) {
    let steps = BTreeMap::from([(
        String::from("broken"),
        BundleStepSection {
            queue: queue.to_string(),
            action: action.to_string(),
        },
    )]);
    let phases = BTreeMap::from([(
        String::from("broken"),
        BundlePhaseSection {
            produce: "broken".to_string(),
            gate: Some("broken".to_string()),
        },
    )]);
    (steps, phases)
}

#[test]
fn profile_def_validates_missing_queue() {
    let (states, mut steps, mut phases, prompts) = build_minimal_test_data();

    steps.insert(
        "broken".to_string(),
        BundleStepSection {
            queue: "missing".to_string(),
            action: "work".to_string(),
        },
    );
    phases.insert(
        "broken".to_string(),
        BundlePhaseSection {
            produce: "broken".to_string(),
            gate: Some("broken".to_string()),
        },
    );
    let err = build_profile_definition(
        "wf",
        "broken",
        &broken_profile(&["broken"]),
        &states,
        &steps,
        &phases,
        &prompts,
    )
    .expect_err("missing queue should fail");
    assert!(err.to_string().contains("unknown queue state"));
}

#[test]
fn profile_def_validates_missing_action() {
    let (states, _, _, prompts) = build_minimal_test_data();
    let (steps, phases) = broken_steps_and_phases("ready", "missing-action");
    let err = build_profile_definition(
        "wf",
        "broken-action",
        &broken_profile(&["broken"]),
        &states,
        &steps,
        &phases,
        &prompts,
    )
    .expect_err("missing action should fail");
    assert!(err.to_string().contains("unknown action state"));
}

#[test]
fn profile_def_validates_missing_prompt() {
    let (mut states, _, _, prompts) = build_minimal_test_data();
    states.insert(
        "orphan".to_string(),
        BundleStateSection {
            kind: "action".to_string(),
            executor: None,
            prompt: None,
            output: None,
            output_hint: None,
        },
    );
    let (steps, phases) = broken_steps_and_phases("ready", "orphan");
    let err = build_profile_definition(
        "wf",
        "orphan",
        &broken_profile(&["broken"]),
        &states,
        &steps,
        &phases,
        &prompts,
    )
    .expect_err("missing prompt should fail");
    assert!(err.to_string().contains("is missing prompt"));
}

type StateMap = BTreeMap<String, BundleStateSection>;
type StepMap = BTreeMap<String, BundleStepSection>;
type PhaseMap = BTreeMap<String, BundlePhaseSection>;
type PromptMap = BTreeMap<String, BundlePromptSection>;

fn build_minimal_test_data() -> (StateMap, StepMap, PhaseMap, PromptMap) {
    let mut states = BTreeMap::new();
    states.insert(
        "ready".to_string(),
        BundleStateSection {
            kind: "queue".to_string(),
            executor: None,
            prompt: None,
            output: None,
            output_hint: None,
        },
    );
    states.insert(
        "work".to_string(),
        BundleStateSection {
            kind: "action".to_string(),
            executor: None,
            prompt: Some("work".to_string()),
            output: None,
            output_hint: None,
        },
    );
    let mut steps = BTreeMap::new();
    steps.insert(
        "step".to_string(),
        BundleStepSection {
            queue: "ready".to_string(),
            action: "work".to_string(),
        },
    );
    let mut phases = BTreeMap::new();
    phases.insert(
        "phase".to_string(),
        BundlePhaseSection {
            produce: "step".to_string(),
            gate: Some("step".to_string()),
        },
    );
    let mut prompts = BTreeMap::new();
    prompts.insert(
        "work".to_string(),
        BundlePromptSection {
            accept: Vec::new(),
            success: BTreeMap::from([(String::from("ok"), String::from("done"))]),
            failure: BTreeMap::new(),
            body: String::from("do work"),
            params: BTreeMap::new(),
        },
    );
    (states, steps, phases, prompts)
}

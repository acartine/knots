use std::collections::BTreeMap;

use super::bundle_json::parse_bundle_json;
use super::bundle_toml::{parse_bundle_toml, render_json_bundle_from_toml};
use super::tests_helpers::{build_prompt_params, render_prompt_template, SAMPLE_BUNDLE};
use super::*;
use crate::domain::knot_type::KnotType;
use crate::profile::OwnerKind;

#[test]
fn parses_bundle_and_renders_prompt() {
    let workflow = parse_bundle_toml(SAMPLE_BUNDLE).expect("bundle should parse");
    assert_eq!(workflow.id, "custom_flow");
    let profile = workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    assert_eq!(profile.initial_state, "ready_for_work");
    let prompt = workflow
        .prompt_for_action_state("work")
        .expect("prompt should exist");
    let rendered = prompt.render(&workflow, profile);
    assert!(rendered.contains("Ship branch output."));
    assert!(rendered.contains("Built output"));
}

#[test]
fn json_round_trips_and_preserves_prompt_routes() {
    let rendered = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render should work");
    let workflow = parse_bundle_json(&rendered).expect("json bundle should parse");
    let profile = workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    assert_eq!(profile.initial_state, "ready_for_work");
    assert_eq!(
        profile.next_happy_path_state("work"),
        Some("ready_for_review")
    );
    assert_eq!(
        profile.prompt_for_action_state("review"),
        Some("Review it.\n")
    );
    assert_eq!(
        profile.acceptance_for_action_state("work"),
        &["Built output".to_string()]
    );
}

#[test]
fn json_rejects_unsupported_metadata() {
    let wrong_format = r#"{
  "format": "other",
  "format_version": 1,
  "workflow": {"name": "x", "version": 1, "default_profile": null},
  "states": [], "steps": [], "phases": [],
  "profiles": [], "prompts": []
}"#;
    let err = parse_bundle_json(wrong_format).expect_err("unknown format should fail");
    assert!(err.to_string().contains("unsupported bundle format"));

    let wrong_version = r#"{
  "format": "knots-bundle",
  "format_version": 99,
  "workflow": {"name": "x", "version": 1, "default_profile": null},
  "states": [], "steps": [], "phases": [],
  "profiles": [], "prompts": []
}"#;
    let err = parse_bundle_json(wrong_version).expect_err("unknown version should fail");
    assert!(err
        .to_string()
        .contains("unsupported bundle format version"));
}

#[test]
fn toml_rejects_multiple_success_outcomes() {
    let invalid = SAMPLE_BUNDLE.replace(
        "[prompts.work.success]\n\
         complete = \"ready_for_review\"\n",
        "[prompts.work.success]\n\
         complete = \"ready_for_review\"\n\
         also_complete = \"done\"\n",
    );
    let err = parse_bundle_toml(&invalid).expect_err("multiple success should fail");
    assert!(err.to_string().contains("multiple success outcomes"));
}

#[test]
fn toml_reads_per_action_outputs() {
    let workflow = parse_bundle_toml(SAMPLE_BUNDLE).expect("bundle should parse");
    let profile = workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    let work_output = profile
        .outputs
        .get("work")
        .expect("work output should exist");
    assert_eq!(work_output.artifact_type, "branch");
    assert_eq!(work_output.access_hint.as_deref(), Some("git log"));
    let review_output = profile
        .outputs
        .get("review")
        .expect("review output should exist");
    assert_eq!(review_output.artifact_type, "note");
    assert!(review_output.access_hint.is_none());
}

#[test]
fn toml_reads_review_hints_per_action() {
    let with_hint = SAMPLE_BUNDLE.replace(
        "[states.review]\n\
         display_name = \"Review\"\n\
         kind = \"action\"\n\
         action_type = \"gate\"\n\
         executor = \"human\"\n\
         prompt = \"review\"\n\
         output = \"note\"\n",
        "[states.review]\n\
         display_name = \"Review\"\n\
         kind = \"action\"\n\
         action_type = \"gate\"\n\
         executor = \"human\"\n\
         prompt = \"review\"\n\
         output = \"note\"\n\
         review_hint = \"Verify coverage above 95%\"\n",
    );
    let workflow = parse_bundle_toml(&with_hint).expect("bundle should parse");
    let profile = workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    assert_eq!(
        profile.review_hints.get("review").map(String::as_str),
        Some("Verify coverage above 95%"),
    );
    assert!(!profile.review_hints.contains_key("work"));
}

#[test]
fn output_builders_cover_override_fallback_and_missing_state_paths() {
    let review_state = bundle_json::JsonStateSection {
        id: "review".to_string(),
        kind: "action".to_string(),
        prompt: None,
        executor: None,
        output: Some("note".to_string()),
        output_hint: Some("inspect".to_string()),
        review_hint: None,
    };
    let json_states = BTreeMap::from([("review", &review_state)]);
    let json = profile_json::build_outputs_from_json_profile(
        &BTreeMap::from([(
            "work".to_string(),
            bundle_json::JsonOutputEntry {
                artifact_type: "bogus".to_string(),
                access_hint: Some("override".to_string()),
            },
        )]),
        &[
            "work".to_string(),
            "review".to_string(),
            "missing".to_string(),
        ],
        &json_states,
    );
    assert_eq!(json["work"].artifact_type, "bogus");
    assert_eq!(json["review"].artifact_type, "note");
    assert!(!json.contains_key("missing"));

    let toml = profile_toml::build_outputs_from_toml_profile(
        &BTreeMap::from([(
            "work".to_string(),
            bundle_toml::BundleOutputEntry {
                artifact_type: "bogus".to_string(),
                access_hint: Some("override".to_string()),
            },
        )]),
        &[
            "work".to_string(),
            "review".to_string(),
            "missing".to_string(),
        ],
        &BTreeMap::from([(
            "review".to_string(),
            bundle_toml::BundleStateSection {
                kind: "action".to_string(),
                executor: None,
                prompt: None,
                output: Some("note".to_string()),
                output_hint: Some("inspect".to_string()),
                review_hint: None,
            },
        )]),
    );
    assert_eq!(toml["work"].artifact_type, "bogus");
    assert_eq!(toml["review"].artifact_type, "note");
    assert!(!toml.contains_key("missing"));
}

#[test]
fn builtin_workflow_has_prompts_and_profiles() {
    let workflow = builtin::work_sdlc_workflow_for_test().expect("builtin workflow should build");
    assert!(workflow.builtin);
    assert_eq!(workflow.default_profile.as_deref(), Some("autopilot"));
    assert!(workflow.prompts.contains_key("planning"));
    assert!(workflow.action_prompts.contains_key("implementation"));
    assert!(workflow.require_profile("autopilot").is_ok());
}

#[test]
fn builtin_workflow_slice_excludes_gate_and_explore_actions() {
    let workflow = builtin::work_sdlc_workflow_for_test().expect("work workflow should build");
    assert_eq!(workflow.id, "work_sdlc");
    assert!(workflow.prompts.contains_key("planning"));
    assert!(workflow.prompts.contains_key("implementation"));
    assert!(workflow.prompts.contains_key("shipment"));
    assert!(!workflow.prompts.contains_key("evaluating"));
    assert!(!workflow.prompts.contains_key("exploration"));
}

#[test]
fn builtin_workflow_refs_cover_all_builtin_knot_types() {
    let refs = [
        (KnotType::Work, "work_sdlc"),
        (KnotType::Gate, "gate_sdlc"),
        (KnotType::Lease, "lease_sdlc"),
        (KnotType::Explore, "explore_sdlc"),
    ];
    for (knot_type, workflow_id) in refs {
        let reference = builtin::builtin_workflow_ref(knot_type);
        assert_eq!(reference.workflow_id, workflow_id);
        assert_eq!(reference.version, Some(1));
    }

    let workflows = builtin::builtin_workflows().expect("builtin workflows should load");
    assert_eq!(workflows.len(), 4);
    assert!(workflows
        .iter()
        .any(|(knot_type, workflow)| *knot_type == KnotType::Work && workflow.id == "work_sdlc"));
    assert!(workflows
        .iter()
        .any(|(knot_type, workflow)| *knot_type == KnotType::Gate && workflow.id == "gate_sdlc"));
    assert!(workflows
        .iter()
        .any(|(knot_type, workflow)| *knot_type == KnotType::Lease && workflow.id == "lease_sdlc"));
    assert!(workflows.iter().any(|(knot_type, workflow)| {
        *knot_type == KnotType::Explore && workflow.id == "explore_sdlc"
    }));
}

#[test]
fn builtin_non_work_workflows_expose_expected_profiles_and_prompts() {
    let work = builtin::work_sdlc_workflow_for_test().expect("work workflow should build");
    assert_eq!(work.id, "work_sdlc");
    assert!(work.require_profile("autopilot").is_ok());

    let gate = builtin::gate_sdlc_workflow_for_test().expect("gate workflow should build");
    assert!(gate.builtin);
    assert_eq!(gate.id, "gate_sdlc");
    assert_eq!(gate.default_profile.as_deref(), Some("evaluate"));
    assert!(gate.prompts.contains_key("evaluating"));
    assert!(gate.require_profile("evaluate").is_ok());

    let lease = builtin::lease_sdlc_workflow_for_test().expect("lease workflow should build");
    assert!(lease.builtin);
    assert_eq!(lease.id, "lease_sdlc");
    assert_eq!(lease.default_profile.as_deref(), Some("lease"));
    assert!(lease.prompts.contains_key("lease_active"));
    assert!(lease.require_profile("lease").is_ok());

    let explore = builtin::explore_sdlc_workflow_for_test().expect("explore workflow should build");
    assert!(explore.builtin);
    assert_eq!(explore.id, "explore_sdlc");
    assert_eq!(explore.default_profile.as_deref(), Some("explore"));
    assert!(explore.prompts.contains_key("exploration"));
    assert!(explore.require_profile("explore").is_ok());
}

#[test]
fn parse_bundle_dispatches_both_formats() {
    let json_bundle = render_json_bundle_from_toml(SAMPLE_BUNDLE).expect("json render");
    let from_toml = parse_bundle(SAMPLE_BUNDLE, BundleFormat::Toml).expect("toml parse");
    let from_json = parse_bundle(&json_bundle, BundleFormat::Json).expect("json parse");
    assert_eq!(from_toml.id, from_json.id);
    assert_eq!(from_toml.version, from_json.version);
}

#[test]
fn toml_reports_missing_phase_and_step_references() {
    let missing_phase = SAMPLE_BUNDLE.replace("phases = [\"main\"]", "phases = [\"missing\"]");
    let err = parse_bundle_toml(&missing_phase).expect_err("missing phase should fail");
    assert!(err.to_string().contains("unknown phase"));

    let missing_step = SAMPLE_BUNDLE.replace("produce = \"impl\"", "produce = \"missing\"");
    let err = parse_bundle_toml(&missing_step).expect_err("missing step should fail");
    assert!(err.to_string().contains("unknown step"));
}

#[test]
fn toml_reports_invalid_state_kinds_and_prompt() {
    let invalid_queue = SAMPLE_BUNDLE.replace(
        "[states.ready_for_work]\n\
         display_name = \"Ready for Work\"\n\
         kind = \"queue\"\n",
        "[states.ready_for_work]\n\
         display_name = \"Ready for Work\"\n\
         kind = \"action\"\n",
    );
    let err = parse_bundle_toml(&invalid_queue).expect_err("queue kind should fail");
    assert!(err.to_string().contains("must be a queue state"));

    let invalid_action = SAMPLE_BUNDLE.replace(
        "[states.work]\n\
         display_name = \"Work\"\n\
         kind = \"action\"\n\
         action_type = \"produce\"\n\
         executor = \"agent\"\n\
         prompt = \"work\"\n",
        "[states.work]\n\
         display_name = \"Work\"\n\
         kind = \"queue\"\n\
         action_type = \"produce\"\n\
         executor = \"agent\"\n\
         prompt = \"work\"\n",
    );
    let err = parse_bundle_toml(&invalid_action).expect_err("action kind should fail");
    assert!(err.to_string().contains("must be an action state"));

    let missing_prompt = SAMPLE_BUNDLE.replace("prompt = \"work\"\n", "");
    let err = parse_bundle_toml(&missing_prompt).expect_err("missing prompt should fail");
    assert!(err.to_string().contains("missing prompt"));

    let unknown_prompt = SAMPLE_BUNDLE.replace("prompt = \"work\"", "prompt = \"missing\"");
    let err = parse_bundle_toml(&unknown_prompt).expect_err("unknown prompt should fail");
    assert!(err.to_string().contains("references unknown prompt"));
}

#[test]
fn toml_requires_success_and_honors_overrides() {
    let missing_success = SAMPLE_BUNDLE.replace(
        "[prompts.work.success]\n\
         complete = \"ready_for_review\"\n",
        "",
    );
    let err = parse_bundle_toml(&missing_success).expect_err("missing success should fail");
    assert!(err.to_string().contains("must define one success target"));

    let overridden = SAMPLE_BUNDLE.replace(
        "[profiles.autopilot]\n\
         description = \"Custom profile\"\n\
         phases = [\"main\"]\n",
        "[profiles.autopilot]\n\
         description = \"Custom profile\"\n\
         phases = [\"main\"]\n\
         overrides.work = \"human\"\n",
    );
    let workflow = parse_bundle_toml(&overridden).expect("override bundle should parse");
    let profile = workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    assert_eq!(
        profile.owners.owner_kind_for_state("work"),
        Some(&OwnerKind::Human)
    );
    assert_eq!(
        profile.owners.owner_kind_for_state("ready_for_work"),
        Some(&OwnerKind::Human)
    );
}

#[test]
fn helpers_cover_prompt_rendering_and_utils() {
    assert_eq!(
        namespaced_profile_id("custom", "autopilot"),
        "custom/autopilot"
    );

    let mut values = vec!["a".to_string()];
    push_unique(&mut values, "a".to_string());
    push_unique(&mut values, "b".to_string());
    assert_eq!(values, vec!["a".to_string(), "b".to_string()]);

    let mut unresolved = Vec::new();
    let rendered = render_prompt_template(
        "Hello {{ name }} and {{ missing }}",
        &BTreeMap::from([(String::from("name"), String::from("Loom"))]),
        &mut unresolved,
    );
    assert_eq!(rendered, "Hello Loom and {{ missing }}");
    assert_eq!(unresolved, vec!["missing".to_string()]);

    let mut unresolved = Vec::new();
    let rendered = render_prompt_template("{{ name ", &BTreeMap::new(), &mut unresolved);
    assert_eq!(rendered, "{{ name ");
    assert!(unresolved.is_empty());
}

#[test]
fn prompt_defaults_cover_param_and_output() {
    let workflow = parse_bundle_toml(&SAMPLE_BUNDLE.replace(
        "[prompts.work]\n\
             accept = [\"Built output\"]\n\
             body = \"\"\"\n\
             Ship {{ output }} output.\n\"\"\"\n",
        "[prompts.work]\n\
             accept = [\"Built output\"]\n\
             body = \"\"\"\n\
             Ship {{ output }} output \
             for {{ audience }}.\n\"\"\"\n\
             [prompts.work.params.audience]\n\
             type = \"enum\"\n\
             default = \"operators\"\n",
    ))
    .expect("bundle with params should parse");
    let profile = workflow
        .require_profile("autopilot")
        .expect("profile should exist");
    let prompt = workflow
        .prompt_for_action_state("work")
        .expect("prompt should exist");
    let params = build_prompt_params(&workflow, profile, prompt);
    assert_eq!(
        params.get("audience").map(String::as_str),
        Some("operators")
    );
    assert_eq!(params.get("output").map(String::as_str), Some("branch"));
}

use super::bundle_toml::parse_bundle_toml;
use super::tests_helpers::{build_prompt_params, SAMPLE_BUNDLE};

#[test]
fn output_fallback_prefers_profile_level_over_state_level() {
    let with_override = SAMPLE_BUNDLE.replace(
        "[profiles.autopilot]\n\
         description = \"Custom profile\"\n\
         phases = [\"main\"]\n",
        "[profiles.autopilot]\n\
         description = \"Custom profile\"\n\
         phases = [\"main\"]\n\
         \n\
         [profiles.autopilot.outputs.work]\n\
         artifact_type = \"pr\"\n\
         access_hint = \"gh pr view\"\n",
    );
    let workflow = parse_bundle_toml(&with_override).expect("bundle should parse");
    let profile = workflow.require_profile("autopilot").expect("profile");
    let work_output = profile.outputs.get("work").expect("work output");
    assert_eq!(work_output.artifact_type, "pr");
    assert_eq!(work_output.access_hint.as_deref(), Some("gh pr view"));
}

#[test]
fn output_fallback_uses_state_level_when_profile_absent() {
    let workflow = parse_bundle_toml(SAMPLE_BUNDLE).expect("bundle should parse");
    let profile = workflow.require_profile("autopilot").expect("profile");
    let work_output = profile.outputs.get("work").expect("work output");
    assert_eq!(work_output.artifact_type, "branch");
    assert_eq!(work_output.access_hint.as_deref(), Some("git log"));
}

#[test]
fn build_prompt_params_propagates_access_hint() {
    let workflow = parse_bundle_toml(SAMPLE_BUNDLE).expect("bundle should parse");
    let profile = workflow.require_profile("autopilot").expect("profile");
    let prompt = workflow.prompt_for_action_state("work").expect("prompt");
    let params = build_prompt_params(&workflow, profile, prompt);
    assert_eq!(
        params.get("output_hint").map(String::as_str),
        Some("git log"),
    );
}

#[test]
fn build_prompt_params_omits_hint_when_absent() {
    let workflow = parse_bundle_toml(SAMPLE_BUNDLE).expect("bundle should parse");
    let profile = workflow.require_profile("autopilot").expect("profile");
    let prompt = workflow.prompt_for_action_state("review").expect("prompt");
    let params = build_prompt_params(&workflow, profile, prompt);
    assert_eq!(params.get("output").map(String::as_str), Some("note"));
    assert!(
        !params.contains_key("output_hint"),
        "output_hint should be absent when access_hint is None",
    );
}

#[test]
fn build_prompt_params_uses_empty_output_when_state_omits_artifact() {
    let no_output = SAMPLE_BUNDLE
        .replace("output = \"branch\"\noutput_hint = \"git log\"\n", "")
        .replace("output = \"note\"\n", "");
    let workflow = parse_bundle_toml(&no_output).expect("bundle should parse");
    let profile = workflow.require_profile("autopilot").expect("profile");
    let prompt = workflow.prompt_for_action_state("work").expect("prompt");
    let params = build_prompt_params(&workflow, profile, prompt);
    assert_eq!(
        params.get("output").map(String::as_str),
        Some(""),
        "output should be empty string when state has no artifact_type",
    );
    assert!(!params.contains_key("output_hint"));
}

use super::resolve_output_specific_sections;

const MULTI_OUTPUT_TEMPLATE: &str = "\
Do the work.
`{{ output }}` = `remote_main` means push to the remote main branch.
`{{ output }}` = `pr` means open a pull request.
`{{ output }}` = `branch` means push to the feature branch.
`{{ output }}` = `live_deployment` means deploy to the live environment.

Finish up.";

#[test]
fn selects_only_matching_output_block() {
    let resolved = resolve_output_specific_sections(MULTI_OUTPUT_TEMPLATE, Some("branch"));
    assert!(resolved.contains("push to the feature branch"));
    assert!(!resolved.contains("remote main"));
    assert!(!resolved.contains("pull request"));
    assert!(!resolved.contains("live environment"));
    assert!(resolved.contains("Do the work."));
    assert!(resolved.contains("Finish up."));
}

#[test]
fn live_deployment_output_selects_correct_block() {
    let resolved = resolve_output_specific_sections(MULTI_OUTPUT_TEMPLATE, Some("live_deployment"));
    assert!(resolved.contains("deploy to the live environment"));
    assert!(!resolved.contains("feature branch"));
    assert!(!resolved.contains("remote main"));
}

#[test]
fn none_output_strips_all_output_blocks() {
    let resolved = resolve_output_specific_sections(MULTI_OUTPUT_TEMPLATE, None);
    assert!(!resolved.contains("remote main"));
    assert!(!resolved.contains("pull request"));
    assert!(!resolved.contains("feature branch"));
    assert!(!resolved.contains("live environment"));
    assert!(resolved.contains("Do the work."));
    assert!(resolved.contains("Finish up."));
}

#[test]
fn template_without_output_blocks_passes_through() {
    let plain = "Just do the work.\nNo conditional blocks here.";
    let resolved = resolve_output_specific_sections(plain, Some("branch"));
    assert_eq!(resolved, plain);

    let resolved_none = resolve_output_specific_sections(plain, None);
    assert_eq!(resolved_none, plain);
}

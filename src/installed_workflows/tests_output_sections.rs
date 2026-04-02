use super::resolve_output_specific_sections;

#[test]
fn branch_output_selects_branch_block() {
    let template = "\
Perform the work.
`{{ output }}` = `branch` means push to a feature branch.
`{{ output }}` = `live_deployment` means deploy to production.

Done.";
    let result = resolve_output_specific_sections(template, Some("branch"));
    assert!(result.contains("push to a feature branch"));
    assert!(!result.contains("deploy to production"));
    assert!(result.contains("Perform the work."));
    assert!(result.contains("Done."));
}

#[test]
fn live_deployment_output_selects_deployment_block() {
    let template = "\
Perform the work.
`{{ output }}` = `branch` means push to a feature branch.
`{{ output }}` = `live_deployment` means deploy to production.

Done.";
    let result = resolve_output_specific_sections(template, Some("live_deployment"));
    assert!(!result.contains("push to a feature branch"));
    assert!(result.contains("deploy to production"));
    assert!(result.contains("Perform the work."));
    assert!(result.contains("Done."));
}

#[test]
fn none_output_strips_all_output_blocks() {
    let template = "\
Perform the work.
`{{ output }}` = `branch` means push to a feature branch.
`{{ output }}` = `live_deployment` means deploy to production.

Done.";
    let result = resolve_output_specific_sections(template, None);
    assert!(!result.contains("push to a feature branch"));
    assert!(!result.contains("deploy to production"));
    assert!(result.contains("Perform the work."));
    assert!(result.contains("Done."));
}

#[test]
fn ordinary_text_passes_through_unchanged() {
    let template = "Line one.\nLine two.\nLine three.";
    let result = resolve_output_specific_sections(template, Some("branch"));
    assert_eq!(result, template);
}

#[test]
fn unmatched_mode_strips_block() {
    let template = "\
Start.
`{{ output }}` = `pr` means open a pull request.

End.";
    let result = resolve_output_specific_sections(template, Some("branch"));
    assert!(!result.contains("open a pull request"));
    assert!(result.contains("Start."));
    assert!(result.contains("End."));
}

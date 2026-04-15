use std::io::Cursor;

use crate::app::AppError;
use crate::domain::execution_plan_edit::CascadeInfo;
use crate::write_dispatch::helpers;

#[test]
fn plan_cascade_prompt_renders_summary_and_accepts_yes() {
    let cascade = CascadeInfo {
        affected_knot_ids: vec!["knots-a".to_string(), "knots-b".to_string()],
        step_count: 2,
    };
    let mut output = Vec::new();
    let mut input = Cursor::new("yes\n");
    let accepted = helpers::plan_cascade_prompt(
        &mut output,
        &mut input,
        "removing wave 2 from knots-1",
        &cascade,
    )
    .expect("prompt should succeed");
    assert!(accepted);
    let rendered = String::from_utf8(output).expect("utf8");
    assert!(rendered.contains("cascade delete 2 step(s)"));
    assert!(rendered.contains("knots-a, knots-b"));
}

#[test]
fn plan_cascade_prompt_rejects_no_and_skips_empty_cascade() {
    let empty = CascadeInfo {
        affected_knot_ids: Vec::new(),
        step_count: 0,
    };
    assert!(!crate::write_dispatch::execute::execute_plan_ops::requires_confirmation(&empty));

    let cascade = CascadeInfo {
        affected_knot_ids: vec!["knots-a".to_string()],
        step_count: 1,
    };
    let mut output = Vec::new();
    let mut input = Cursor::new("no\n");
    let accepted = helpers::plan_cascade_prompt(
        &mut output,
        &mut input,
        "removing wave 2 from knots-1",
        &cascade,
    )
    .expect("prompt should succeed");
    assert!(!accepted);
}

#[test]
fn plan_cascade_prompt_omits_knot_ids_when_none_are_present() {
    let cascade = CascadeInfo {
        affected_knot_ids: Vec::new(),
        step_count: 2,
    };
    let mut output = Vec::new();
    let mut input = Cursor::new("yes\n");
    let accepted = helpers::plan_cascade_prompt(
        &mut output,
        &mut input,
        "removing wave 2 from knots-1",
        &cascade,
    )
    .expect("prompt should succeed");
    assert!(accepted);
    let rendered = String::from_utf8(output).expect("utf8");
    assert!(rendered.contains("affect 0 knot id(s)"));
    assert!(!rendered.contains("\n  knots-"));
}

#[test]
fn confirm_plan_cascade_requires_tty_without_force() {
    let cascade = CascadeInfo {
        affected_knot_ids: vec!["knots-a".to_string()],
        step_count: 1,
    };
    let err = crate::write_dispatch::execute::execute_plan_ops::confirm_plan_cascade(
        false,
        "removing step 1 from wave 1 in knots-1",
        &cascade,
    )
    .expect_err("non-tty confirmation should fail");
    assert!(matches!(err, AppError::InvalidArgument(_)));
}

#[test]
fn confirm_plan_cascade_accepts_force_without_tty() {
    let cascade = CascadeInfo {
        affected_knot_ids: vec!["knots-a".to_string()],
        step_count: 1,
    };
    crate::write_dispatch::execute::execute_plan_ops::confirm_plan_cascade(
        true,
        "removing step 1 from wave 1 in knots-1",
        &cascade,
    )
    .expect("force should bypass interactive confirmation");
}

#[test]
fn requires_confirmation_when_only_knot_ids_are_present() {
    let cascade = CascadeInfo {
        affected_knot_ids: vec!["knots-a".to_string()],
        step_count: 0,
    };
    assert!(crate::write_dispatch::execute::execute_plan_ops::requires_confirmation(&cascade));
}

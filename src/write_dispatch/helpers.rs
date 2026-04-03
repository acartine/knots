use std::str::FromStr;
use std::{io, io::BufRead, io::IsTerminal, io::Write};

use crate::app::{App, AppError, GateDecision};
use crate::dispatch::knot_ref;
use crate::domain::gate::{parse_failure_mode_spec, GateData, GateOwnerKind};
use crate::domain::knot_type::KnotType;
use crate::domain::state::KnotState;
use crate::ui;

const CLAIM_ONLY_LEASE_BINDING: &str = "lease binding is only allowed during claim operations";

pub(crate) fn resolve_lease_agent_info(
    app: &App,
    knot_id: &str,
) -> Option<crate::domain::lease::AgentInfo> {
    let knot = app.show_knot(knot_id).ok()??;
    let lease_id = knot.lease_id.as_ref()?;
    let lease_knot = app.show_knot(lease_id).ok()??;
    lease_knot.lease.as_ref()?.agent_info.clone()
}

pub(crate) fn reject_non_claim_lease_binding(lease_id: Option<&str>) -> Result<(), AppError> {
    if let Some(provided_lease) = lease_id {
        return Err(AppError::InvalidArgument(format!(
            "{CLAIM_ONLY_LEASE_BINDING}: caller provided '{provided_lease}'"
        )));
    }
    Ok(())
}

pub(crate) fn validate_non_claim_lease(
    knot: &crate::app::KnotView,
    lease_id: Option<&str>,
) -> Result<(), AppError> {
    let Some(provided_lease) = lease_id else {
        return Ok(());
    };

    match knot.lease_id.as_deref() {
        Some(knot_lease) if knot_lease == provided_lease => Ok(()),
        Some(knot_lease) => Err(AppError::InvalidArgument(format!(
            "lease mismatch: knot has '{knot_lease}', caller provided '{provided_lease}'"
        ))),
        None => Err(AppError::InvalidArgument(format!(
            "knot has no active lease but caller provided \
             '{provided_lease}'; {CLAIM_ONLY_LEASE_BINDING}"
        ))),
    }
}

pub(crate) fn execute_with_terminal_cascade_prompt<T, F>(
    preapproved: bool,
    mut action: F,
) -> Result<T, AppError>
where
    F: FnMut(bool) -> Result<T, AppError>,
{
    let mut approved = preapproved;
    loop {
        match action(approved) {
            Ok(value) => return Ok(value),
            Err(AppError::TerminalCascadeApprovalRequired {
                knot_id,
                target_state,
                descendants,
            }) if !approved => {
                if !io::stdin().is_terminal() {
                    return Err(AppError::TerminalCascadeApprovalRequired {
                        knot_id,
                        target_state,
                        descendants,
                    });
                }
                if prompt_for_terminal_cascade_approval(&knot_id, &target_state, &descendants)? {
                    approved = true;
                    continue;
                }
                return Err(AppError::InvalidArgument(
                    "terminal cascade cancelled; no changes written".to_string(),
                ));
            }
            Err(err) => return Err(err),
        }
    }
}

fn prompt_for_terminal_cascade_approval(
    knot_id: &str,
    target_state: &str,
    descendants: &[crate::state_hierarchy::HierarchyKnot],
) -> Result<bool, AppError> {
    let mut stderr = io::stderr();
    let mut stdin = io::stdin().lock();
    terminal_cascade_prompt(&mut stderr, &mut stdin, knot_id, target_state, descendants)
}

pub(super) fn terminal_cascade_prompt<W: Write, R: BufRead>(
    writer: &mut W,
    reader: &mut R,
    knot_id: &str,
    target_state: &str,
    descendants: &[crate::state_hierarchy::HierarchyKnot],
) -> Result<bool, AppError> {
    writeln!(
        writer,
        "moving '{}' to '{}' will also move descendant knots \
         to that terminal state:",
        knot_id, target_state
    )?;
    writeln!(
        writer,
        "  {}",
        crate::state_hierarchy::format_hierarchy_knots(descendants)
    )?;
    write!(writer, "continue? [y/N]: ")?;
    writer.flush()?;

    let mut input = String::new();
    reader.read_line(&mut input)?;
    Ok(is_terminal_cascade_approval(&input))
}

pub(super) fn is_terminal_cascade_approval(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

pub(crate) fn format_json(value: &serde_json::Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("queued json serialization should succeed")
    )
}

pub(crate) fn parse_gate_decision(raw: &str) -> Result<GateDecision, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "yes" | "pass" => Ok(GateDecision::Yes),
        "no" | "fail" => Ok(GateDecision::No),
        _ => Err(AppError::InvalidArgument(
            "--decision must be one of: yes, no".to_string(),
        )),
    }
}

pub(crate) fn parse_knot_type_arg(raw: Option<&str>) -> Result<KnotType, AppError> {
    raw.unwrap_or("work")
        .parse::<KnotType>()
        .map_err(|err| AppError::InvalidArgument(err.to_string()))
}

pub(crate) fn parse_gate_owner_kind_arg(
    raw: Option<&str>,
) -> Result<Option<GateOwnerKind>, AppError> {
    raw.map(|value| {
        value
            .parse::<GateOwnerKind>()
            .map_err(|err| AppError::InvalidArgument(err.to_string()))
    })
    .transpose()
}

pub(crate) fn parse_gate_failure_modes_option(
    raw_specs: &[String],
) -> Result<Option<std::collections::BTreeMap<String, Vec<String>>>, AppError> {
    if raw_specs.is_empty() {
        return Ok(None);
    }
    let mut failure_modes = std::collections::BTreeMap::new();
    for raw in raw_specs {
        let (invariant, targets) = parse_failure_mode_spec(raw)
            .map_err(|err| AppError::InvalidArgument(err.to_string()))?;
        failure_modes.insert(invariant, targets);
    }
    Ok(Some(failure_modes))
}

pub(crate) fn parse_gate_data_args(
    owner_kind: Option<&str>,
    raw_failure_modes: &[String],
    knot_type: KnotType,
) -> Result<GateData, AppError> {
    let owner_kind = parse_gate_owner_kind_arg(owner_kind)?;
    let failure_modes = parse_gate_failure_modes_option(raw_failure_modes)?.unwrap_or_default();
    if knot_type != KnotType::Gate && (owner_kind.is_some() || !failure_modes.is_empty()) {
        return Err(AppError::InvalidArgument(
            "gate owner/failure mode fields require knot type 'gate'".to_string(),
        ));
    }
    Ok(GateData {
        owner_kind: owner_kind.unwrap_or_default(),
        failure_modes,
    })
}

pub(crate) fn normalize_expected_state(raw: &str) -> String {
    let trimmed = raw.trim();
    KnotState::from_str(trimmed)
        .map(|state| state.as_str().to_string())
        .unwrap_or_else(|_| trimmed.to_ascii_lowercase().replace('-', "_"))
}

pub(crate) fn format_next_output(
    knot: &crate::app::KnotView,
    previous_state: &str,
    owner_kind: Option<&str>,
    json: bool,
) -> String {
    if json {
        let result = serde_json::json!({
            "id": &knot.id,
            "previous_state": previous_state,
            "state": &knot.state,
            "owner_kind": owner_kind,
        });
        return format_json(&result);
    }
    let palette = ui::Palette::auto();
    let owner_suffix = owner_kind
        .map(|kind| format!(" (owner: {kind})"))
        .unwrap_or_default();
    format!(
        "updated {} -> {}{}\n",
        palette.id(&knot_ref(knot)),
        palette.state(&knot.state),
        owner_suffix,
    )
}

pub(crate) fn format_rollback_output(
    knot: &crate::app::KnotView,
    target_state: &str,
    owner_kind: Option<&str>,
    reason: &str,
    dry_run: bool,
) -> String {
    let palette = ui::Palette::auto();
    let owner_suffix = owner_kind
        .map(|kind| format!(" (owner: {kind})"))
        .unwrap_or_default();
    let verb = if dry_run {
        "would roll back"
    } else {
        "rolled back"
    };
    format!(
        "{verb} {} -> {}{} ({reason})\n",
        palette.id(&knot_ref(knot)),
        palette.state(target_state),
        owner_suffix,
    )
}

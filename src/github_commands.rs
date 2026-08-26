use crate::cli::CompactArgs;
use crate::{app, print_json};

pub(crate) fn run_canary(args: CompactArgs) -> Result<(), app::AppError> {
    let required = |value: Option<String>, name: &str| {
        value.ok_or_else(|| app::AppError::InvalidArgument(format!("{name} is required")))
    };
    let payload_path = args
        .event_payload
        .ok_or_else(|| app::AppError::InvalidArgument("--event-payload is required".to_string()))?;
    let payload = std::fs::read(payload_path)?;
    let signed = crate::compaction::create_github_canary_submission(
        &required(args.repository_id, "--repository-id")?,
        &required(args.proposal_oid, "--proposal-oid")?,
        &required(args.event_id, "--event-id")?,
        &required(args.event_path, "--event-path")?,
        &payload,
    )
    .map_err(app::AppError::InvalidArgument)?;
    print_json(&signed);
    Ok(())
}

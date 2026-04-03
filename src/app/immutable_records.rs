use serde_json::json;

use crate::domain::immutable::ensure_append_only_records;
use crate::domain::lease::{LeaseReference, LeaseReferenceError};
use crate::domain::metadata::MetadataEntry;
use crate::domain::step_history::StepRecord;

use super::error::AppError;

pub(crate) fn lease_ref_from_lease_id(
    lease_id: Option<&str>,
) -> Result<Option<LeaseReference>, AppError> {
    LeaseReference::from_option(lease_id).map_err(invalid_lease_reference)
}

pub(crate) fn ensure_append_only_metadata(
    existing: &[MetadataEntry],
    next: &[MetadataEntry],
    kind: &str,
) -> Result<(), AppError> {
    ensure_append_only_records(existing, next, kind).map_err(AppError::ImmutableRecordViolation)
}

pub(crate) fn ensure_append_only_step_history(
    existing: &[StepRecord],
    next: &[StepRecord],
) -> Result<(), AppError> {
    ensure_append_only_records(existing, next, "step history")
        .map_err(AppError::ImmutableRecordViolation)
}

pub(crate) fn metadata_entry_event_data(entry: &MetadataEntry) -> serde_json::Value {
    json!({
        "entry_id": entry.entry_id,
        "content": entry.content,
        "username": entry.username,
        "datetime": entry.datetime,
        "agentname": entry.agentname,
        "model": entry.model,
        "version": entry.version,
        "lease_ref": entry.lease_ref,
    })
}

fn invalid_lease_reference(err: LeaseReferenceError) -> AppError {
    AppError::InvalidArgument(err.to_string())
}

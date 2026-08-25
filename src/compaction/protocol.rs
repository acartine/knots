use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::{CompactionManifest, V2RefLayout, WriterHead};

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum ControlKind {
    Activation,
    Recovery { rollback_of_epoch: u64 },
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlRecord {
    pub schema_version: u32,
    pub epoch: u64,
    pub previous_control_head: Option<String>,
    pub active_generation_id: String,
    pub active_generation_commit: String,
    pub archive_ref: String,
    pub acknowledged_writer_heads: Vec<WriterHead>,
    pub protection_policy_sha256: String,
    pub action: ControlKind,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ProtocolError {
    GenerationAlreadyStaged,
    NoPreparedGeneration,
    PreparedGenerationMismatch,
    ActiveHeadMoved,
    EpochNotMonotonic,
    WriterHeadRegressed,
    UnknownGeneration,
    InvalidRecovery,
    InvalidControlRecord,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "compaction protocol error: {self:?}")
    }
}

impl Error for ProtocolError {}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct ProtocolModel {
    control_head: Option<String>,
    active: Option<ControlRecord>,
    prepared_generation: Option<String>,
    archived_generations: BTreeSet<String>,
}

impl ProtocolModel {
    pub(crate) fn with_active(control_head: impl Into<String>, record: ControlRecord) -> Self {
        let mut archived_generations = BTreeSet::new();
        archived_generations.insert(record.active_generation_id.clone());
        Self {
            control_head: Some(control_head.into()),
            active: Some(record),
            prepared_generation: None,
            archived_generations,
        }
    }

    pub(crate) fn active(&self) -> Option<&ControlRecord> {
        self.active.as_ref()
    }

    pub(crate) fn control_head(&self) -> Option<&str> {
        self.control_head.as_deref()
    }

    pub(crate) fn prepared_generation(&self) -> Option<&str> {
        self.prepared_generation.as_deref()
    }

    pub(crate) fn prepare(&mut self, manifest: &CompactionManifest) -> Result<(), ProtocolError> {
        if self.prepared_generation.is_some() {
            return Err(ProtocolError::GenerationAlreadyStaged);
        }
        let current_epoch = self.active.as_ref().map_or(0, |record| record.epoch);
        if manifest.predecessor_control_epoch != current_epoch {
            return Err(ProtocolError::EpochNotMonotonic);
        }
        self.archived_generations
            .insert(manifest.generation_id.clone());
        self.prepared_generation = Some(manifest.generation_id.clone());
        Ok(())
    }

    pub(crate) fn interrupt_prepare(&mut self) {
        self.prepared_generation = None;
    }

    pub(crate) fn activate(
        &mut self,
        expected_control_head: Option<&str>,
        new_control_head: &str,
        record: ControlRecord,
    ) -> Result<(), ProtocolError> {
        self.validate_common(expected_control_head, &record)?;
        if self.prepared_generation.as_deref() != Some(&record.active_generation_id) {
            return Err(if self.prepared_generation.is_some() {
                ProtocolError::PreparedGenerationMismatch
            } else {
                ProtocolError::NoPreparedGeneration
            });
        }
        if !matches!(record.action, ControlKind::Activation) {
            return Err(ProtocolError::InvalidRecovery);
        }
        self.commit_control(new_control_head, record);
        self.prepared_generation = None;
        Ok(())
    }

    pub(crate) fn recover(
        &mut self,
        expected_control_head: &str,
        new_control_head: &str,
        record: ControlRecord,
    ) -> Result<(), ProtocolError> {
        self.validate_common(Some(expected_control_head), &record)?;
        let Some(active) = self.active.as_ref() else {
            return Err(ProtocolError::InvalidRecovery);
        };
        let ControlKind::Recovery { rollback_of_epoch } = record.action else {
            return Err(ProtocolError::InvalidRecovery);
        };
        if rollback_of_epoch != active.epoch
            || !self
                .archived_generations
                .contains(&record.active_generation_id)
        {
            return Err(ProtocolError::InvalidRecovery);
        }
        self.commit_control(new_control_head, record);
        self.prepared_generation = None;
        Ok(())
    }

    fn validate_common(
        &self,
        expected_control_head: Option<&str>,
        record: &ControlRecord,
    ) -> Result<(), ProtocolError> {
        validate_record_shape(record)?;
        if self.control_head.as_deref() != expected_control_head
            || record.previous_control_head.as_deref() != expected_control_head
        {
            return Err(ProtocolError::ActiveHeadMoved);
        }
        let next_epoch = self.active.as_ref().map_or(1, |active| active.epoch + 1);
        if record.epoch != next_epoch {
            return Err(ProtocolError::EpochNotMonotonic);
        }
        if !self
            .archived_generations
            .contains(&record.active_generation_id)
        {
            return Err(ProtocolError::UnknownGeneration);
        }
        if let Some(active) = &self.active {
            ensure_vector_dominates(
                &record.acknowledged_writer_heads,
                &active.acknowledged_writer_heads,
            )?;
        }
        Ok(())
    }

    fn commit_control(&mut self, new_control_head: &str, record: ControlRecord) {
        self.control_head = Some(new_control_head.to_string());
        self.active = Some(record);
    }
}

fn validate_record_shape(record: &ControlRecord) -> Result<(), ProtocolError> {
    let layout = V2RefLayout::default();
    if record.schema_version != 1
        || !valid_object_id(&record.active_generation_commit)
        || record.archive_ref != layout.archive(&record.active_generation_id)
        || !valid_digest(&record.protection_policy_sha256)
    {
        return Err(ProtocolError::InvalidControlRecord);
    }
    let mut writers = HashSet::new();
    for head in &record.acknowledged_writer_heads {
        if head.writer_id.is_empty()
            || head.inbox_ref != layout.inbox(&head.writer_id)
            || !valid_object_id(&head.commit)
            || !writers.insert(&head.writer_id)
        {
            return Err(ProtocolError::InvalidControlRecord);
        }
    }
    Ok(())
}

fn ensure_vector_dominates(next: &[WriterHead], prior: &[WriterHead]) -> Result<(), ProtocolError> {
    let next: BTreeMap<&str, (&str, u64)> = next
        .iter()
        .map(|head| {
            (
                head.writer_id.as_str(),
                (head.commit.as_str(), head.sequence),
            )
        })
        .collect();
    for head in prior {
        let Some((commit, sequence)) = next.get(head.writer_id.as_str()) else {
            return Err(ProtocolError::WriterHeadRegressed);
        };
        if *sequence < head.sequence || (*sequence == head.sequence && *commit != head.commit) {
            return Err(ProtocolError::WriterHeadRegressed);
        }
    }
    Ok(())
}

fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

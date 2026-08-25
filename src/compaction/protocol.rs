use std::error::Error;
use std::fmt;

use super::{CompactionManifest, GenerationState};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ProtocolError {
    WrongState,
    GenerationAlreadyStaged,
    NoPreparedGeneration,
    PreparedGenerationMismatch,
    ActiveHeadMoved,
    UnknownRollbackTarget,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "compaction protocol error: {self:?}")
    }
}

impl Error for ProtocolError {}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct ProtocolModel {
    active_generation: Option<String>,
    prepared_generation: Option<String>,
    verified_generations: Vec<String>,
}

impl ProtocolModel {
    pub(crate) fn with_active(generation_id: impl Into<String>) -> Self {
        let generation_id = generation_id.into();
        Self {
            active_generation: Some(generation_id.clone()),
            prepared_generation: None,
            verified_generations: vec![generation_id],
        }
    }

    pub(crate) fn active_generation(&self) -> Option<&str> {
        self.active_generation.as_deref()
    }

    pub(crate) fn prepared_generation(&self) -> Option<&str> {
        self.prepared_generation.as_deref()
    }

    pub(crate) fn prepare(&mut self, manifest: &CompactionManifest) -> Result<(), ProtocolError> {
        if manifest.state != GenerationState::Prepared {
            return Err(ProtocolError::WrongState);
        }
        if self.prepared_generation.is_some() {
            return Err(ProtocolError::GenerationAlreadyStaged);
        }
        self.prepared_generation = Some(manifest.generation_id.clone());
        Ok(())
    }

    pub(crate) fn interrupt_prepare(&mut self) {
        self.prepared_generation = None;
    }

    pub(crate) fn activate(
        &mut self,
        expected_active: Option<&str>,
        manifest: &CompactionManifest,
    ) -> Result<(), ProtocolError> {
        if manifest.state != GenerationState::Active {
            return Err(ProtocolError::WrongState);
        }
        if self.active_generation.as_deref() != expected_active {
            return Err(ProtocolError::ActiveHeadMoved);
        }
        if self.prepared_generation.as_deref() != Some(manifest.generation_id.as_str()) {
            return Err(if self.prepared_generation.is_some() {
                ProtocolError::PreparedGenerationMismatch
            } else {
                ProtocolError::NoPreparedGeneration
            });
        }
        let generation_id = manifest.generation_id.clone();
        self.active_generation = Some(generation_id.clone());
        self.prepared_generation = None;
        if !self.verified_generations.contains(&generation_id) {
            self.verified_generations.push(generation_id);
        }
        Ok(())
    }

    pub(crate) fn rollback(
        &mut self,
        expected_active: &str,
        target: &str,
    ) -> Result<(), ProtocolError> {
        if self.active_generation.as_deref() != Some(expected_active) {
            return Err(ProtocolError::ActiveHeadMoved);
        }
        let Some(active_position) = self
            .verified_generations
            .iter()
            .position(|value| value == expected_active)
        else {
            return Err(ProtocolError::UnknownRollbackTarget);
        };
        if active_position == 0 || self.verified_generations[active_position - 1] != target {
            return Err(ProtocolError::UnknownRollbackTarget);
        }
        self.active_generation = Some(target.to_string());
        self.prepared_generation = None;
        Ok(())
    }
}

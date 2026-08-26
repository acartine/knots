// This module is the shared contract for the publication and bootstrap follow-on knots.
#![allow(dead_code, unused_imports)]

mod checkpoint;
mod checkpoint_store;
mod inbox;
mod integrator;
mod manifest;
mod pack;
mod projections;
mod protection;
mod protocol;
mod publication;
mod refs;
mod validation;

pub(crate) use checkpoint::{
    install_checkpoint, prepare_checkpoint, CheckpointError, CheckpointInput, CheckpointSummary,
    PreparedCheckpoint,
};
pub(crate) use checkpoint_store::drain_generation_quarantine;
pub(crate) use inbox::{
    validate_inbox, InboxBundle, InboxCandidate, InboxEntry, InboxError, ValidatedInbox,
    INBOX_DATA_ROOT,
};
pub(crate) use integrator::{
    integrate_generation, ConflictRecord, GenerationArtifacts, GenerationBase, GenerationInput,
    IntegrationError, ProjectionReplay,
};
pub(crate) use manifest::{
    CompactionManifest, Compatibility, EventIndexEntry, EventPack, SnapshotDescriptor, SnapshotSet,
    SourceCheckpoint, WriterHead, ARCHIVE_REF_PREFIX, CANONICAL_ROOT, CONTROL_REF,
    INBOX_REF_PREFIX, PROTOCOL_VERSION,
};
#[cfg(test)]
pub(crate) use pack::fixture_pack;
pub(crate) use pack::{build_pack, BuiltPack, PackError, RawEvent};
#[cfg(test)]
pub(crate) use projections::fixture_projection_bytes;
pub(crate) use projections::{
    canonical_projection_bytes, projection_descriptor, CanonicalProjections, ProjectionError,
};
pub(crate) use protection::{
    validate_protection, ProtectionError, ProtectionMarker, ProviderProtectionFacts,
    ValidatedProtection,
};
pub(crate) use protocol::{ControlKind, ControlRecord, ProtocolError, ProtocolModel};
pub(crate) use publication::{GenerationPublisher, PublicationTarget};
pub(crate) use refs::{V2RefLayout, CANONICAL_REF_PREFIX, LEGACY_REF};
pub(crate) use validation::{
    parse_and_validate, validate, SourceFacts, ValidationContext, ValidationError,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod protocol_coverage_tests;

#[cfg(test)]
mod integrator_tests;

#[cfg(test)]
mod checkpoint_tests;

#[cfg(test)]
mod checkpoint_state_tests;

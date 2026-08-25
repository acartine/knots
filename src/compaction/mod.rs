// This module is the shared contract for the publication and bootstrap follow-on knots.
#![allow(dead_code, unused_imports)]

mod manifest;
mod protection;
mod protocol;
mod publication;
mod refs;
mod validation;

pub(crate) use manifest::{
    CompactionManifest, Compatibility, EventIndexEntry, EventPack, SnapshotDescriptor, SnapshotSet,
    SourceCheckpoint, WriterHead, ARCHIVE_REF_PREFIX, CANONICAL_ROOT, CONTROL_REF,
    INBOX_REF_PREFIX, PROTOCOL_VERSION,
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

// This module is the shared contract for the publication and bootstrap follow-on knots.
#![allow(dead_code, unused_imports)]

mod manifest;
mod protocol;
mod validation;

pub(crate) use manifest::{
    CompactionManifest, Compatibility, GenerationState, Retention, SnapshotDescriptor, SnapshotSet,
    SourceCheckpoint, PROTOCOL_VERSION,
};
pub(crate) use protocol::{ProtocolError, ProtocolModel};
pub(crate) use validation::{
    parse_and_validate, validate, SourceFacts, ValidationContext, ValidationError,
};

#[cfg(test)]
mod tests;

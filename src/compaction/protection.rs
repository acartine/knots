use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{V2RefLayout, CONTROL_REF};

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtectionMarker {
    pub schema_version: u32,
    pub repository_id: String,
    pub policy_id: String,
    pub policy_sha256: String,
    pub integrator_id: String,
    pub control_ref: String,
    pub canonical_prefix: String,
    pub archive_prefix: String,
    pub inbox_prefix: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderProtectionFacts<'a> {
    pub repository_id: &'a str,
    pub policy_id: &'a str,
    pub policy_bytes: &'a [u8],
    pub integrator_id: &'a str,
    pub control_ref: &'a str,
    pub canonical_prefix: &'a str,
    pub archive_prefix: &'a str,
    pub inbox_prefix: &'a str,
    pub control_head: Option<&'a str>,
    pub control_protected: bool,
    pub canonical_create_only: bool,
    pub archives_create_only: bool,
    pub inboxes_writer_scoped: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ValidatedProtection {
    repository_id: String,
    integrator_id: String,
    control_head: Option<String>,
}

impl ValidatedProtection {
    pub(crate) fn repository_id(&self) -> &str {
        &self.repository_id
    }

    pub(crate) fn integrator_id(&self) -> &str {
        &self.integrator_id
    }

    pub(crate) fn control_head(&self) -> Option<&str> {
        self.control_head.as_deref()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum ProtectionError {
    Unavailable,
    InvalidMarker,
    ProviderMismatch,
    PolicyNotEnforced,
}

impl fmt::Display for ProtectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol-v2 provider protection unavailable: {self:?}")
    }
}

impl Error for ProtectionError {}

pub(crate) fn validate_protection(
    marker: Option<&ProtectionMarker>,
    facts: Option<&ProviderProtectionFacts<'_>>,
) -> Result<ValidatedProtection, ProtectionError> {
    let marker = marker.ok_or(ProtectionError::Unavailable)?;
    let facts = facts.ok_or(ProtectionError::Unavailable)?;
    validate_marker_shape(marker)?;
    if !provider_matches(marker, facts) {
        return Err(ProtectionError::ProviderMismatch);
    }
    if !facts.control_protected
        || !facts.canonical_create_only
        || !facts.archives_create_only
        || !facts.inboxes_writer_scoped
    {
        return Err(ProtectionError::PolicyNotEnforced);
    }
    Ok(ValidatedProtection {
        repository_id: marker.repository_id.clone(),
        integrator_id: marker.integrator_id.clone(),
        control_head: facts.control_head.map(str::to_string),
    })
}

fn validate_marker_shape(marker: &ProtectionMarker) -> Result<(), ProtectionError> {
    let layout = V2RefLayout::default();
    let valid = marker.schema_version == 1
        && !marker.repository_id.is_empty()
        && !marker.policy_id.is_empty()
        && valid_digest(&marker.policy_sha256)
        && !marker.integrator_id.is_empty()
        && marker.control_ref == CONTROL_REF
        && marker.canonical_prefix == layout.canonical_prefix
        && marker.archive_prefix == layout.archive_prefix
        && marker.inbox_prefix == layout.inbox_prefix;
    if valid {
        Ok(())
    } else {
        Err(ProtectionError::InvalidMarker)
    }
}

fn provider_matches(marker: &ProtectionMarker, facts: &ProviderProtectionFacts<'_>) -> bool {
    marker.repository_id == facts.repository_id
        && marker.policy_id == facts.policy_id
        && marker.policy_sha256 == format!("{:x}", Sha256::digest(facts.policy_bytes))
        && marker.integrator_id == facts.integrator_id
        && marker.control_ref == facts.control_ref
        && marker.canonical_prefix == facts.canonical_prefix
        && marker.archive_prefix == facts.archive_prefix
        && marker.inbox_prefix == facts.inbox_prefix
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

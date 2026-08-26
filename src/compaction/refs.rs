use super::{ARCHIVE_REF_PREFIX, CONTROL_REF, INBOX_REF_PREFIX};

pub(crate) const LEGACY_REF: &str = "refs/heads/knots";
pub(crate) const CANONICAL_REF_PREFIX: &str = "refs/heads/knots-v2-canonical/";
pub(crate) const PROPOSAL_REF_PREFIX: &str = "refs/heads/knots-v2-proposals/";

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct V2RefLayout {
    pub legacy: &'static str,
    pub control: &'static str,
    pub canonical_prefix: &'static str,
    pub archive_prefix: &'static str,
    pub inbox_prefix: &'static str,
    pub proposal_prefix: &'static str,
}

impl Default for V2RefLayout {
    fn default() -> Self {
        Self {
            legacy: LEGACY_REF,
            control: CONTROL_REF,
            canonical_prefix: CANONICAL_REF_PREFIX,
            archive_prefix: ARCHIVE_REF_PREFIX,
            inbox_prefix: INBOX_REF_PREFIX,
            proposal_prefix: PROPOSAL_REF_PREFIX,
        }
    }
}

impl V2RefLayout {
    pub(crate) fn canonical(&self, generation_id: &str) -> String {
        format!("{}{generation_id}", self.canonical_prefix)
    }

    pub(crate) fn archive(&self, generation_id: &str) -> String {
        format!("{}{generation_id}", self.archive_prefix)
    }

    pub(crate) fn inbox(&self, writer_id: &str) -> String {
        format!("{}{writer_id}", self.inbox_prefix)
    }

    pub(crate) fn proposal(&self, writer_id: &str, sequence: u64) -> String {
        format!("{}{writer_id}/{sequence}", self.proposal_prefix)
    }
}

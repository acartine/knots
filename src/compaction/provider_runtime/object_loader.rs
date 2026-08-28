use std::collections::BTreeMap;
use std::path::{Component, Path};

use super::{ProviderRefLayout, RuntimeError};

const INBOX_ROOT: &str = ".knots/v2/inbox";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ObjectKind {
    Blob,
    Tree,
    Symlink,
    Submodule,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TreeObject {
    pub path: String,
    pub kind: ObjectKind,
    pub oid: String,
    pub bytes: Vec<u8>,
}

pub(crate) trait ExactObjectReader {
    fn resolve_ref(&self, remote_ref: &str) -> Result<Option<String>, RuntimeError>;

    /// Read one commit tree by exact OID. Implementations must use object database reads,
    /// never checkout, hooks, filters, or worktree materialization.
    fn read_commit_tree(&self, commit_oid: &str) -> Result<Vec<TreeObject>, RuntimeError>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct LoadedInbox {
    pub proposal_ref: String,
    pub commit_oid: String,
    pub objects: BTreeMap<String, Vec<u8>>,
}

pub(crate) fn load_untrusted_inbox(
    reader: &impl ExactObjectReader,
    refs: &ProviderRefLayout,
    proposal_ref: &str,
    expected_oid: &str,
) -> Result<LoadedInbox, RuntimeError> {
    if !proposal_ref.starts_with(&refs.submission_prefix) || !valid_oid(expected_oid) {
        return Err(RuntimeError::InvalidObject("invalid proposal identity"));
    }
    if reader.resolve_ref(proposal_ref)?.as_deref() != Some(expected_oid) {
        return Err(RuntimeError::RefDrift);
    }
    let tree = reader.read_commit_tree(expected_oid)?;
    let mut objects = BTreeMap::new();
    for object in tree {
        validate_object(&object)?;
        if objects.insert(object.path, object.bytes).is_some() {
            return Err(RuntimeError::InvalidObject("duplicate inbox path"));
        }
    }
    if reader.resolve_ref(proposal_ref)?.as_deref() != Some(expected_oid) {
        return Err(RuntimeError::RefDrift);
    }
    if !objects.contains_key(".knots/v2/inbox/submission.json")
        || !objects.contains_key(".knots/v2/inbox/bundle.json")
    {
        return Err(RuntimeError::InvalidObject("inbox metadata is incomplete"));
    }
    Ok(LoadedInbox {
        proposal_ref: proposal_ref.to_string(),
        commit_oid: expected_oid.to_string(),
        objects,
    })
}

fn validate_object(object: &TreeObject) -> Result<(), RuntimeError> {
    if object.kind != ObjectKind::Blob || !valid_oid(&object.oid) {
        return Err(RuntimeError::InvalidObject(
            "inbox contains a non-blob object",
        ));
    }
    if object.path.contains('\\') || !valid_path(&object.path) {
        return Err(RuntimeError::InvalidObject(
            "inbox path escapes its data root",
        ));
    }
    Ok(())
}

fn valid_path(value: &str) -> bool {
    let path = Path::new(value);
    if !path.starts_with(INBOX_ROOT)
        || path == Path::new(INBOX_ROOT)
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return false;
    }
    let Some(relative) = path.strip_prefix(INBOX_ROOT).ok() else {
        return false;
    };
    let text = relative.to_string_lossy();
    text == "submission.json"
        || text == "bundle.json"
        || ((text.starts_with("events/") || text.starts_with("index/"))
            && path
                .extension()
                .is_some_and(|extension| extension == "json"))
}

pub(super) fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

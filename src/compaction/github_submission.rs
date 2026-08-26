use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{assign_pending_outbox, ensure_writer_epoch, record_outbox_event};

use super::submission::decode_array;
use super::{
    verify_submission, RegistrationAuthority, SignedSubmission, SubmissionCandidate, V2RefLayout,
    INBOX_DATA_ROOT,
};

#[derive(Debug, Clone)]
pub(crate) struct GitHubProposalInput {
    pub git_dir: PathBuf,
    pub repository_id: String,
    pub proposal_ref: String,
    pub proposal_oid: String,
    pub signed_submission: PathBuf,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct GitHubPromotionPlan {
    pub proposal_ref: String,
    pub proposal_oid: String,
    pub inbox_ref: String,
    pub expected_old_oid: Option<String>,
    pub writer_registry_ref: String,
    pub expected_registry_oid: Option<String>,
    pub writer_id: String,
    pub public_key: String,
    pub parent_writer_id: Option<String>,
    pub purpose: String,
    pub sequence: u64,
    pub signed_writer_verified: bool,
    pub authority_constructed: bool,
}

pub(crate) fn verify_github_proposal(
    input: &GitHubProposalInput,
) -> Result<GitHubPromotionPlan, String> {
    validate_oid(&input.proposal_oid)?;
    let submission: SignedSubmission =
        serde_json::from_slice(&fs::read(&input.signed_submission).map_err(error)?)
            .map_err(error)?;
    validate_untrusted_ref_shape(&submission)?;
    let objects = read_exact_objects(&input.git_dir, &input.proposal_oid, &submission)?;
    let current = remote_ref(&input.git_dir, &submission.target_ref)?;
    let registry_ref = writer_registry_ref(&submission.bundle.writer_id);
    let registry_oid = remote_ref(&input.git_dir, &registry_ref)?;
    if let Some(oid) = registry_oid.as_deref() {
        fetch_exact(&input.git_dir, oid)?;
    }
    validate_parent(&input.git_dir, &input.proposal_oid, current.as_deref())?;
    let authority = authority_from_registry(
        &input.git_dir,
        &submission,
        current.as_deref(),
        registry_oid.as_deref(),
    )?;
    let request = verify_submission(
        &SubmissionCandidate {
            repository_id: input.repository_id.clone(),
            proposal_ref: input.proposal_ref.clone(),
            observed_oid: input.proposal_oid.clone(),
            current_inbox_oid: current,
            expected_sequence: authority.expected_sequence,
            submission: serde_json::to_vec(&submission).map_err(error)?,
            objects,
        },
        authority.authority.borrowed(),
    )
    .map_err(error)?;
    Ok(GitHubPromotionPlan {
        proposal_ref: request.proposal_ref,
        proposal_oid: request.proposal_oid,
        inbox_ref: request.inbox_ref,
        expected_old_oid: request.expected_old_oid,
        writer_registry_ref: registry_ref,
        expected_registry_oid: registry_oid,
        writer_id: request.writer_id,
        public_key: submission.public_key,
        parent_writer_id: submission.parent_writer_id,
        purpose: authority.purpose,
        sequence: request.sequence,
        signed_writer_verified: true,
        authority_constructed: true,
    })
}

pub(crate) fn create_github_canary_submission(
    repository_id: &str,
    proposal_oid: &str,
    event_id: &str,
    relative_path: &str,
    payload: &[u8],
) -> Result<SignedSubmission, String> {
    validate_oid(proposal_oid)?;
    let conn = crate::db::open_connection(":memory:").map_err(error)?;
    let digest = format!("{:x}", Sha256::digest(payload));
    record_outbox_event(&conn, event_id, "events", relative_path, &digest, payload)
        .map_err(error)?;
    let writer = ensure_writer_epoch(&conn, "github-live-canary").map_err(error)?;
    let records = assign_pending_outbox(&conn, &writer, 1).map_err(error)?;
    super::sign_submission(&conn, repository_id, &writer, &records, proposal_oid, None)
        .map_err(error)
}

struct ProviderAuthority {
    authority: GitHubAuthority,
    expected_sequence: u64,
    purpose: String,
}

enum GitHubAuthority {
    Existing([u8; 32]),
    First,
    Rotation {
        parent_writer_id: String,
        parent_public_key: [u8; 32],
    },
}

impl GitHubAuthority {
    fn borrowed(&self) -> RegistrationAuthority<'_> {
        match self {
            Self::Existing(public_key) => RegistrationAuthority::Existing { public_key },
            Self::First => RegistrationAuthority::First,
            Self::Rotation {
                parent_writer_id,
                parent_public_key,
            } => RegistrationAuthority::Rotation {
                parent_writer_id,
                parent_public_key,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriterRegistry {
    schema_version: u64,
    writer_id: String,
    public_key: String,
    inbox_oid: String,
    sequence: u64,
    parent_writer_id: Option<String>,
    purpose: String,
}

fn authority_from_registry(
    git_dir: &Path,
    submission: &SignedSubmission,
    current: Option<&str>,
    registry_oid: Option<&str>,
) -> Result<ProviderAuthority, String> {
    let writer_key = public_key(submission)?;
    if fingerprint(&writer_key) != submission.bundle.writer_id {
        return Err("submission key does not match its writer fingerprint".to_string());
    }
    match (current, registry_oid) {
        (Some(current), Some(registry_oid)) => {
            let registry = read_registry(git_dir, registry_oid)?;
            validate_registry(&registry, &submission.bundle.writer_id, current)?;
            let registered_key = decode_array(&registry.public_key)
                .ok_or("protected writer registry public key is invalid")?;
            if registered_key != writer_key {
                return Err("submission key differs from protected writer registry".to_string());
            }
            let purpose = submission_purpose(submission);
            if registry.parent_writer_id != submission.parent_writer_id
                || registry.purpose != purpose
            {
                return Err("submission differs from protected writer registry lineage".to_string());
            }
            let expected_sequence = registry
                .sequence
                .checked_add(1)
                .ok_or("sequence overflow")?;
            return Ok(ProviderAuthority {
                authority: GitHubAuthority::Existing(registered_key),
                expected_sequence,
                purpose,
            });
        }
        (None, None) => {}
        _ => return Err("protected inbox and writer registry disagree".to_string()),
    }
    let Some(parent_writer) = submission.parent_writer_id.as_deref() else {
        return Ok(ProviderAuthority {
            authority: GitHubAuthority::First,
            expected_sequence: 1,
            purpose: submission_purpose(submission),
        });
    };
    let parent_ref = writer_registry_ref(parent_writer);
    let parent_oid = remote_ref(git_dir, &parent_ref)?
        .ok_or("rotation parent is absent from protected writer registry")?;
    fetch_exact(git_dir, &parent_oid)?;
    let parent = read_registry(git_dir, &parent_oid)?;
    validate_registry_identity(&parent, parent_writer)?;
    let parent_key =
        decode_array(&parent.public_key).ok_or("protected parent writer public key is invalid")?;
    Ok(ProviderAuthority {
        authority: GitHubAuthority::Rotation {
            parent_writer_id: parent_writer.to_string(),
            parent_public_key: parent_key,
        },
        expected_sequence: 1,
        purpose: submission_purpose(submission),
    })
}

fn writer_registry_ref(writer_id: &str) -> String {
    format!("refs/heads/knots-v2-writers/{writer_id}")
}

fn read_registry(git_dir: &Path, oid: &str) -> Result<WriterRegistry, String> {
    let paths = tree_paths(git_dir, oid)?;
    if paths != BTreeSet::from([".knots/v2/writer.json".to_string()]) {
        return Err("protected writer registry tree is invalid".to_string());
    }
    serde_json::from_slice(&git(
        git_dir,
        ["show", &format!("{oid}:.knots/v2/writer.json")],
    )?)
    .map_err(error)
}

fn validate_registry(
    registry: &WriterRegistry,
    writer_id: &str,
    inbox_oid: &str,
) -> Result<(), String> {
    validate_registry_identity(registry, writer_id)?;
    validate_oid(&registry.inbox_oid)?;
    if registry.inbox_oid != inbox_oid || registry.sequence == 0 {
        return Err("protected writer registry state is inconsistent".to_string());
    }
    Ok(())
}

fn validate_registry_identity(registry: &WriterRegistry, writer_id: &str) -> Result<(), String> {
    if registry.schema_version != 1 || registry.writer_id != writer_id {
        return Err("protected writer registry identity is invalid".to_string());
    }
    if !matches!(registry.purpose.as_str(), "production" | "canary") {
        return Err("protected writer registry purpose is invalid".to_string());
    }
    let key = decode_array(&registry.public_key)
        .ok_or("protected writer registry public key is invalid")?;
    if fingerprint(&key) != writer_id {
        return Err("protected writer registry fingerprint is invalid".to_string());
    }
    if let Some(parent) = registry.parent_writer_id.as_deref() {
        if parent.len() != 64 || !parent.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("protected parent writer ID is invalid".to_string());
        }
    }
    Ok(())
}

fn submission_purpose(submission: &SignedSubmission) -> String {
    if submission
        .bundle
        .entries
        .iter()
        .all(|entry| entry.event_id.starts_with("github-live-canary-"))
    {
        "canary".to_string()
    } else {
        "production".to_string()
    }
}

fn fetch_exact(git_dir: &Path, oid: &str) -> Result<(), String> {
    git(git_dir, ["fetch", "--no-tags", "origin", oid]).map(|_| ())
}

fn read_exact_objects(
    git_dir: &Path,
    oid: &str,
    submission: &SignedSubmission,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let listed = tree_paths(git_dir, oid)?;
    let expected = submission
        .bundle
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    if listed != expected {
        return Err("proposal tree contains missing or undeclared paths".to_string());
    }
    submission
        .bundle
        .entries
        .iter()
        .map(|entry| read_blob(git_dir, oid, &entry.path).map(|bytes| (entry.path.clone(), bytes)))
        .collect()
}

fn tree_paths(git_dir: &Path, oid: &str) -> Result<BTreeSet<String>, String> {
    let output = git(git_dir, ["ls-tree", "-r", "-z", "--name-only", oid])?;
    output
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| String::from_utf8(value.to_vec()).map_err(error))
        .collect()
}

fn read_blob(git_dir: &Path, oid: &str, path: &str) -> Result<Vec<u8>, String> {
    if !path.starts_with(&format!("{INBOX_DATA_ROOT}/")) {
        return Err("proposal contains a path outside the data boundary".to_string());
    }
    git(git_dir, ["show", &format!("{oid}:{path}")])
}

fn remote_ref(git_dir: &Path, remote_ref: &str) -> Result<Option<String>, String> {
    let output = git(git_dir, ["ls-remote", "--refs", "origin", remote_ref])?;
    let text = String::from_utf8(output).map_err(error)?;
    let Some(line) = text.lines().next() else {
        return Ok(None);
    };
    let oid = line
        .split_whitespace()
        .next()
        .ok_or("invalid ls-remote output")?;
    validate_oid(oid)?;
    Ok(Some(oid.to_string()))
}

fn validate_parent(git_dir: &Path, oid: &str, current: Option<&str>) -> Result<(), String> {
    let output = git(git_dir, ["rev-list", "--parents", "-n", "1", oid])?;
    let text = String::from_utf8(output).map_err(error)?;
    let parents = text.split_whitespace().skip(1).collect::<Vec<_>>();
    match (current, parents.as_slice()) {
        (None, []) => Ok(()),
        (Some(expected), [actual]) if *actual == expected => Ok(()),
        _ => Err("proposal is not an exact fast-forward of the protected inbox".to_string()),
    }
}

fn git<const N: usize>(git_dir: &Path, args: [&str; N]) -> Result<Vec<u8>, String> {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(args)
        .output()
        .map_err(error)?;
    if !status.success() {
        return Err(String::from_utf8_lossy(&stderr).trim().to_string());
    }
    Ok(stdout)
}

fn validate_untrusted_ref_shape(submission: &SignedSubmission) -> Result<(), String> {
    let layout = V2RefLayout::default();
    if submission.proposal_ref
        != layout.proposal(&submission.bundle.writer_id, submission.bundle.sequence)
        || submission.target_ref != layout.inbox(&submission.bundle.writer_id)
    {
        return Err("submission ref layout is invalid".to_string());
    }
    Ok(())
}

fn public_key(submission: &SignedSubmission) -> Result<[u8; 32], String> {
    decode_array(&submission.public_key).ok_or_else(|| "invalid public key".to_string())
}

fn fingerprint(public: &[u8; 32]) -> String {
    format!("{:x}", Sha256::digest(public))
}

fn validate_oid(value: &str) -> Result<(), String> {
    if matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("invalid Git object ID".to_string())
    }
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

#[cfg(test)]
#[path = "github_submission_tests.rs"]
mod tests;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::super::ControlRecord;
use super::object_loader::valid_oid;
use super::{
    CasOutcome, ControlMode, ControlState, ExactObjectReader, GenerationProvider,
    GenerationRefKind, ObjectKind, RuntimeError, SealedProviderEvidence, TreeObject,
    CONTROL_OBJECT_PATH,
};

const ZERO_OID: &str = "0000000000000000000000000000000000000000";

#[derive(Debug, Clone)]
pub(crate) struct GitObjectProvider {
    repo: PathBuf,
}

impl GitObjectProvider {
    pub(crate) fn new(repo: impl Into<PathBuf>) -> Self {
        Self { repo: repo.into() }
    }

    /// Fetch the fixed Diffinite authority refs into their provider-defined local names.
    pub(crate) fn fetch_diffinite_checkpoint(
        &self,
        remote: &str,
        evidence: &SealedProviderEvidence,
    ) -> Result<ControlState, RuntimeError> {
        let refs = evidence.refs();
        if refs.control_is_prefix || refs.canonical_is_prefix {
            return Err(RuntimeError::Provider(
                "Diffinite checkpoint fetch requires fixed refs".to_string(),
            ));
        }
        self.fetch_refs(remote, &[&refs.control, &refs.canonical])?;
        let state = self.read_control_at(&refs.control)?;
        let generation_id = state
            .generation_id
            .as_deref()
            .ok_or(RuntimeError::InvalidObject("control generation is missing"))?;
        let generation_oid = state
            .canonical_oid
            .as_deref()
            .ok_or(RuntimeError::InvalidObject(
                "control generation OID is missing",
            ))?;
        let archive_ref = format!("{}{generation_id}", refs.archive_prefix);
        self.fetch_refs(remote, &[&archive_ref])?;
        if self.resolve_ref(&refs.canonical)?.as_deref() != Some(generation_oid)
            || self.resolve_ref(&archive_ref)?.as_deref() != Some(generation_oid)
        {
            return Err(RuntimeError::RefDrift);
        }
        Ok(state)
    }

    fn fetch_refs(&self, remote: &str, refs: &[&str]) -> Result<(), RuntimeError> {
        let mut args = vec!["fetch", "--no-tags", "--no-write-fetch-head", remote];
        let refspecs = refs
            .iter()
            .map(|remote_ref| format!("+{remote_ref}:{remote_ref}"))
            .collect::<Vec<_>>();
        args.extend(refspecs.iter().map(String::as_str));
        self.git_ok(&args).map(|_| ())
    }

    fn git(&self, args: &[&str]) -> Result<Output, RuntimeError> {
        Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(args)
            .output()
            .map_err(|error| provider_error("run git", error))
    }

    fn git_ok(&self, args: &[&str]) -> Result<Vec<u8>, RuntimeError> {
        let output = self.git(args)?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        Err(RuntimeError::Provider(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or("command"),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    fn control_ref(
        &self,
        evidence: &SealedProviderEvidence,
    ) -> Result<Option<String>, RuntimeError> {
        let refs = evidence.refs();
        if !refs.control_is_prefix {
            return Ok(self
                .resolve_ref(&refs.control)?
                .map(|_| refs.control.clone()));
        }
        let bytes = self.git_ok(&["for-each-ref", "--format=%(refname)", refs.control.as_str()])?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| RuntimeError::Provider("git returned a non-UTF-8 ref".to_string()))?;
        latest_epoch_ref(text.lines(), &refs.control)
    }

    fn read_control_at(&self, remote_ref: &str) -> Result<ControlState, RuntimeError> {
        let Some(oid) = self.resolve_ref(remote_ref)? else {
            return Ok(ControlState::default());
        };
        let objects = self.read_commit_tree(&oid)?;
        let bytes = objects
            .iter()
            .find(|object| object.path == CONTROL_OBJECT_PATH && object.kind == ObjectKind::Blob)
            .map(|object| object.bytes.as_slice())
            .ok_or(RuntimeError::InvalidObject("control record is missing"))?;
        let record: ControlRecord = serde_json::from_slice(bytes)
            .map_err(|_| RuntimeError::InvalidObject("control record is invalid"))?;
        Ok(ControlState {
            oid: Some(oid),
            epoch: record.epoch,
            generation_id: Some(record.active_generation_id),
            canonical_oid: Some(record.active_generation_commit),
            acknowledged_writer_heads: record.acknowledged_writer_heads,
        })
    }

    fn create_ref(&self, remote_ref: &str, oid: &str) -> Result<(), RuntimeError> {
        if let Some(existing) = self.resolve_ref(remote_ref)? {
            return if existing == oid {
                Ok(())
            } else {
                Err(RuntimeError::Provider("immutable ref conflict".to_string()))
            };
        }
        self.update_ref(remote_ref, oid, None)
    }

    fn update_ref(
        &self,
        remote_ref: &str,
        oid: &str,
        expected_oid: Option<&str>,
    ) -> Result<(), RuntimeError> {
        let expected = expected_oid.unwrap_or(ZERO_OID);
        let output = self.git(&["update-ref", remote_ref, oid, expected])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(RuntimeError::RefDrift)
        }
    }

    fn ensure_fast_forward(&self, old: &str, new: &str) -> Result<(), RuntimeError> {
        let output = self.git(&["merge-base", "--is-ancestor", old, new])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(RuntimeError::Provider(
                "canonical update is not a fast-forward".to_string(),
            ))
        }
    }

    pub(crate) fn git_input(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>, RuntimeError> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_AUTHOR_NAME", "Knots Integrator")
            .env("GIT_AUTHOR_EMAIL", "knots-integrator@invalid")
            .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z")
            .env("GIT_COMMITTER_NAME", "Knots Integrator")
            .env("GIT_COMMITTER_EMAIL", "knots-integrator@invalid")
            .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z")
            .spawn()
            .map_err(|error| provider_error("start git object writer", error))?;
        child
            .stdin
            .take()
            .ok_or_else(|| RuntimeError::Provider("git stdin is unavailable".to_string()))?
            .write_all(input)
            .map_err(|error| provider_error("write git object input", error))?;
        let output = child
            .wait_with_output()
            .map_err(|error| provider_error("wait for git object writer", error))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RuntimeError::Provider(format!(
                "git {} failed: {}",
                args.first().copied().unwrap_or("command"),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

impl ExactObjectReader for GitObjectProvider {
    fn resolve_ref(&self, remote_ref: &str) -> Result<Option<String>, RuntimeError> {
        let output = self.git(&["show-ref", "--verify", "--hash", remote_ref])?;
        if !output.status.success() {
            return Ok(None);
        }
        let oid = String::from_utf8(output.stdout)
            .map_err(|_| RuntimeError::Provider("git returned a non-UTF-8 OID".to_string()))?;
        let oid = oid.trim().to_string();
        if valid_oid(&oid) {
            Ok(Some(oid))
        } else {
            Err(RuntimeError::Provider(
                "git returned an invalid OID".to_string(),
            ))
        }
    }

    fn read_commit_tree(&self, commit_oid: &str) -> Result<Vec<TreeObject>, RuntimeError> {
        if !valid_oid(commit_oid) {
            return Err(RuntimeError::InvalidObject("invalid commit OID"));
        }
        let kind = self.git_ok(&["cat-file", "-t", commit_oid])?;
        if kind != b"commit\n" {
            return Err(RuntimeError::InvalidObject("object is not a commit"));
        }
        let bytes = self.git_ok(&["ls-tree", "-r", "-z", commit_oid])?;
        parse_tree(&self.repo, &bytes)
    }
}

impl GenerationProvider for GitObjectProvider {
    fn read_control(
        &mut self,
        evidence: &SealedProviderEvidence,
    ) -> Result<ControlState, RuntimeError> {
        match self.control_ref(evidence)? {
            Some(remote_ref) => self.read_control_at(&remote_ref),
            None => Ok(ControlState::default()),
        }
    }

    fn publish_generation(
        &mut self,
        evidence: &SealedProviderEvidence,
        kind: GenerationRefKind,
        remote_ref: &str,
        expected_oid: Option<&str>,
        oid: &str,
    ) -> Result<(), RuntimeError> {
        if !valid_oid(oid) {
            return Err(RuntimeError::InvalidObject("invalid publication OID"));
        }
        match kind {
            GenerationRefKind::Archive => self.create_ref(remote_ref, oid),
            GenerationRefKind::Canonical if evidence.refs().canonical_is_prefix => {
                self.create_ref(remote_ref, oid)
            }
            GenerationRefKind::Canonical => {
                if let Some(old) = expected_oid {
                    self.ensure_fast_forward(old, oid)?;
                }
                self.update_ref(remote_ref, oid, expected_oid)
            }
        }
    }

    fn activate_control(
        &mut self,
        evidence: &SealedProviderEvidence,
        remote_ref: &str,
        expected_oid: Option<&str>,
        control_oid: &str,
    ) -> Result<CasOutcome, RuntimeError> {
        let result = match evidence.control_mode() {
            ControlMode::ImmutableEpoch => self.create_ref(remote_ref, control_oid),
            ControlMode::IntegratorCompareAndSwap => {
                self.update_ref(remote_ref, control_oid, expected_oid)
            }
        };
        match result {
            Ok(()) => Ok(CasOutcome::Applied),
            Err(RuntimeError::RefDrift) => Ok(CasOutcome::Stale(self.read_control(evidence)?)),
            Err(error) => Err(error),
        }
    }
}

fn latest_epoch_ref<'a>(
    refs: impl Iterator<Item = &'a str>,
    prefix: &str,
) -> Result<Option<String>, RuntimeError> {
    let mut candidates = Vec::new();
    for remote_ref in refs {
        let suffix = remote_ref.strip_prefix(prefix).ok_or_else(|| {
            RuntimeError::Provider("git returned a ref outside the control prefix".to_string())
        })?;
        let epoch = suffix
            .split('/')
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| RuntimeError::Provider("invalid control epoch ref".to_string()))?;
        candidates.push((epoch, remote_ref.to_string()));
    }
    candidates.sort();
    if candidates.len() >= 2
        && candidates[candidates.len() - 2].0 == candidates[candidates.len() - 1].0
    {
        return Err(RuntimeError::Provider(
            "multiple control refs claim the latest epoch".to_string(),
        ));
    }
    Ok(candidates.pop().map(|(_, remote_ref)| remote_ref))
}

fn parse_tree(repo: &Path, bytes: &[u8]) -> Result<Vec<TreeObject>, RuntimeError> {
    let mut objects = bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(parse_tree_entry)
        .collect::<Result<Vec<_>, _>>()?;
    read_tree_objects(repo, &mut objects)?;
    Ok(objects)
}

fn parse_tree_entry(entry: &[u8]) -> Result<TreeObject, RuntimeError> {
    let tab = entry
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or(RuntimeError::InvalidObject("malformed tree entry"))?;
    let metadata = std::str::from_utf8(&entry[..tab])
        .map_err(|_| RuntimeError::InvalidObject("non-UTF-8 tree metadata"))?;
    let mut fields = metadata.split_whitespace();
    let mode = fields
        .next()
        .ok_or(RuntimeError::InvalidObject("missing mode"))?;
    let git_kind = fields
        .next()
        .ok_or(RuntimeError::InvalidObject("missing kind"))?;
    let oid = fields
        .next()
        .ok_or(RuntimeError::InvalidObject("missing OID"))?;
    if fields.next().is_some() || !valid_oid(oid) {
        return Err(RuntimeError::InvalidObject("invalid tree metadata"));
    }
    let path = std::str::from_utf8(&entry[tab + 1..])
        .map_err(|_| RuntimeError::InvalidObject("non-UTF-8 tree path"))?;
    let kind = object_kind(mode, git_kind)?;
    Ok(TreeObject {
        path: path.to_string(),
        kind,
        oid: oid.to_string(),
        bytes: Vec::new(),
    })
}

fn read_tree_objects(repo: &Path, objects: &mut [TreeObject]) -> Result<(), RuntimeError> {
    let requested = objects
        .iter_mut()
        .filter(|object| object.kind != ObjectKind::Submodule)
        .collect::<Vec<_>>();
    if requested.is_empty() {
        return Ok(());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| provider_error("start batched blob reader", error))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| RuntimeError::Provider("git stdin is unavailable".to_string()))?;
    let oids = requested
        .iter()
        .map(|object| object.oid.clone())
        .collect::<Vec<_>>();
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        for oid in oids {
            writeln!(stdin, "{oid}")?;
        }
        Ok(())
    });
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RuntimeError::Provider("git stdout is unavailable".to_string()))?;
    let mut reader = BufReader::new(stdout);
    for object in requested {
        object.bytes = read_batch_object(&mut reader, object)?;
    }
    writer
        .join()
        .map_err(|_| RuntimeError::Provider("git object writer panicked".to_string()))?
        .map_err(|error| provider_error("write batched object IDs", error))?;
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr)
            .map_err(|error| provider_error("read git object errors", error))?;
    }
    let status = child
        .wait()
        .map_err(|error| provider_error("wait for batched object reader", error))?;
    if status.success() {
        Ok(())
    } else {
        Err(RuntimeError::Provider(format!(
            "git cat-file failed: {}",
            stderr.trim()
        )))
    }
}

fn read_batch_object(
    reader: &mut impl BufRead,
    object: &TreeObject,
) -> Result<Vec<u8>, RuntimeError> {
    let mut header = String::new();
    reader
        .read_line(&mut header)
        .map_err(|error| provider_error("read object header", error))?;
    let mut fields = header.split_whitespace();
    let oid = fields.next();
    let kind = fields.next();
    let size = fields.next().and_then(|value| value.parse::<usize>().ok());
    let expected_kind = match object.kind {
        ObjectKind::Blob | ObjectKind::Symlink => "blob",
        ObjectKind::Tree => "tree",
        ObjectKind::Submodule => "commit",
    };
    if oid != Some(object.oid.as_str()) || kind != Some(expected_kind) || size.is_none() {
        return Err(RuntimeError::InvalidObject("invalid batched object header"));
    }
    let mut bytes = vec![0; size.expect("checked object size")];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| provider_error("read object body", error))?;
    let mut delimiter = [0];
    reader
        .read_exact(&mut delimiter)
        .map_err(|error| provider_error("read object delimiter", error))?;
    if delimiter != *b"\n" {
        return Err(RuntimeError::InvalidObject(
            "invalid batched object delimiter",
        ));
    }
    Ok(bytes)
}

fn object_kind(mode: &str, git_kind: &str) -> Result<ObjectKind, RuntimeError> {
    match (mode, git_kind) {
        ("120000", "blob") => Ok(ObjectKind::Symlink),
        ("160000", "commit") => Ok(ObjectKind::Submodule),
        (_, "blob") => Ok(ObjectKind::Blob),
        (_, "tree") => Ok(ObjectKind::Tree),
        _ => Err(RuntimeError::InvalidObject("unsupported tree object")),
    }
}

fn provider_error(action: &str, error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::Provider(format!("failed to {action}: {error}"))
}

#[cfg(test)]
#[path = "git_adapter_tests.rs"]
mod tests;

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use std::collections::BTreeMap;

use super::*;
use crate::compaction::provider_runtime::{
    validate_provider_evidence, CanonicalMode, ProviderEvidenceInput, ProviderKind,
    ProviderRefLayout, SubmissionMode,
};
use crate::compaction::MANIFEST_OBJECT_PATH;

const POLICY: &[u8] = b"provider policy";

#[test]
fn reads_exact_commit_without_materializing_a_checkout() {
    let repo = TestRepo::new();
    let first = repo.commit_file(".knots/v2/inbox/submission.json", b"first", None);
    let second = repo.commit_file(".knots/v2/inbox/submission.json", b"second", Some(&first));
    let reader = GitObjectProvider::new(&repo.path);

    let tree = reader.read_commit_tree(&first).expect("first commit tree");
    let object = tree
        .iter()
        .find(|entry| entry.path == ".knots/v2/inbox/submission.json")
        .expect("submission blob");
    assert_eq!(object.bytes, b"first");
    assert_ne!(first, second);
    assert!(!repo.path.join(".knots").exists());
}

#[test]
fn github_refs_are_create_only_and_control_uses_latest_epoch() {
    let repo = TestRepo::new();
    let generation = repo.commit_file("payload", b"one", None);
    let other = repo.commit_file("payload", b"two", Some(&generation));
    let control = repo.control_commit(4, "generation-4", &generation, None);
    let mut provider = GitObjectProvider::new(&repo.path);
    let evidence = sealed(ProviderKind::GitHub);
    let canonical = "refs/heads/knots-v2-canonical/generation-4/nonce";

    provider
        .publish_generation(
            &evidence,
            GenerationRefKind::Canonical,
            canonical,
            None,
            &generation,
        )
        .expect("create canonical");
    provider
        .publish_generation(
            &evidence,
            GenerationRefKind::Canonical,
            canonical,
            None,
            &generation,
        )
        .expect("idempotent publication");
    assert!(provider
        .publish_generation(
            &evidence,
            GenerationRefKind::Canonical,
            canonical,
            None,
            &other,
        )
        .is_err());
    repo.update_ref(
        "refs/heads/knots-v2-control/epochs/4/run-nonce-digest",
        &control,
    );
    let observed = provider.read_control(&evidence).expect("latest control");
    assert_eq!(observed.epoch, 4);
    assert_eq!(observed.canonical_oid.as_deref(), Some(generation.as_str()));
}

#[test]
fn diffinite_control_reports_stale_compare_and_swap() {
    let repo = TestRepo::new();
    let generation = repo.commit_file("payload", b"generation", None);
    let first = repo.control_commit(1, "generation-1", &generation, None);
    let second = repo.control_commit(2, "generation-2", &generation, Some(&first));
    let evidence = sealed(ProviderKind::Diffinite);
    let mut provider = GitObjectProvider::new(&repo.path);
    repo.update_ref(evidence.refs().control.as_str(), &first);

    let outcome = provider
        .activate_control(&evidence, evidence.refs().control.as_str(), None, &second)
        .expect("stale CAS is observable");
    assert!(matches!(outcome, CasOutcome::Stale(state) if state.oid == Some(first)));
}

#[test]
fn diffinite_canonical_requires_a_fast_forward_and_exact_expected_oid() {
    let repo = TestRepo::new();
    let base = repo.commit_file("payload", b"base", None);
    let next = repo.commit_file("payload", b"next", Some(&base));
    let unrelated = repo.commit_file("other", b"unrelated", None);
    let evidence = sealed(ProviderKind::Diffinite);
    let canonical = evidence.refs().canonical.as_str();
    let mut provider = GitObjectProvider::new(&repo.path);
    repo.update_ref(canonical, &base);

    provider
        .publish_generation(
            &evidence,
            GenerationRefKind::Canonical,
            canonical,
            Some(&base),
            &next,
        )
        .expect("fast-forward canonical update");
    assert!(provider
        .publish_generation(
            &evidence,
            GenerationRefKind::Canonical,
            canonical,
            Some(&next),
            &unrelated,
        )
        .is_err());
    assert_eq!(provider.resolve_ref(canonical).unwrap(), Some(next));
}

#[test]
fn object_writer_creates_nested_commit_with_requested_parent() {
    let repo = TestRepo::new();
    let parent = repo.commit_file("seed", b"seed", None);
    let provider = GitObjectProvider::new(&repo.path);
    let commit = provider
        .write_commit(
            BTreeMap::from([
                (MANIFEST_OBJECT_PATH.to_string(), b"manifest".to_vec()),
                (
                    ".knots/v2/packs/pack-a.pack.zst".to_string(),
                    b"pack".to_vec(),
                ),
            ]),
            Some(&parent),
            "fixture generation\n",
        )
        .expect("generation commit is written");

    let tree = provider
        .read_commit_tree(&commit)
        .expect("commit tree reads");
    assert!(tree
        .iter()
        .any(|object| object.path == MANIFEST_OBJECT_PATH));
    assert!(tree
        .iter()
        .any(|object| object.path == ".knots/v2/packs/pack-a.pack.zst"));
    let parent_line = git(&repo.path, &["rev-parse", &format!("{commit}^")]);
    assert_eq!(parent_line.trim(), parent);
}

#[test]
fn diffinite_fetch_loads_control_canonical_and_archive_by_exact_oid() {
    let remote = TestRepo::new();
    let local = TestRepo::new();
    let evidence = sealed(ProviderKind::Diffinite);
    let generation_id = "generation-7";
    let generation = remote.commit_file(MANIFEST_OBJECT_PATH, b"manifest", None);
    let control = remote.control_commit(7, generation_id, &generation, None);
    remote.update_ref(evidence.refs().control.as_str(), &control);
    remote.update_ref(evidence.refs().canonical.as_str(), &generation);
    remote.update_ref(
        &format!("{}{generation_id}", evidence.refs().archive_prefix),
        &generation,
    );
    let provider = GitObjectProvider::new(&local.path);

    let observed = provider
        .fetch_diffinite_checkpoint(local_path(&remote.path), &evidence)
        .expect("fixed checkpoint refs fetch");

    assert_eq!(observed.epoch, 7);
    assert_eq!(observed.oid.as_deref(), Some(control.as_str()));
    assert_eq!(observed.canonical_oid.as_deref(), Some(generation.as_str()));
}

fn sealed(provider: ProviderKind) -> SealedProviderEvidence {
    let repository_id = match provider {
        ProviderKind::GitHub => "github:owner/repo",
        ProviderKind::Diffinite => "diffinite:owner/repo",
    };
    let refs = match provider {
        ProviderKind::GitHub => ProviderRefLayout::github(),
        ProviderKind::Diffinite => ProviderRefLayout::diffinite(),
    };
    let input = ProviderEvidenceInput {
        provider,
        repository_id: repository_id.to_string(),
        policy_id: "policy-1".to_string(),
        policy_sha256: format!("{:x}", Sha256::digest(POLICY)),
        integrator_id: "integrator-1".to_string(),
        refs,
        control_mode: match provider {
            ProviderKind::GitHub => ControlMode::ImmutableEpoch,
            ProviderKind::Diffinite => ControlMode::IntegratorCompareAndSwap,
        },
        canonical_mode: match provider {
            ProviderKind::GitHub => CanonicalMode::CreateOnly,
            ProviderKind::Diffinite => CanonicalMode::IntegratorFastForward,
        },
        archives_create_only: true,
        submission_mode: match provider {
            ProviderKind::GitHub => SubmissionMode::ImmutableProposal,
            ProviderKind::Diffinite => SubmissionMode::CredentialInbox,
        },
        exact_oid_reads: true,
    };
    validate_provider_evidence(input, POLICY, provider, repository_id).unwrap()
}

struct TestRepo {
    _workspace: knots_test_support::TestWorkspace,
    path: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let workspace = knots_test_support::workspace("knots-provider-git");
        let path = workspace.path().to_path_buf();
        git(&path, &["init", "--bare"]);
        Self {
            _workspace: workspace,
            path,
        }
    }

    fn commit_file(&self, path: &str, bytes: &[u8], parent: Option<&str>) -> String {
        let blob = git_input(&self.path, &["hash-object", "-w", "--stdin"], bytes);
        let tree = self.tree_for_path(path, blob.trim());
        let mut args = vec!["commit-tree", tree.trim(), "-m", "fixture"];
        if let Some(parent) = parent {
            args.extend(["-p", parent]);
        }
        git(&self.path, &args).trim().to_string()
    }

    fn tree_for_path(&self, path: &str, blob: &str) -> String {
        let mut parts: Vec<&str> = path.split('/').collect();
        let file = parts.pop().unwrap();
        let entry = format!("100644 blob {blob}\t{file}\n");
        let mut tree = git_input(&self.path, &["mktree"], entry.as_bytes());
        while let Some(directory) = parts.pop() {
            let entry = format!("040000 tree {}\t{directory}\n", tree.trim());
            tree = git_input(&self.path, &["mktree"], entry.as_bytes());
        }
        tree
    }

    fn control_commit(
        &self,
        epoch: u64,
        generation_id: &str,
        generation_oid: &str,
        previous: Option<&str>,
    ) -> String {
        let record = serde_json::json!({
            "schema_version": 1,
            "epoch": epoch,
            "previous_control_head": previous,
            "active_generation_id": generation_id,
            "active_generation_commit": generation_oid,
            "archive_ref": format!("refs/heads/knots-v2-archive/{generation_id}"),
            "acknowledged_writer_heads": [],
            "protection_policy_sha256": "a".repeat(64),
            "action": { "kind": "activation" }
        });
        self.commit_file(
            CONTROL_OBJECT_PATH,
            &serde_json::to_vec(&record).unwrap(),
            previous,
        )
    }

    fn update_ref(&self, remote_ref: &str, oid: &str) {
        git(&self.path, &["update-ref", remote_ref, oid]);
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn git_input(repo: &Path, args: &[&str], input: &[u8]) -> String {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn local_path(path: &Path) -> &str {
    path.to_str().expect("test path is UTF-8")
}

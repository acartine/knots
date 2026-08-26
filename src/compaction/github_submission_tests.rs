use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::compaction::{sign_submission, GitHubProposalInput};
use crate::db::{
    acknowledge_outbox, assign_pending_outbox, ensure_writer_epoch, mark_outbox_proposed,
    record_outbox_event,
};

use super::{create_github_canary_submission, submission_purpose, verify_github_proposal};

#[test]
fn creates_isolated_signed_canary_writer() {
    let payload = event("github-live-canary-example");
    let signed = create_github_canary_submission(
        "acartine/knots",
        &"ab".repeat(20),
        "github-live-canary-example",
        "events/2026/08/26/github-live-canary-example.json",
        &payload,
    )
    .unwrap();
    assert_eq!(signed.bundle.sequence, 1);
    assert_eq!(submission_purpose(&signed), "canary");
    assert_eq!(signed.proposal_oid, "ab".repeat(20));
}

#[test]
fn verifies_first_and_fast_forward_proposals_from_provider_state() {
    let fixture = Fixture::new();
    let first = fixture.proposal("event-first", None, false);
    let plan = fixture.verify(&first).expect("first proposal verifies");
    assert_eq!(plan.sequence, 1);
    assert_eq!(plan.expected_old_oid, None);
    fixture.promote(&first);
    git(
        &fixture.work,
        [
            "push",
            "origin",
            &format!(
                "{}:refs/heads/knots-v2-proposals/{}/999",
                first.oid, first.writer_id
            ),
        ],
    );
    acknowledge_outbox(&fixture.conn, &first.writer_id, &first.oid, true)
        .expect("acknowledge first");

    let second = fixture.proposal("event-second", Some(&first.oid), false);
    let plan = fixture.verify(&second).expect("second proposal verifies");
    assert_eq!(plan.sequence, 2);
    assert_eq!(plan.expected_old_oid.as_deref(), Some(first.oid.as_str()));
}

#[test]
fn rejects_inbox_without_protected_writer_registry() {
    let fixture = Fixture::new();
    let first = fixture.proposal("event-first", None, false);
    fixture.promote_inbox_only(&first);
    acknowledge_outbox(&fixture.conn, &first.writer_id, &first.oid, true).unwrap();
    let second = fixture.proposal("event-second", Some(&first.oid), false);
    assert!(fixture.verify(&second).is_err());
}

#[test]
fn protected_registry_prevents_adding_removing_or_changing_lineage() {
    let fake_a = "11".repeat(32);
    let fake_b = "22".repeat(32);

    let added = Fixture::new();
    let first = added.proposal("event-first", None, false);
    added.promote(&first);
    acknowledge_outbox(&added.conn, &first.writer_id, &first.oid, true).unwrap();
    let second = added.proposal("event-second", Some(&first.oid), false);
    mutate_lineage(&second.envelope, Some(&fake_a));
    assert!(added.verify(&second).unwrap_err().contains("lineage"));

    let removed = Fixture::new();
    let first = removed.proposal("event-first", None, false);
    removed.promote_with_parent(&first, Some(&fake_a));
    acknowledge_outbox(&removed.conn, &first.writer_id, &first.oid, true).unwrap();
    let second = removed.proposal("event-second", Some(&first.oid), false);
    assert!(removed.verify(&second).unwrap_err().contains("lineage"));

    let changed = Fixture::new();
    let first = changed.proposal("event-first", None, false);
    changed.promote_with_parent(&first, Some(&fake_a));
    acknowledge_outbox(&changed.conn, &first.writer_id, &first.oid, true).unwrap();
    let second = changed.proposal("event-second", Some(&first.oid), false);
    mutate_lineage(&second.envelope, Some(&fake_b));
    assert!(changed.verify(&second).unwrap_err().contains("lineage"));
}

#[test]
fn rejects_forged_envelopes_and_undeclared_tree_paths() {
    let fixture = Fixture::new();
    let forged = fixture.proposal("event-forged", None, false);
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&forged.envelope).unwrap()).unwrap();
    envelope["signature"] = json!("00".repeat(64));
    fs::write(&forged.envelope, serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert!(fixture.verify(&forged).is_err());

    let extra_fixture = Fixture::new();
    let extra = extra_fixture.proposal("event-extra", None, true);
    assert!(extra_fixture.verify(&extra).is_err());
}

struct Proposal {
    oid: String,
    proposal_ref: String,
    inbox_ref: String,
    writer_id: String,
    envelope: PathBuf,
}

struct Fixture {
    _workspace: knots_test_support::TestWorkspace,
    root: PathBuf,
    work: PathBuf,
    verifier: PathBuf,
    conn: Connection,
}

impl Fixture {
    fn new() -> Self {
        let workspace = knots_test_support::workspace("knots-github-submission");
        let root = workspace.path().to_path_buf();
        let origin = root.join("origin.git");
        let work = root.join("work");
        let verifier = root.join("verify.git");
        fs::create_dir_all(&root).unwrap();
        git(&root, ["init", "--bare", origin.to_str().unwrap()]);
        git(&root, ["init", work.to_str().unwrap()]);
        git(&work, ["config", "user.name", "Knots Test"]);
        git(&work, ["config", "user.email", "knots@example.invalid"]);
        git(&work, ["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&root, ["init", "--bare", verifier.to_str().unwrap()]);
        git(
            &root,
            [
                "--git-dir",
                verifier.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                origin.to_str().unwrap(),
            ],
        );
        Self {
            _workspace: workspace,
            root,
            work,
            verifier,
            conn: crate::db::open_connection(":memory:").unwrap(),
        }
    }

    fn proposal(&self, event_id: &str, parent: Option<&str>, extra: bool) -> Proposal {
        let payload = event(event_id);
        let relative = format!("events/2026/08/26/{event_id}.json");
        record_outbox_event(
            &self.conn,
            event_id,
            "events",
            &relative,
            &digest(&payload),
            &payload,
        )
        .unwrap();
        let writer = ensure_writer_epoch(&self.conn, "github-test").unwrap();
        let records = assign_pending_outbox(&self.conn, &writer, 10).unwrap();
        reset_worktree(&self.work, parent);
        let path = self.work.join(".knots/v2/inbox").join(&relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &payload).unwrap();
        if extra {
            fs::write(self.work.join("undeclared.txt"), b"untrusted").unwrap();
        }
        git(&self.work, ["add", "."]);
        git(&self.work, ["commit", "-m", event_id]);
        let oid = output(&self.work, ["rev-parse", "HEAD"]);
        let signed =
            sign_submission(&self.conn, "acartine/knots", &writer, &records, &oid, None).unwrap();
        let event_ids = records
            .iter()
            .map(|record| record.event_id.clone())
            .collect::<Vec<_>>();
        mark_outbox_proposed(&self.conn, &writer.writer_id, &event_ids, &oid, None).unwrap();
        git(
            &self.work,
            ["push", "origin", &format!("{oid}:{}", signed.proposal_ref)],
        );
        git(
            &self.root,
            [
                "--git-dir",
                self.verifier.to_str().unwrap(),
                "fetch",
                "origin",
                &signed.proposal_ref,
            ],
        );
        let envelope = self.root.join(format!("{event_id}.json"));
        fs::write(&envelope, serde_json::to_vec(&signed).unwrap()).unwrap();
        Proposal {
            oid,
            proposal_ref: signed.proposal_ref,
            inbox_ref: signed.target_ref,
            writer_id: signed.bundle.writer_id,
            envelope,
        }
    }

    fn verify(&self, proposal: &Proposal) -> Result<super::GitHubPromotionPlan, String> {
        verify_github_proposal(&GitHubProposalInput {
            git_dir: self.verifier.clone(),
            repository_id: "acartine/knots".to_string(),
            proposal_ref: proposal.proposal_ref.clone(),
            proposal_oid: proposal.oid.clone(),
            signed_submission: proposal.envelope.clone(),
        })
    }

    fn promote(&self, proposal: &Proposal) {
        self.promote_with_parent(proposal, None);
    }

    fn promote_with_parent(&self, proposal: &Proposal, parent_writer_id: Option<&str>) {
        self.promote_inbox_only(proposal);
        let signed: serde_json::Value =
            serde_json::from_slice(&fs::read(&proposal.envelope).unwrap()).unwrap();
        let registry = self
            .root
            .join(format!("registry-{}", &proposal.writer_id[..8]));
        git(&self.root, ["init", registry.to_str().unwrap()]);
        git(&registry, ["config", "user.name", "Knots Test"]);
        git(&registry, ["config", "user.email", "knots@example.invalid"]);
        git(
            &registry,
            [
                "remote",
                "add",
                "origin",
                self.root.join("origin.git").to_str().unwrap(),
            ],
        );
        let path = registry.join(".knots/v2/writer.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            serde_json::to_vec(&json!({
                "schema_version": 1,
                "writer_id": proposal.writer_id,
                "public_key": signed["public_key"],
                "inbox_oid": proposal.oid,
                "sequence": 1,
                "parent_writer_id": parent_writer_id,
                "purpose": "production",
            }))
            .unwrap(),
        )
        .unwrap();
        git(&registry, ["add", "."]);
        git(&registry, ["commit", "-m", "writer registry"]);
        git(
            &registry,
            [
                "push",
                "origin",
                &format!("HEAD:refs/heads/knots-v2-writers/{}", proposal.writer_id),
            ],
        );
    }

    fn promote_inbox_only(&self, proposal: &Proposal) {
        git(
            &self.work,
            [
                "push",
                "origin",
                &format!("{}:{}", proposal.oid, proposal.inbox_ref),
            ],
        );
    }
}

fn mutate_lineage(path: &Path, parent_writer_id: Option<&str>) {
    let mut envelope: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    envelope["parent_writer_id"] = json!(parent_writer_id);
    envelope["rotation_signature"] = json!(parent_writer_id.map(|_| "00".repeat(64)));
    fs::write(path, serde_json::to_vec(&envelope).unwrap()).unwrap();
}

fn reset_worktree(work: &Path, parent: Option<&str>) {
    if let Some(parent) = parent {
        git(work, ["reset", "--hard", parent]);
        let data = work.join(".knots/v2/inbox");
        if data.exists() {
            fs::remove_dir_all(data).unwrap();
        }
    }
}

fn event(event_id: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "event_id": event_id,
        "occurred_at": "2026-08-26T04:00:00Z",
        "knot_id": format!("knots-{event_id}"),
        "type": "knot.created",
        "data": {"title": event_id}
    }))
    .unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn output<const N: usize>(cwd: &Path, args: [&str; N]) -> String {
    let result = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout).unwrap().trim().to_string()
}

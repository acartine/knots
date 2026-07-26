//! `kno rollback --outcome <name>`: workflow-declared failure transitions.
//!
//! Covers the shipment -> ready_for_implementation case that generic
//! rollback cannot express, the atomic lease release that used to require
//! manual lease surgery, and the rejections that keep this from becoming an
//! unrestricted force-state command.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

fn unique_workspace(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

fn run_knots(repo_root: &Path, db_path: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(PathBuf::from(env!("CARGO_BIN_EXE_knots")))
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--db")
        .arg(db_path)
        .env("HOME", home)
        .env("KNOTS_SKIP_DOCTOR_UPGRADE", "1")
        .args(args)
        .output()
        .expect("knots command should run")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "expected success but failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    db: PathBuf,
}

impl Fixture {
    fn new(prefix: &str) -> Self {
        let root = unique_workspace(prefix);
        let home = unique_workspace(&format!("{prefix}-home"));
        let db = root.join(".knots/cache/state.sqlite");
        let fixture = Self { root, home, db };
        fixture.run(&["init"]);
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        run_knots(&self.root, &self.db, &self.home, args)
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert_success(&output);
        serde_json::from_slice(&output.stdout).expect("output should be valid JSON")
    }

    /// `show --json` omits `lease_id`, so read the knot from `ls --json`.
    fn show(&self, id: &str) -> Value {
        self.json(&["ls", "--json"])
            .as_array()
            .expect("ls --json should return an array")
            .iter()
            .find(|knot| knot["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("knot {id} should be listed"))
    }

    fn lease_state(&self, lease: &str) -> String {
        self.json(&["lease", "show", lease, "--json"])["state"]
            .as_str()
            .expect("lease should have a state")
            .to_string()
    }

    /// Create a work knot and walk it to `ready_for_shipment`.
    fn knot_ready_for_shipment(&self, title: &str) -> String {
        let created = self.json(&["new", title, "-d", "failure outcome fixture", "--json"]);
        let id = created["id"]
            .as_str()
            .expect("new --json should return an id")
            .to_string();
        for state in [
            "planning",
            "ready_for_plan_review",
            "plan_review",
            "ready_for_implementation",
            "implementation",
            "ready_for_implementation_review",
            "implementation_review",
            "ready_for_shipment",
        ] {
            assert_success(&self.run(&["update", &id, "--status", state]));
        }
        id
    }

    /// Create a lease and claim `id` with it, returning the lease id.
    fn claim(&self, id: &str, nickname: &str) -> String {
        let created = self.run(&["lease", "create", "--nickname", nickname]);
        assert_success(&created);
        let lease = String::from_utf8_lossy(&created.stdout)
            .split_whitespace()
            .nth(2)
            .expect("lease create should print an id")
            .to_string();
        let claimed = self.json(&["claim", id, "--lease", &lease, "--json"]);
        assert_eq!(claimed["state"], "shipment");
        lease
    }
}

fn last_step(knot: &Value) -> &Value {
    knot["step_history"]
        .as_array()
        .and_then(|steps| steps.last())
        .expect("knot should carry step history")
}

#[test]
fn shipment_merge_conflicts_lands_in_ready_for_implementation_and_frees_the_lease() {
    let fx = Fixture::new("knots-rollback-outcome");
    let id = fx.knot_ready_for_shipment("Failed shipment");
    let lease = fx.claim(&id, "shipper");

    let rolled = fx.json(&[
        "rollback",
        &id,
        "--outcome",
        "merge_conflicts",
        "--lease",
        &lease,
        "--json",
    ]);
    assert_eq!(rolled["state"], "ready_for_implementation");
    assert_eq!(rolled["target_state"], "ready_for_implementation");
    assert_eq!(rolled["outcome"], "merge_conflicts");
    assert_eq!(rolled["lease_released"], Value::Bool(true));

    let knot = fx.show(&id);
    assert_eq!(knot["state"], "ready_for_implementation");
    assert_eq!(
        knot["lease_id"],
        Value::Null,
        "failure outcome must unbind the lease"
    );

    assert_eq!(
        fx.lease_state(&lease),
        "lease_terminated",
        "failure outcome must terminate the lease knot"
    );
}

#[test]
fn failed_shipment_step_records_its_non_adjacent_target_and_identity() {
    let fx = Fixture::new("knots-rollback-outcome-steps");
    let id = fx.knot_ready_for_shipment("Failed shipment history");
    let lease = fx.claim(&id, "historian");

    assert_success(&fx.run(&[
        "rollback",
        &id,
        "--outcome",
        "merge_conflicts",
        "--lease",
        &lease,
    ]));

    let knot = fx.show(&id);
    let step = last_step(&knot);
    assert_eq!(step["step"], "shipment");
    assert_eq!(step["status"], "failed");
    assert_eq!(step["from_state"], "ready_for_shipment");
    assert_eq!(step["to_state"], "ready_for_implementation");
    assert_eq!(
        step["actor_kind"], "agent",
        "failed step should keep lease-sourced actor identity"
    );
}

#[test]
fn escape_state_outcomes_block_the_knot_and_still_release_the_lease() {
    let fx = Fixture::new("knots-rollback-outcome-escape");
    let id = fx.knot_ready_for_shipment("Release blocked");
    let lease = fx.claim(&id, "releaser");

    let rolled = fx.json(&[
        "rollback",
        &id,
        "--outcome",
        "release_blocked",
        "--lease",
        &lease,
        "--json",
    ]);
    assert_eq!(rolled["state"], "blocked");

    let knot = fx.show(&id);
    assert_eq!(knot["state"], "blocked");
    assert_eq!(knot["lease_id"], Value::Null);
    assert_eq!(
        knot["blocked_from_state"], "ready_for_shipment",
        "blocked provenance must survive the failure transition"
    );
    assert_eq!(fx.lease_state(&lease), "lease_terminated");
}

#[test]
fn undeclared_outcomes_are_rejected_without_touching_state_or_lease() {
    let fx = Fixture::new("knots-rollback-outcome-unknown");
    let id = fx.knot_ready_for_shipment("Undeclared outcome");
    let lease = fx.claim(&id, "stranger");

    let rejected = fx.run(&[
        "rollback",
        &id,
        "--outcome",
        "definitely_not_real",
        "--lease",
        &lease,
    ]);
    assert!(!rejected.status.success());
    let message = stderr(&rejected);
    assert!(
        message.contains("is not a declared failure outcome"),
        "{message}"
    );
    assert!(message.contains("merge_conflicts"), "{message}");

    let knot = fx.show(&id);
    assert_eq!(knot["state"], "shipment");
    assert_eq!(knot["lease_id"].as_str(), Some(lease.as_str()));
    assert_eq!(fx.lease_state(&lease), "lease_active");
}

#[test]
fn success_outcomes_are_rejected_and_point_at_next() {
    let fx = Fixture::new("knots-rollback-outcome-success");
    let id = fx.knot_ready_for_shipment("Success outcome");
    let lease = fx.claim(&id, "advancer");

    let rejected = fx.run(&["rollback", &id, "--outcome", "success", "--lease", &lease]);
    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected).contains("kno next"),
        "{}",
        stderr(&rejected)
    );

    let knot = fx.show(&id);
    assert_eq!(knot["state"], "shipment");
    assert_eq!(knot["lease_id"].as_str(), Some(lease.as_str()));
}

#[test]
fn dry_run_previews_the_target_without_mutating_state_or_lease() {
    let fx = Fixture::new("knots-rollback-outcome-dry-run");
    let id = fx.knot_ready_for_shipment("Dry run outcome");
    let lease = fx.claim(&id, "previewer");

    let preview = fx.json(&[
        "rollback",
        &id,
        "--outcome",
        "merge_conflicts",
        "--lease",
        &lease,
        "--dry-run",
        "--json",
    ]);
    assert_eq!(preview["dry_run"], Value::Bool(true));
    assert_eq!(preview["state"], "shipment");
    assert_eq!(preview["target_state"], "ready_for_implementation");
    assert_eq!(preview["lease_released"], Value::Bool(false));

    let knot = fx.show(&id);
    assert_eq!(knot["state"], "shipment");
    assert_eq!(knot["lease_id"].as_str(), Some(lease.as_str()));
    assert_eq!(fx.lease_state(&lease), "lease_active");
}

#[test]
fn a_mismatched_lease_is_rejected_before_any_mutation() {
    let fx = Fixture::new("knots-rollback-outcome-lease");
    let id = fx.knot_ready_for_shipment("Lease mismatch");
    let lease = fx.claim(&id, "owner");

    let rejected = fx.run(&[
        "rollback",
        &id,
        "--outcome",
        "merge_conflicts",
        "--lease",
        "zzzz",
    ]);
    assert!(!rejected.status.success());
    assert!(stderr(&rejected).contains("lease"), "{}", stderr(&rejected));

    let knot = fx.show(&id);
    assert_eq!(knot["state"], "shipment");
    assert_eq!(knot["lease_id"].as_str(), Some(lease.as_str()));
}

#[test]
fn generic_rollback_keeps_its_immediate_prior_queue_semantics() {
    let fx = Fixture::new("knots-rollback-generic");
    let id = fx.knot_ready_for_shipment("Generic rollback");
    let lease = fx.claim(&id, "rewinder");

    let rolled = fx.json(&["rollback", &id, "--json"]);
    assert_eq!(
        rolled["target_state"], "ready_for_shipment",
        "bare rollback must still rewind one queue state"
    );
    assert!(rolled["outcome"].is_null(), "bare rollback has no outcome");
    assert_eq!(fx.show(&id)["state"], "ready_for_shipment");
    assert_eq!(fx.lease_state(&lease), "lease_terminated");
}

#[test]
fn generic_rollback_still_rejects_queue_states() {
    let fx = Fixture::new("knots-rollback-generic-queue");
    let id = fx.knot_ready_for_shipment("Queue rollback");

    let rejected = fx.run(&["rollback", &id]);
    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected).contains("queue state"),
        "{}",
        stderr(&rejected)
    );
    assert_eq!(fx.show(&id)["state"], "ready_for_shipment");
}

#[test]
fn outcome_rollback_also_rejects_queue_states() {
    let fx = Fixture::new("knots-rollback-outcome-queue");
    let id = fx.knot_ready_for_shipment("Queue outcome");

    let rejected = fx.run(&["rollback", &id, "--outcome", "merge_conflicts"]);
    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected).contains("queue state"),
        "{}",
        stderr(&rejected)
    );
    assert_eq!(fx.show(&id)["state"], "ready_for_shipment");
}

#[test]
fn mcp_session_leases_are_returned_to_ready_rather_than_terminated() {
    // MCP owns a session-scoped lease, so releasing it must recycle it the
    // same way `kno next` and generic rollback do.
    let fx = Fixture::new("knots-rollback-outcome-mcp");
    let id = fx.knot_ready_for_shipment("MCP session lease");
    let lease = fx.claim(&id, "mcp-session-abc");

    assert_success(&fx.run(&[
        "rollback",
        &id,
        "--outcome",
        "merge_conflicts",
        "--lease",
        &lease,
    ]));

    assert_eq!(fx.show(&id)["state"], "ready_for_implementation");
    assert_eq!(fx.show(&id)["lease_id"], Value::Null);
    assert_eq!(
        fx.lease_state(&lease),
        "lease_ready",
        "an MCP session lease should be recycled, not terminated"
    );
}

#[test]
fn outcome_rollback_works_on_an_unclaimed_action_state() {
    let fx = Fixture::new("knots-rollback-outcome-unleased");
    let id = fx.knot_ready_for_shipment("No lease bound");
    assert_success(&fx.run(&["update", &id, "--status", "shipment"]));

    let rolled = fx.json(&["rollback", &id, "--outcome", "ci_failure", "--json"]);
    assert_eq!(rolled["state"], "ready_for_implementation");
    assert_eq!(
        rolled["lease_released"],
        Value::Bool(false),
        "there was no lease to release"
    );
    assert_eq!(fx.show(&id)["state"], "ready_for_implementation");
}

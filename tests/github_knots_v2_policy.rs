use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn policy() -> Value {
    let bytes = fs::read(root().join("config/github-knots-v2-rulesets.json"))
        .expect("ruleset policy should read");
    serde_json::from_slice(&bytes).expect("ruleset policy should parse")
}

#[test]
fn authority_rulesets_have_only_the_actions_bypass() {
    let policy = policy();
    assert_eq!(policy["trusted_integration_id"], 15368);
    for phase in ["canary", "production"] {
        for rule in policy["phases"][phase].as_array().expect("phase array") {
            if rule["trusted_bypass"] == true {
                assert!(rule["rules"]
                    .as_array()
                    .expect("rules")
                    .iter()
                    .any(|v| v == "update"));
            }
        }
    }
    let script = fs::read_to_string(root().join("scripts/repo/knots-v2-rulesets.mjs"))
        .expect("ruleset script should read");
    assert!(script.contains("actor_type: \"Integration\""));
    assert!(script.contains("actor.actor_type === \"RepositoryRole\""));
}

#[test]
fn production_patterns_and_inbox_rules_are_exact() {
    let policy = policy();
    let rules = policy["phases"]["production"]
        .as_array()
        .expect("production rules");
    let patterns: Vec<&str> = rules
        .iter()
        .flat_map(|rule| rule["patterns"].as_array().expect("patterns"))
        .map(|value| value.as_str().expect("pattern string"))
        .collect();
    assert_eq!(
        patterns,
        [
            "refs/heads/knots",
            "refs/heads/knots-v2-control",
            "refs/heads/knots-v2-canonical/*",
            "refs/heads/knots-v2-archive/*",
            "refs/heads/knots-v2-proposals/*/*",
            "refs/heads/knots-v2-writers/*",
            "refs/heads/knots-v2-inbox/*",
        ]
    );
    assert_eq!(
        rules.first().expect("legacy rule")["enforcement"],
        "disabled"
    );
    let inbox = rules.last().expect("inbox rule");
    assert_eq!(
        inbox["rules"],
        serde_json::json!(["creation", "update", "deletion", "non_fast_forward"])
    );
    assert_eq!(inbox["trusted_bypass"], true);
}

#[test]
fn trusted_workflow_constructs_authority_from_signed_data() {
    let workflow = fs::read_to_string(root().join(".github/workflows/knots-v2-integrate.yml"))
        .expect("workflow should read");
    assert!(workflow.contains("ref: main"));
    assert!(workflow.contains("persist-credentials: false"));
    assert!(!workflow.contains("ref: ${{ inputs.inbox"));
    assert!(workflow.contains("signed_submission_base64"));
    assert!(workflow.contains("workflow_dispatch:"));
    assert!(!workflow.contains("writer_signature"));
    assert!(!workflow.contains("canonical_oid:"));
    assert!(!workflow.contains("control_oid:"));

    let script = fs::read_to_string(root().join("scripts/repo/knots-v2-integrate.sh"))
        .expect("integrator script should read");
    assert!(script.contains("--verify-github-proposal"));
    assert!(script.contains("--signed-submission"));
    assert!(script.contains("--force-with-lease"));
    assert!(script.contains("--atomic"));
    assert!(script.contains("writer_registry_ref"));
    assert!(script.contains("signed_writer_verified"));
    assert!(!script.contains("publication.json"));
    assert!(!script.contains("checkout ${INBOX"));
    assert!(!script.contains("eval "));
}

#[test]
fn production_apply_requires_complete_canary_evidence() {
    let script = fs::read_to_string(root().join("scripts/repo/knots-v2-rulesets.mjs"))
        .expect("ruleset script should read");
    for signal in [
        "ordinary_protected_rejected",
        "ordinary_inbox_rejected",
        "ordinary_inbox_fast_forward_rejected",
        "actions_inbox_creation_succeeded",
        "actions_inbox_fast_forward_succeeded",
        "inbox_non_fast_forward_rejected",
        "inbox_deletion_rejected",
        "actions_protected_succeeded",
        "signed_writer_verified",
        "authority_constructed",
        "actions_run_id",
        "registry_oid",
    ] {
        assert!(script.contains(signal), "missing canary signal {signal}");
    }
}

#[test]
fn every_contents_write_workflow_pins_external_actions() {
    for name in [
        "changesets-version-pr.yml",
        "knots-v2-integrate.yml",
        "release.yml",
    ] {
        let workflow = fs::read_to_string(root().join(".github/workflows").join(name)).unwrap();
        assert!(workflow.contains("contents: write"));
        for line in workflow
            .lines()
            .filter(|line| line.trim().starts_with("uses:"))
        {
            let revision = line.split('@').nth(1).expect("action revision");
            let sha = revision.split_whitespace().next().unwrap();
            assert_eq!(sha.len(), 40, "{name} has an unpinned action: {line}");
            assert!(sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }
}

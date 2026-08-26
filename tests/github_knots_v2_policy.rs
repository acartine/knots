use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn policy() -> Value {
    let bytes = fs::read(root().join("config/github-knots-v2-rulesets.json"))
        .expect("ruleset policy should read");
    serde_json::from_slice(&bytes).expect("ruleset policy should parse")
}

#[test]
fn active_v2_rules_are_create_only_with_zero_bypass() {
    let policy = policy();
    assert!(policy.get("trusted_integration_id").is_none());
    for phase in ["authority_code", "canary", "production"] {
        for rule in policy["phases"][phase].as_array().expect("phase array") {
            if rule["enforcement"] == "disabled" {
                continue;
            }
            assert_eq!(
                rule["rules"],
                serde_json::json!(["update", "deletion", "non_fast_forward"])
            );
            assert!(rule.get("trusted_bypass").is_none());
        }
    }
    let script = fs::read_to_string(root().join("scripts/repo/knots-v2-rulesets.mjs"))
        .expect("ruleset script should read");
    assert!(script.contains("bypass_actors: []"));
    assert!(script.contains("relevant.bypass_actors.length !== 0"));
    assert!(!script.contains("RepositoryRole"));
    assert!(!script.contains("Integration"));
}

#[test]
fn authority_code_refs_are_immutable_before_any_dispatch() {
    let policy = policy();
    let rules = policy["phases"]["authority_code"]
        .as_array()
        .expect("authority code rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0]["patterns"],
        serde_json::json!(["refs/heads/knots-v2-authority-code/*"])
    );
    assert_eq!(
        rules[0]["rules"],
        serde_json::json!(["update", "deletion", "non_fast_forward"])
    );
}

#[test]
fn production_patterns_are_exact_and_legacy_freeze_is_disabled() {
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
            "refs/heads/knots-v2-control/epochs/**/*",
            "refs/heads/knots-v2-canonical/**/*",
            "refs/heads/knots-v2-archive/**/*",
            "refs/heads/knots-v2-proposals/*/*/*",
        ]
    );
    assert_eq!(rules[0]["enforcement"], "disabled");
    assert_eq!(
        rules[0]["rules"],
        serde_json::json!(["creation", "update", "deletion", "non_fast_forward"])
    );
}

#[test]
fn trusted_workflow_attests_exact_authority_code_manifest() {
    let workflow = fs::read_to_string(root().join(".github/workflows/knots-v2-control-epoch.yml"))
        .expect("workflow should read");
    for required in [
        "ref: ${{ github.sha }}",
        "id-token: write",
        "attestations: write",
        "artifact-metadata: write",
        "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
        "find-knots-v2-control-epoch.sh",
        "inspect-knots-v2-control-epoch.sh",
        "Prove Actions cannot rewrite or delete",
        "openssl rand -hex 16",
        "git config user.name",
        "refs/heads/knots-v2-authority-code/",
        "--source-ref \"${GITHUB_REF}\"",
        "--signer-digest \"${GITHUB_SHA}\"",
    ] {
        assert!(
            workflow.contains(required),
            "missing workflow binding {required}"
        );
    }
    assert!(!workflow.contains("secrets."));
    assert!(!workflow.contains("deploy key"));
    assert!(!workflow.contains("workflow_dispatch:"));

    let canary =
        fs::read_to_string(root().join(".github/workflows/knots-v2-control-epoch-canary.yml"))
            .expect("canary dispatcher should read");
    assert!(canary.contains("workflow_dispatch:"));
    assert!(canary.contains("namespace: canary"));
    assert!(!canary.contains("namespace: ${{"));

    let finder = fs::read_to_string(root().join("scripts/repo/find-knots-v2-control-epoch.sh"))
        .expect("authority finder should read");
    assert!(finder.contains("verify-chain"));

    let verifier = fs::read_to_string(root().join("scripts/repo/verify-knots-v2-control-epoch.sh"))
        .expect("verifier should read");
    for required in [
        "--repo \"${repo}\"",
        "--signer-workflow",
        "github-knots-v2-trusted-signers.json",
        ".sha == $sha and .ref == $ref and .workflow == $workflow",
        "--source-ref \"${source_ref}\"",
        "--source-digest \"${signer_sha}\"",
        "--signer-digest \"${signer_sha}\"",
        "--deny-self-hosted-runners",
    ] {
        assert!(
            verifier.contains(required),
            "missing verifier binding {required}"
        );
    }
    let signers = fs::read_to_string(root().join("config/github-knots-v2-trusted-signers.json"))
        .expect("trusted signer allowlist should read");
    assert!(!signers.contains("e869300032bde6e8257673a3d11ccd60"));
    assert!(!signers.contains("f41b6733d80f9253362d5aacb725a3050b473e08"));
    assert_eq!(
        serde_json::from_str::<Value>(&signers).unwrap()["trusted_signers"],
        serde_json::json!([{
            "ref": concat!(
                "refs/heads/knots-v2-authority-code/",
                "d3cd880046a8da7538179a4486e1ef28"
            ),
            "sha": "7eeaa578a963b95377fd0d03f240442f0bfa4dd9",
            "workflow": ".github/workflows/knots-v2-control-epoch.yml"
        }])
    );
}

#[test]
fn control_epoch_logic_rejects_gaps_and_non_dominating_vectors() {
    let status = Command::new("node")
        .arg("scripts/repo/knots-v2-control-epoch.mjs")
        .arg("self-test")
        .current_dir(root())
        .status()
        .expect("node should execute control epoch self-test");
    assert!(status.success());
}

#[test]
fn production_apply_requires_complete_attested_canary_evidence() {
    let script = fs::read_to_string(root().join("scripts/repo/knots-v2-rulesets.mjs"))
        .expect("ruleset script should read");
    for signal in [
        "ordinary_creation_succeeded",
        "ordinary_rewrite_rejected",
        "ordinary_deletion_rejected",
        "actions_attestation_verified",
        "actions_control_rewrite_rejected",
        "actions_control_deletion_rejected",
        "unattested_lookalike_rejected",
        "exact_workflow_head_verified",
        "manifest_sha256",
        "control_oid",
        "authority_sha",
        "authority_ref",
        "signer_workflow",
        "live canary authority does not match the activation evidence",
        "verify-knots-v2-control-epoch.sh",
        "refs/heads/main",
        "canary evidence does not bind the reviewed authority tuple",
    ] {
        assert!(script.contains(signal), "missing canary signal {signal}");
    }
    assert!(!script.contains("SIGNED_OUTBOX"));
}

#[test]
fn live_canary_driver_proves_create_only_and_exact_attestation() {
    let script = fs::read_to_string(root().join("scripts/repo/run-knots-v2-policy-canary.sh"))
        .expect("canary driver should read");
    for required in [
        "knots-v2-rulesets.mjs apply canary",
        "ordinary_creation_succeeded",
        "ordinary_rewrite_rejected",
        "ordinary_deletion_rejected",
        "unattested_lookalike_rejected",
        "exact_workflow_head_verified",
        "gh workflow run knots-v2-control-epoch-canary.yml",
        "verify-knots-v2-control-epoch.sh",
        "authority_sha",
        "authority_ref",
        "existing_control_ref",
        "lookalike_epoch",
        "probe_ref",
        "https://github.com/acartine/knots.git",
    ] {
        assert!(script.contains(required), "missing canary proof {required}");
    }
    assert!(!script.contains("secrets."));
    assert!(!script.contains("deploy key"));
    assert!(!script.contains("ls-remote origin"));
    assert!(!script.contains("--no-verify origin"));

    let verifier = fs::read_to_string(root().join("scripts/repo/verify-knots-v2-control-epoch.sh"))
        .expect("verifier should read");
    assert!(verifier.contains("gh run view"));
    assert!(verifier.contains(".conclusion == \"success\""));
    assert!(verifier.contains(".headSha == $sha"));
}

#[test]
fn mutable_inbox_promotion_surface_is_absent() {
    for path in [
        ".github/workflows/knots-v2-integrate.yml",
        "scripts/repo/knots-v2-integrate.sh",
        "src/compaction/github_submission.rs",
        "src/github_commands.rs",
    ] {
        assert!(
            !root().join(path).exists(),
            "mutable promotion remains at {path}"
        );
    }
    let cli = fs::read_to_string(root().join("src/cli_ops.rs")).unwrap();
    assert!(!cli.contains("verify_github_proposal"));
    assert!(!cli.contains("create_github_proposal_canary"));
}

#[test]
fn every_contents_write_workflow_pins_external_actions() {
    for name in [
        "changesets-version-pr.yml",
        "knots-v2-control-epoch-canary.yml",
        "knots-v2-control-epoch.yml",
        "release.yml",
    ] {
        let workflow = fs::read_to_string(root().join(".github/workflows").join(name)).unwrap();
        assert!(workflow.contains("contents: write"));
        for line in workflow
            .lines()
            .filter(|line| line.trim().starts_with("uses:"))
        {
            if line.contains("uses: ./") {
                continue;
            }
            let revision = line.split('@').nth(1).expect("action revision");
            let sha = revision.split_whitespace().next().unwrap();
            assert_eq!(sha.len(), 40, "{name} has an unpinned action: {line}");
            assert!(sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }
}

#[cfg(unix)]
#[test]
fn authority_shell_entrypoints_are_executable() {
    use std::os::unix::fs::PermissionsExt;

    for path in [
        "scripts/repo/find-knots-v2-control-epoch.sh",
        "scripts/repo/inspect-knots-v2-control-epoch.sh",
        "scripts/repo/verify-knots-v2-control-epoch.sh",
    ] {
        let mode = fs::metadata(root().join(path))
            .expect("authority entrypoint should exist")
            .permissions()
            .mode();
        assert_ne!(
            mode & 0o111,
            0,
            "authority entrypoint is not executable: {path}"
        );
    }
}

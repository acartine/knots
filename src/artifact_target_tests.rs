use crate::artifact_target::ArtifactTarget;
use std::str::FromStr;

#[test]
fn canonical_round_trip() {
    let targets = [
        "local",
        "remote",
        "remote_main",
        "pr",
        "branch",
        "note",
        "approval",
        "live_deployment",
    ];
    for name in targets {
        let parsed =
            ArtifactTarget::from_str(name).unwrap_or_else(|_| panic!("{name} should parse"));
        assert_eq!(parsed.as_str(), name);
        assert_eq!(parsed.to_string(), name);
    }
}

#[test]
fn unknown_rejected() {
    for bad in ["foobar", "", "deploymentt", "PR", "Branch", "LOCAL"] {
        assert!(
            ArtifactTarget::from_str(bad).is_err(),
            "{bad:?} should be rejected"
        );
    }
}

#[test]
fn error_displays_value() {
    let err = ArtifactTarget::from_str("oops").unwrap_err();
    assert!(err.to_string().contains("oops"));
}

#[test]
fn all_variants_distinct() {
    use std::collections::HashSet;
    let all = [
        ArtifactTarget::Local,
        ArtifactTarget::Remote,
        ArtifactTarget::RemoteMain,
        ArtifactTarget::Pr,
        ArtifactTarget::Branch,
        ArtifactTarget::Note,
        ArtifactTarget::Approval,
        ArtifactTarget::LiveDeployment,
    ];
    let names: HashSet<_> = all.iter().map(|t| t.as_str()).collect();
    assert_eq!(names.len(), all.len());
}

use std::path::Path;
use std::process::Command;

const EVENT: &[u8] = include_bytes!("fixtures/releases/v0.17.6/event.json");
const INDEX: &[u8] = include_bytes!("fixtures/releases/v0.17.6/index.json");
const PROVENANCE: &str = include_str!("fixtures/releases/v0.17.6/provenance.json");

#[test]
fn released_v0176_legacy_publish_cannot_change_canonical_v2_hash() {
    assert!(PROVENANCE.contains("d67c43d165fb54a3f830a7d455ee72361f4c3a18"));
    assert!(PROVENANCE.contains("723ad12dc354a71c1dcf045b163318ae1b617c2430e9db76a2753ade40c805ad"));
    let remote_ws = knots_test_support::workspace("knots-v0176-v2-remote");
    let client_ws = knots_test_support::workspace("knots-v0176-v2-client");
    let remote = remote_ws.path();
    let client = client_ws.path();
    git(remote, &["init", "--bare"]);
    git(client, &["init"]);
    git(client, &["config", "user.email", "v0176@example.com"]);
    git(client, &["config", "user.name", "Knots v0.17.6 fixture"]);
    git(
        client,
        &["remote", "add", "origin", &remote.display().to_string()],
    );

    write(client, ".knots/v2/control/record.json", b"{\"epoch\":1}\n");
    write(
        client,
        ".knots/v2/packs/pack-fixture.pack",
        b"canonical bytes\n",
    );
    git(client, &["add", "-f", ".knots/v2"]);
    git(client, &["commit", "-m", "seed protected v2"]);
    let canonical_ref = "refs/heads/knots-v2-canonical/fixture";
    git(
        client,
        &["push", "origin", &format!("HEAD:{canonical_ref}")],
    );
    let canonical_oid = output(remote, &["rev-parse", canonical_ref]);
    let canonical_tree = output(
        remote,
        &["rev-parse", &format!("{canonical_ref}:.knots/v2")],
    );

    write(
        client,
        ".knots/events/2026/08/25/v0176-knot.created.json",
        EVENT,
    );
    write(
        client,
        ".knots/index/2026/08/25/v0176-idx.knot_head.json",
        INDEX,
    );
    git(client, &["add", "-f", ".knots/events", ".knots/index"]);
    git(client, &["commit", "-m", "knots: publish local events"]);
    git(client, &["push", "origin", "HEAD:refs/heads/knots"]);

    assert_eq!(output(remote, &["rev-parse", canonical_ref]), canonical_oid);
    assert_eq!(
        output(
            remote,
            &["rev-parse", &format!("{canonical_ref}:.knots/v2")]
        ),
        canonical_tree
    );
    assert!(output(remote, &["ls-tree", "-r", "refs/heads/knots"])
        .contains(".knots/events/2026/08/25/v0176-knot.created.json"));
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    std::fs::write(path, bytes).expect("write fixture");
}

fn git(repo: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        result.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn output(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git");
    assert!(result.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&result.stdout).trim().to_string()
}

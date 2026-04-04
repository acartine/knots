use std::path::PathBuf;
use std::process::Command;

use super::App;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-show-lease-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn run_git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo(root: &std::path::Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "knots@example.com"]);
    run_git(root, &["config", "user.name", "Knots Test"]);
    std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-m", "init"]);
    run_git(root, &["branch", "-M", "main"]);
}

fn open_app(root: &std::path::Path) -> App {
    let db_path = root.join(".knots/cache/state.sqlite");
    App::open(db_path.to_str().expect("utf8 db path"), root.to_path_buf()).expect("app should open")
}

#[test]
fn show_knot_populates_lease_agent_from_bound_lease_record() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let work = app
        .create_knot("Lease-bound work", None, Some("work_item"), Some("default"))
        .expect("work knot should be created");
    let lease = crate::lease::create_lease(
        &app,
        "test-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(crate::domain::lease::AgentInfo {
            agent_type: "cli".to_string(),
            provider: "Anthropic".to_string(),
            agent_name: "claude".to_string(),
            model: "opus".to_string(),
            model_version: "4.6".to_string(),
        }),
        600,
    )
    .expect("lease should be created");
    crate::lease::bind_lease(&app, &work.id, &lease.id).expect("bind should succeed");

    let shown = app
        .show_knot(&work.id)
        .expect("show should succeed")
        .expect("work knot should exist");
    let agent = shown
        .lease_agent
        .expect("bound lease agent should be exposed");
    assert_eq!(shown.lease_id.as_deref(), Some(lease.id.as_str()));
    assert_eq!(agent.agent_type, "cli");
    assert_eq!(agent.provider, "Anthropic");
    assert_eq!(agent.agent_name, "claude");
    assert_eq!(agent.model, "opus");
    assert_eq!(agent.model_version, "4.6");

    let _ = std::fs::remove_dir_all(root);
}

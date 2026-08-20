use std::path::PathBuf;
use std::process::Command;

use super::App;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-app-list-lease-{}", uuid::Uuid::now_v7()));
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
fn list_knots_populates_lease_agent_for_active_knot_and_omits_for_queued() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let active = app
        .create_knot(
            "Active lease-bound work",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("active knot should be created");
    let queued = app
        .create_knot(
            "Queued unleased work",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("queued knot should be created");

    let lease = crate::lease::create_lease(
        &app,
        "list-lease",
        crate::domain::lease::LeaseType::Agent,
        Some(crate::domain::lease::AgentInfo {
            agent_type: "cli".to_string(),
            provider: "Anthropic".to_string(),
            agent_name: "claude".to_string(),
            model: "opus".to_string(),
            model_version: "4.7".to_string(),
        }),
        600,
    )
    .expect("lease should be created");
    crate::lease::bind_lease(&app, &active.id, &lease.id).expect("bind should succeed");

    let listing = app.list_knots().expect("list should succeed");
    let active_view = listing
        .iter()
        .find(|k| k.id == active.id)
        .expect("active knot should appear in listing");
    let queued_view = listing
        .iter()
        .find(|k| k.id == queued.id)
        .expect("queued knot should appear in listing");

    let agent = active_view
        .lease_agent
        .as_ref()
        .expect("active knot should expose lease agent in listing");
    assert_eq!(active_view.lease_id.as_deref(), Some(lease.id.as_str()));
    assert_eq!(agent.agent_type, "cli");
    assert_eq!(agent.provider, "Anthropic");
    assert_eq!(agent.agent_name, "claude");
    assert_eq!(agent.model, "opus");
    assert_eq!(agent.model_version, "4.7");

    assert!(
        queued_view.lease_id.is_none(),
        "queued knot should have no bound lease id"
    );
    assert!(
        queued_view.lease_agent.is_none(),
        "queued knot must not pretend a lease is present"
    );

    let _ = std::fs::remove_dir_all(root);
}

/// Create a lease knot owned by another machine, as a pull from that machine
/// would leave it in this cache.
fn create_foreign_lease(app: &App, nickname: &str) -> String {
    let lease_data = crate::domain::lease::LeaseData {
        lease_type: crate::domain::lease::LeaseType::Manual,
        nickname: nickname.to_string(),
        owner: Some(crate::domain::lease::LeaseOwner {
            machine_id: "another-machine".to_string(),
            pid: 4242,
        }),
        ..Default::default()
    };
    let lease = app
        .create_knot_with_options(
            &format!("Lease: {}", nickname),
            None,
            Some("lease_ready"),
            None,
            None,
            super::CreateKnotOptions {
                knot_type: crate::domain::knot_type::KnotType::Lease,
                lease_data,
                ..super::CreateKnotOptions::default()
            },
        )
        .expect("foreign lease should be created");
    app.set_lease_expiry(&lease.id, crate::lease_expiry::compute_expiry_ts(600))
        .expect("expiry should be settable");
    lease.id
}

#[test]
fn created_leases_record_this_machine_as_owner() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let lease = crate::lease::create_lease(
        &app,
        "owner-stamp",
        crate::domain::lease::LeaseType::Manual,
        None,
        600,
    )
    .expect("lease should be created");

    let owner = lease
        .lease
        .as_ref()
        .and_then(|data| data.owner.as_ref())
        .expect("a newly created lease must record its owner");
    assert_eq!(
        owner.machine_id,
        app.machine_id().expect("machine id should resolve")
    );
    assert_eq!(owner.pid, std::process::id());
    assert!(!owner.machine_id.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn list_local_active_leases_excludes_leases_owned_elsewhere() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let mine = crate::lease::create_lease(
        &app,
        "mine",
        crate::domain::lease::LeaseType::Manual,
        None,
        600,
    )
    .expect("local lease should be created");
    let theirs = create_foreign_lease(&app, "theirs");

    let all = crate::lease::list_active_leases(&app).expect("list should succeed");
    let mut all_ids: Vec<&str> = all.iter().map(|k| k.id.as_str()).collect();
    all_ids.sort_unstable();
    assert_eq!(all_ids.len(), 2, "both leases are active: {:?}", all_ids);
    assert!(all_ids.contains(&theirs.as_str()));

    let local = crate::lease::list_local_active_leases(&app).expect("list should succeed");
    let local_ids: Vec<&str> = local.iter().map(|k| k.id.as_str()).collect();
    assert_eq!(
        local_ids,
        vec![mine.id.as_str()],
        "only this machine's lease is locally held"
    );

    let _ = std::fs::remove_dir_all(root);
}

use crate::write_dispatch::execute::execute_operation;
use crate::write_queue::{LeaseCreateOperation, LeaseExtendOperation, WriteOperation};

use super::tests_lease_ext::{open_app, setup_repo, unique_workspace};

fn create_active_lease(app: &crate::app::App) -> String {
    let op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "extend-test".to_string(),
        lease_type: "agent".to_string(),
        agent_type: Some("cli".to_string()),
        provider: Some("test".to_string()),
        agent_name: Some("agent".to_string()),
        model: Some("model".to_string()),
        model_version: Some("1.0".to_string()),
        json: false,
        timeout_seconds: None,
    });
    execute_operation(app, &op).expect("create should succeed");
    let leases = crate::lease::list_active_leases(app).expect("list");
    let lease = leases.into_iter().last().expect("lease exists");
    crate::lease::activate_lease(app, &lease.id).expect("activate");
    lease.id
}

#[test]
fn extend_active_lease_succeeds() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let lease_id = create_active_lease(&app);

    let op = WriteOperation::LeaseExtend(LeaseExtendOperation {
        lease_id: lease_id.clone(),
        timeout_seconds: Some(1200),
        json: false,
    });
    let output = execute_operation(&app, &op).expect("extend should succeed");
    assert!(
        output.contains("extended"),
        "output should say extended: {output}"
    );
    assert!(
        output.contains("1200s"),
        "output should include timeout: {output}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extend_active_lease_json_output() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let lease_id = create_active_lease(&app);

    let op = WriteOperation::LeaseExtend(LeaseExtendOperation {
        lease_id: lease_id.clone(),
        timeout_seconds: None,
        json: true,
    });
    let output = execute_operation(&app, &op).expect("extend should succeed");
    let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid json");
    assert_eq!(parsed["id"].as_str().unwrap(), lease_id);
    assert_eq!(parsed["timeout_seconds"].as_u64().unwrap(), 600);
    assert!(parsed["lease_expiry_ts"].as_i64().is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extend_terminated_lease_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let lease_id = create_active_lease(&app);
    crate::lease::terminate_lease(&app, &lease_id).expect("terminate");

    let op = WriteOperation::LeaseExtend(LeaseExtendOperation {
        lease_id,
        timeout_seconds: None,
        json: false,
    });
    let err = execute_operation(&app, &op).expect_err("extend should fail");
    assert!(
        err.to_string().contains("terminated or expired"),
        "error should mention terminated: {}",
        err
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extend_non_lease_knot_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);
    let work = app
        .create_knot("Not a lease", None, Some("work_item"), Some("default"))
        .expect("create");

    let op = WriteOperation::LeaseExtend(LeaseExtendOperation {
        lease_id: work.id,
        timeout_seconds: None,
        json: false,
    });
    let err = execute_operation(&app, &op).expect_err("extend should fail");
    assert!(
        err.to_string().contains("not a lease"),
        "error should mention not a lease: {}",
        err
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn extend_nonexistent_lease_fails() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let op = WriteOperation::LeaseExtend(LeaseExtendOperation {
        lease_id: "nonexistent-id".to_string(),
        timeout_seconds: None,
        json: false,
    });
    let err = execute_operation(&app, &op).expect_err("extend should fail");
    assert!(
        err.to_string().contains("not found"),
        "error should mention not found: {}",
        err
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lease_create_manual_type_succeeds() {
    let root = unique_workspace();
    setup_repo(&root);
    let app = open_app(&root);

    let op = WriteOperation::LeaseCreate(LeaseCreateOperation {
        nickname: "manual-lease".to_string(),
        lease_type: "manual".to_string(),
        agent_type: None,
        provider: None,
        agent_name: None,
        model: None,
        model_version: None,
        json: false,
        timeout_seconds: Some(300),
    });
    let output = execute_operation(&app, &op).expect("create manual");
    assert!(output.contains("created lease"), "output: {output}");

    let _ = std::fs::remove_dir_all(root);
}

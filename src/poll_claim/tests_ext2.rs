use super::*;
use std::path::PathBuf;

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-poll-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

#[test]
fn run_ready_empty_queue_prints_message() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let args = ReadyArgs {
        ready_type: None,
        json: false,
    };
    run_ready(&app, args).expect("run_ready should succeed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_ready_json_empty_queue() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let args = ReadyArgs {
        ready_type: None,
        json: true,
    };
    run_ready(&app, args).expect("run_ready json should succeed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn peek_knot_does_not_advance_state() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let created = app
        .create_knot("Peek test", None, Some("work_item"), Some("default"))
        .expect("create should succeed");
    let original_state = created.state.clone();
    let result = peek_knot(&app, &created.id);
    assert!(result.is_ok(), "peek_knot should succeed");
    let after = app
        .show_knot(&created.id)
        .expect("show should succeed")
        .expect("knot should exist");
    assert_eq!(after.state, original_state, "state should be unchanged");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_ready_with_knot_in_queue() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    app.create_knot("Test ready", None, Some("work_item"), Some("default"))
        .expect("create should succeed");
    let args = ReadyArgs {
        ready_type: None,
        json: false,
    };
    run_ready(&app, args).expect("run_ready with knot should succeed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn peek_knot_completion_command_has_agent_metadata_flags() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let created = app
        .create_knot(
            "Peek completion command",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create should succeed");
    let result = peek_knot(&app, &created.id).expect("peek_knot should succeed");
    assert_eq!(
        result.completion_cmd,
        completion_command(&created.id, "implementation", None)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn claim_rejects_knot_in_action_state() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let created = app
        .create_knot(
            "Action guard test",
            None,
            Some("work_item"),
            Some("default"),
        )
        .expect("create should succeed");
    // Advance to action state (planning)
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    };
    app.set_state_with_actor(&created.id, "implementation", false, None, actor.clone())
        .expect("advance should succeed");
    let result = claim_knot(&app, &created.id, actor, None);
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("claim should reject action state"),
    };
    assert!(
        err.contains("not a claimable queue state"),
        "error should mention queue state: {err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn peek_rejects_knot_in_action_state() {
    let root = unique_workspace();
    let db_path = root.join(".knots/cache/state.sqlite");
    let app = App::open(db_path.to_str().expect("utf8"), root.clone()).expect("app should open");
    let created = app
        .create_knot("Peek guard test", None, Some("work_item"), Some("default"))
        .expect("create should succeed");
    let actor = StateActorMetadata {
        actor_kind: Some("agent".to_string()),
        agent_name: None,
        agent_model: None,
        agent_version: None,
    };
    app.set_state_with_actor(&created.id, "implementation", false, None, actor)
        .expect("advance should succeed");
    let result = peek_knot(&app, &created.id);
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("peek should reject action state"),
    };
    assert!(
        err.contains("not a claimable queue state"),
        "error should mention queue state: {err}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn prompt_body_for_state_distinguishes_branch_and_pr_profiles() {
    let root = unique_workspace();
    let registry =
        crate::profile::ProfileRegistry::load_for_repo(&root).expect("registry should load");

    for profile_id in [
        "autopilot",
        "semiauto",
        "autopilot_no_planning",
        "semiauto_no_planning",
    ] {
        let implementation =
            prompt_body_for_state(&registry, profile_id, "implementation").expect("prompt body");
        assert!(
            !implementation.contains("pull request"),
            "{profile_id}: {implementation}"
        );

        let review = prompt_body_for_state(&registry, profile_id, "implementation_review")
            .expect("review prompt body");
        assert!(!review.contains("pull request"), "{profile_id}: {review}");

        let shipment =
            prompt_body_for_state(&registry, profile_id, "shipment").expect("shipment prompt");
        assert!(shipment.contains("merge the feature branch to main"));
        assert!(
            !shipment.contains("pull request"),
            "{profile_id}: {shipment}"
        );

        let shipment_review = prompt_body_for_state(&registry, profile_id, "shipment_review")
            .expect("shipment review prompt");
        assert!(shipment_review.contains("review the code now on main"));
        assert!(
            !shipment_review.contains("pull request"),
            "{profile_id}: {shipment_review}"
        );
    }

    for profile_id in ["autopilot_with_pr", "autopilot_with_pr_no_planning"] {
        let implementation =
            prompt_body_for_state(&registry, profile_id, "implementation").expect("prompt body");
        assert!(
            implementation.contains("pull request"),
            "{profile_id}: {implementation}"
        );

        let review = prompt_body_for_state(&registry, profile_id, "implementation_review")
            .expect("review prompt body");
        assert!(review.contains("pull request"), "{profile_id}: {review}");

        let shipment =
            prompt_body_for_state(&registry, profile_id, "shipment").expect("shipment prompt");
        assert!(shipment.contains("merge the approved pull request"));

        let shipment_review = prompt_body_for_state(&registry, profile_id, "shipment_review")
            .expect("shipment review prompt");
        assert!(shipment_review.contains("review the merged pull request"));
    }

    let _ = std::fs::remove_dir_all(root);
}

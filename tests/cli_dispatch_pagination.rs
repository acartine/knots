mod cli_dispatch_helpers;

use std::collections::HashSet;

use cli_dispatch_helpers::*;
use serde_json::Value;

fn ls_json_page(repo_root: &std::path::Path, db: &std::path::Path, extra: &[&str]) -> Value {
    let mut args = vec!["ls", "--json"];
    args.extend_from_slice(extra);
    let output = run_knots(repo_root, db, &args);
    assert_success(&output);
    serde_json::from_slice(&output.stdout).expect("ls json should parse")
}

fn create_knot_with_tag(
    repo_root: &std::path::Path,
    db: &std::path::Path,
    title: &str,
    tag: &str,
) -> String {
    let output = run_knots(
        repo_root,
        db,
        &["new", title, "--tag", tag, "--profile", "autopilot"],
    );
    assert_success(&output);
    parse_created_id(&output)
}

fn create_knot_with_state(
    repo_root: &std::path::Path,
    db: &std::path::Path,
    title: &str,
    state: &str,
) -> String {
    let output = run_knots(
        repo_root,
        db,
        &["new", title, "--profile", "autopilot", "--state", state],
    );
    assert_success(&output);
    parse_created_id(&output)
}

#[test]
fn ls_tag_filter_pagination_is_stable_and_total_matches_filtered() {
    let root_ws = unique_workspace("knots-cli-ls-tag-pagination");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    let mut tagged_ids = Vec::new();
    for i in 0..5 {
        let id = create_knot_with_tag(&root, &db, &format!("Tagged knot {i}"), "approval-harness");
        tagged_ids.push(id);
    }

    for i in 0..3 {
        let output = run_knots(
            &root,
            &db,
            &[
                "new",
                &format!("Untagged knot {i}"),
                "--profile",
                "autopilot",
            ],
        );
        assert_success(&output);
    }

    let page_limit_1 = ls_json_page(&root, &db, &["--tag", "approval-harness", "--limit", "1"]);
    assert_eq!(page_limit_1["total"], 5, "total should be 5");
    let data = page_limit_1["data"]
        .as_array()
        .expect("data should be array");
    assert_eq!(data.len(), 1, "limit 1 should return 1 item");

    let page_limit_1000 = ls_json_page(
        &root,
        &db,
        &["--tag", "approval-harness", "--limit", "1000"],
    );
    assert_eq!(page_limit_1000["total"], 5, "total should be 5");
    let data = page_limit_1000["data"]
        .as_array()
        .expect("data should be array");
    assert_eq!(data.len(), 5, "limit 1000 should return all 5 items");

    let mut collected = HashSet::new();
    let mut offset = 0;
    loop {
        let page = ls_json_page(
            &root,
            &db,
            &[
                "--tag",
                "approval-harness",
                "--limit",
                "2",
                "--offset",
                &offset.to_string(),
            ],
        );
        let data = page["data"].as_array().expect("data should be array");
        for item in data {
            let id = item["id"].as_str().expect("each item should have an id");
            collected.insert(id.to_string());
        }
        if !page["has_more"].as_bool().unwrap_or(false) {
            break;
        }
        offset += data.len();
    }

    assert_eq!(
        collected.len(),
        5,
        "paging through should collect exactly 5 unique ids"
    );
    for expected in &tagged_ids {
        assert!(
            collected.iter().any(|id| id.ends_with(expected)),
            "missing expected id suffix {expected}"
        );
    }
}

#[test]
fn ls_state_shipped_pagination_returns_only_shipped() {
    let root_ws = unique_workspace("knots-cli-ls-state-pagination");
    let root = root_ws.path().to_path_buf();
    setup_repo(&root);
    let db = root.join(".knots/cache/state.sqlite");

    for i in 0..3 {
        create_knot_with_state(&root, &db, &format!("Shipped knot {i}"), "shipped");
    }
    for i in 0..2 {
        create_knot_with_state(&root, &db, &format!("Idea knot {i}"), "idea");
    }

    let page = ls_json_page(&root, &db, &["--state", "shipped", "--limit", "2"]);
    assert_eq!(page["total"], 3, "total should be 3");
    let data = page["data"].as_array().expect("data should be array");
    assert_eq!(data.len(), 2, "limit 2 should return 2 items");

    for item in data {
        let state = item["state"]
            .as_str()
            .expect("each item should have a state");
        assert_eq!(state, "shipped", "all items should have state shipped");
    }
}

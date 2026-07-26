//! Parity gate between `loom/work_sdlc/prompts/*.md` and the compiled
//! `loom/work_sdlc/dist/bundle.json`.
//!
//! The dist bundle once dropped every prompt's `## Failure Modes` section
//! while keeping the outcome map, so the runtime prompt told agents nothing
//! about how to record a failure. Nothing caught it because the bundle is a
//! generated artifact that is committed by hand. These tests compare the two
//! files directly, so they fail on drift without needing the `loom` binary.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

fn workflow_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("loom/work_sdlc")
}

fn bundle_prompts() -> BTreeMap<String, Value> {
    let raw = std::fs::read_to_string(workflow_dir().join("dist/bundle.json"))
        .expect("dist/bundle.json should be readable");
    let bundle: Value = serde_json::from_str(&raw).expect("bundle should be valid JSON");
    bundle["prompts"]
        .as_array()
        .expect("bundle should carry a prompts array")
        .iter()
        .map(|prompt| {
            let name = prompt["name"]
                .as_str()
                .expect("bundle prompt should be named")
                .to_string();
            (name, prompt.clone())
        })
        .collect()
}

/// Source prompts: name -> (frontmatter, body).
fn source_prompts() -> BTreeMap<String, (String, String)> {
    let dir = workflow_dir().join("prompts");
    let mut prompts = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("prompts dir should be readable") {
        let path = entry.expect("dir entry should be readable").path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("prompt file should have a stem")
            .to_string();
        let raw = std::fs::read_to_string(&path).expect("prompt should be readable");
        let rest = raw
            .strip_prefix("---\n")
            .expect("prompt should open with frontmatter");
        let (frontmatter, body) = rest
            .split_once("\n---\n")
            .expect("prompt frontmatter should terminate");
        prompts.insert(name, (frontmatter.to_string(), body.to_string()));
    }
    prompts
}

/// Parse the `failure:` block of a prompt's YAML frontmatter.
fn source_failure_outcomes(frontmatter: &str) -> BTreeMap<String, String> {
    let mut outcomes = BTreeMap::new();
    let mut in_failure = false;
    for line in frontmatter.lines() {
        if line.starts_with("failure:") {
            in_failure = true;
            continue;
        }
        if in_failure {
            if !line.starts_with("  ") {
                if line.trim().is_empty() {
                    continue;
                }
                in_failure = false;
                continue;
            }
            let (name, target) = line
                .trim()
                .split_once(':')
                .expect("failure entry should be name: target");
            outcomes.insert(name.trim().to_string(), target.trim().to_string());
        }
    }
    outcomes
}

fn bundle_failure_outcomes(prompt: &Value) -> BTreeMap<String, String> {
    prompt["outcomes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|outcome| !outcome["is_success"].as_bool().unwrap_or(false))
        .map(|outcome| {
            (
                outcome["name"].as_str().unwrap_or_default().to_string(),
                outcome["target"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

#[test]
fn bundle_covers_every_source_prompt() {
    let bundle = bundle_prompts();
    for name in source_prompts().keys() {
        assert!(
            bundle.contains_key(name),
            "dist/bundle.json is missing prompt '{name}'; run `make loom-bundle`"
        );
    }
}

#[test]
fn bundle_failure_maps_match_source_frontmatter() {
    let bundle = bundle_prompts();
    for (name, (frontmatter, _)) in source_prompts() {
        let expected = source_failure_outcomes(&frontmatter);
        let actual = bundle_failure_outcomes(&bundle[&name]);
        assert_eq!(
            expected, actual,
            "failure outcomes for '{name}' drifted from the source prompt; \
             run `make loom-bundle`"
        );
    }
}

#[test]
fn bundle_bodies_retain_the_failure_modes_section() {
    let bundle = bundle_prompts();
    for (name, (_, body)) in source_prompts() {
        let Some(source_section) = body.split_once("## Failure Modes") else {
            continue;
        };
        let compiled = bundle[&name]["body"]
            .as_str()
            .expect("bundle prompt should carry a body");
        assert!(
            compiled.contains("## Failure Modes"),
            "compiled prompt '{name}' dropped its Failure Modes section; \
             run `make loom-bundle`"
        );
        // Every command line in the source section must survive compilation.
        for line in source_section.1.lines() {
            let line = line.trim();
            if !line.starts_with("`kno ") {
                continue;
            }
            assert!(
                compiled.contains(line),
                "compiled prompt '{name}' is missing failure command {line}; \
                 run `make loom-bundle`"
            );
        }
    }
}

#[test]
fn every_declared_failure_outcome_has_a_documented_command() {
    let bundle = bundle_prompts();
    for (name, prompt) in &bundle {
        let body = prompt["body"].as_str().unwrap_or_default();
        for outcome in bundle_failure_outcomes(prompt).keys() {
            let command = format!("kno rollback <id> --outcome {outcome}");
            assert!(
                body.contains(&command),
                "prompt '{name}' declares failure outcome '{outcome}' but its \
                 body never documents `{command}`"
            );
        }
    }
}

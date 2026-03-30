use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

use super::*;

pub const SAMPLE_BUNDLE: &str = r#"
[workflow]
name = "custom_flow"
version = 3
default_profile = "autopilot"

[states.ready_for_work]
display_name = "Ready for Work"
kind = "queue"

[states.work]
display_name = "Work"
kind = "action"
action_type = "produce"
executor = "agent"
prompt = "work"
output = "branch"
output_hint = "git log"

[states.review]
display_name = "Review"
kind = "action"
action_type = "gate"
executor = "human"
prompt = "review"
output = "note"

[states.ready_for_review]
display_name = "Ready for Review"
kind = "queue"

[states.done]
display_name = "Done"
kind = "terminal"

[states.blocked]
display_name = "Blocked"
kind = "escape"

[states.deferred]
display_name = "Deferred"
kind = "escape"

[states.abandoned]
display_name = "Abandoned"
kind = "terminal"

[steps.impl]
queue = "ready_for_work"
action = "work"

[steps.rev]
queue = "ready_for_review"
action = "review"

[phases.main]
produce = "impl"
gate = "rev"

[profiles.autopilot]
description = "Custom profile"
phases = ["main"]

[prompts.work]
accept = ["Built output"]
body = """
Ship {{ output }} output.
"""

[prompts.work.success]
complete = "ready_for_review"

[prompts.work.failure]
blocked = "blocked"

[prompts.review]
accept = ["Reviewed output"]
body = """
Review it.
"""

[prompts.review.success]
approved = "done"

[prompts.review.failure]
changes = "ready_for_work"
"#;

pub fn unique_workspace(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
    root
}

pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn build_prompt_params(
    workflow: &WorkflowDefinition,
    profile: &crate::profile::ProfileDefinition,
    prompt: &PromptDefinition,
) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    params.insert("workflow_id".to_string(), workflow.id.clone());
    params.insert("profile_id".to_string(), profile.id.clone());
    if let Some(output_def) = profile.outputs.get(&prompt.action_state) {
        params.insert("output".to_string(), output_def.artifact_type.clone());
        if let Some(hint) = &output_def.access_hint {
            params.insert("output_hint".to_string(), hint.clone());
        }
    }
    for param in &prompt.params {
        if let Some(default) = param.default.as_deref() {
            params
                .entry(param.name.clone())
                .or_insert_with(|| default.to_string());
        }
    }
    params
}

pub fn render_prompt_template(
    template: &str,
    params: &BTreeMap<String, String>,
    unresolved: &mut Vec<String>,
) -> String {
    let mut rendered = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            rendered.push_str(&rest[start..]);
            return rendered;
        };
        let token = &after_start[..end];
        let key = token.trim();
        if let Some(value) = params.get(key) {
            rendered.push_str(value);
        } else {
            unresolved.push(key.to_string());
            rendered.push_str("{{");
            rendered.push_str(token);
            rendered.push_str("}}");
        }
        rest = &after_start[end + 2..];
    }
    rendered.push_str(rest);
    unresolved.sort();
    unresolved.dedup();
    rendered
}

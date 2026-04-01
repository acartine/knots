use crate::profile::ProfileDefinition;

pub fn render_for_profile(profile: &ProfileDefinition, action_state: &str) -> Option<String> {
    let prompt_body = profile.prompt_for_action_state(action_state)?;
    let mut rendered = prompt_body.trim().to_string();
    let acceptance = profile.acceptance_for_action_state(action_state);
    if !acceptance.is_empty() {
        if !rendered.is_empty() {
            rendered.push_str("\n\n");
        }
        rendered.push_str("## Acceptance Criteria\n\n");
        for item in acceptance {
            rendered.push_str("- ");
            rendered.push_str(item);
            rendered.push('\n');
        }
    }
    Some(rendered)
}

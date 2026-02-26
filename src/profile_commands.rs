use std::io::IsTerminal;

use crate::app;
use crate::cli;
use crate::workflow;
use crate::workflow_diagram;

pub(crate) fn run_profile_command(
    args: &cli::ProfileArgs,
    repo_root: &std::path::Path,
    db_path: &str,
) -> Result<(), app::AppError> {
    use cli::ProfileSubcommands;

    let registry = workflow::ProfileRegistry::load()?;
    let palette = ProfilePalette::auto();
    match &args.command {
        ProfileSubcommands::List(list_args) => {
            let profiles = registry.list();
            if list_args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&profiles)
                        .expect("json serialization should work")
                );
            } else if profiles.is_empty() {
                println!("{}", palette.dim("no profiles found"));
            } else {
                println!("{}", palette.heading("Profiles"));
                let count = profiles.len();
                for (index, profile) in profiles.into_iter().enumerate() {
                    if index > 0 {
                        println!();
                    }
                    let profile_name = profile
                        .description
                        .as_deref()
                        .unwrap_or(profile.id.as_str());
                    let fields = vec![
                        ProfileField::new("name", profile_name),
                        ProfileField::new("id", profile.id.clone()),
                        ProfileField::new(
                            "planning",
                            format_profile_gate_mode(&profile.planning_mode),
                        ),
                        ProfileField::new(
                            "impl_review",
                            format_profile_gate_mode(&profile.implementation_review_mode),
                        ),
                        ProfileField::new("output", format_profile_output_mode(&profile.output)),
                        ProfileField::new("initial_state", profile.initial_state.clone()),
                        ProfileField::new("terminal_states", profile.terminal_states.join(", ")),
                    ];
                    for line in format_profile_fields(&fields, &palette) {
                        println!("{line}");
                    }
                }
                if count > 1 {
                    println!();
                }
                println!("{}", palette.dim(&format!("{count} profile(s)")));
            }
        }
        ProfileSubcommands::Show(show_args) => {
            let profile = registry.require(&show_args.id)?.clone();
            if show_args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&profile).expect("json serialization should work")
                );
            } else {
                println!("{}", palette.heading("Profile"));
                let mut fields = vec![
                    ProfileField::new("id", profile.id.clone()),
                    ProfileField::new("planning", format_profile_gate_mode(&profile.planning_mode)),
                    ProfileField::new(
                        "impl_review",
                        format_profile_gate_mode(&profile.implementation_review_mode),
                    ),
                    ProfileField::new("output", format_profile_output_mode(&profile.output)),
                    ProfileField::new("initial_state", profile.initial_state.clone()),
                    ProfileField::new("terminal_states", profile.terminal_states.join(", ")),
                ];
                if let Some(description) = profile.description.as_deref() {
                    fields.insert(1, ProfileField::new("description", description));
                }
                for line in format_profile_fields(&fields, &palette) {
                    println!("{line}");
                }
                println!("{}", palette.dim("workflow:"));
                for line in workflow_diagram::render(&profile) {
                    println!("  {line}");
                }
            }
        }
        ProfileSubcommands::SetDefault(set_default_args) => {
            let app = app::App::open(db_path, repo_root.to_path_buf())?;
            let profile_id = app.set_default_profile_id(&set_default_args.id)?;
            println!("default profile: {}", profile_id);
        }
        ProfileSubcommands::Set(set_args) => {
            let app = app::App::open(db_path, repo_root.to_path_buf())?;
            let profile = registry.require(&set_args.profile)?;
            let current = app
                .show_knot(&set_args.id)?
                .ok_or_else(|| app::AppError::NotFound(set_args.id.clone()))?;
            let state = resolve_profile_state_selection(
                profile,
                set_args.state.as_deref(),
                &current.state,
            )?;
            let knot = app.set_profile(
                &set_args.id,
                &profile.id,
                &state,
                set_args.if_match.as_deref(),
            )?;
            let short_id = crate::knot_id::display_id(&knot.id);
            let knot_label = match knot.alias.as_deref() {
                Some(alias) => format!("{alias} ({short_id})"),
                None => short_id.to_string(),
            };
            println!(
                "updated {} [{}] profile={}",
                knot_label, knot.state, knot.profile_id
            );
        }
    }
    Ok(())
}

pub(crate) fn format_profile_output_mode(mode: &workflow::OutputMode) -> &'static str {
    match mode {
        workflow::OutputMode::Local => "Local",
        workflow::OutputMode::Remote => "Remote",
        workflow::OutputMode::Pr => "Pr",
        workflow::OutputMode::RemoteMain => "RemoteMain (merged)",
    }
}

pub(crate) fn format_profile_gate_mode(mode: &workflow::GateMode) -> &'static str {
    match mode {
        workflow::GateMode::Required => "Required",
        workflow::GateMode::Optional => "Optional",
        workflow::GateMode::Skipped => "Skipped",
    }
}

pub(crate) fn format_profile_fields(
    fields: &[ProfileField],
    palette: &ProfilePalette,
) -> Vec<String> {
    if fields.is_empty() {
        return Vec::new();
    }
    let label_width = fields
        .iter()
        .map(|field| field.label.len() + 1)
        .max()
        .unwrap_or(0);
    fields
        .iter()
        .map(|field| {
            let label = format!("{}:", field.label);
            let label_text = format!("{label:>label_width$}");
            format!("{}  {}", palette.label(&label_text), field.value)
        })
        .collect()
}

pub(crate) struct ProfileField {
    pub label: &'static str,
    pub value: String,
}

impl ProfileField {
    pub fn new(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
        }
    }
}

pub(crate) struct ProfilePalette {
    pub enabled: bool,
}

impl ProfilePalette {
    pub fn auto() -> Self {
        let enabled = std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        Self { enabled }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn heading(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    pub fn label(&self, text: &str) -> String {
        self.paint("36", text)
    }

    pub fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }
}

pub(crate) fn resolve_profile_state_selection(
    profile: &workflow::ProfileDefinition,
    requested_state: Option<&str>,
    current_state: &str,
) -> Result<String, app::AppError> {
    let interactive = std::io::stdin().is_terminal();

    if let Some(raw_state) = requested_state {
        let state = normalize_cli_state(raw_state)?;
        if profile.require_state(&state).is_ok() {
            return Ok(state);
        }
        if !interactive {
            return Err(app::AppError::InvalidArgument(format!(
                "state '{}' is not valid for profile '{}'; valid states: {}",
                state,
                profile.id,
                profile.states.join(", ")
            )));
        }
        return prompt_for_profile_state(profile, current_state);
    }

    if !interactive {
        return Err(app::AppError::InvalidArgument(
            "--state is required in non-interactive mode".to_string(),
        ));
    }
    prompt_for_profile_state(profile, current_state)
}

fn prompt_for_profile_state(
    profile: &workflow::ProfileDefinition,
    current_state: &str,
) -> Result<String, app::AppError> {
    use std::io::{self, Write};

    if profile.states.is_empty() {
        return Err(app::AppError::InvalidArgument(format!(
            "profile '{}' has no valid states",
            profile.id
        )));
    }

    println!(
        "choose state for profile '{}' (knot currently '{}'):",
        profile.id, current_state
    );
    for (index, state) in profile.states.iter().enumerate() {
        println!("  {}. {}", index + 1, state);
    }

    let fallback_index = profile
        .states
        .iter()
        .position(|state| state == current_state)
        .or_else(|| {
            profile
                .states
                .iter()
                .position(|state| state == &profile.initial_state)
        })
        .unwrap_or(0);
    println!("press Enter to choose {}", profile.states[fallback_index]);

    let mut input = String::new();
    loop {
        print!("state [1-{}]: ", profile.states.len());
        io::stdout().flush()?;
        input.clear();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(profile.states[fallback_index].clone());
        }
        if let Ok(index) = trimmed.parse::<usize>() {
            if (1..=profile.states.len()).contains(&index) {
                return Ok(profile.states[index - 1].clone());
            }
        }
        println!("enter a number between 1 and {}", profile.states.len());
    }
}

fn normalize_cli_state(raw: &str) -> Result<String, app::AppError> {
    use std::str::FromStr;

    let parsed = crate::domain::state::KnotState::from_str(raw)?;
    Ok(parsed.as_str().to_string())
}

#[cfg(test)]
#[path = "profile_commands_tests.rs"]
mod tests;

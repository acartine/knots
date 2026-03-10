use std::path::PathBuf;
use std::str::FromStr;
use std::{io, io::BufRead, io::IsTerminal, io::Write};

use crate::app::{App, AppError, StateActorMetadata, UpdateKnotPatch};
use crate::cli::{Cli, Commands, EdgeSubcommands, StepSubcommands};
use crate::dispatch::{knot_ref, resolve_next_state};
use crate::domain::invariant::parse_invariant_spec;
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntryInput;
use crate::domain::state::KnotState;
use crate::domain::step_history::StepActorInfo;
use crate::poll_claim;
use crate::ui;
use crate::write_queue::{
    self, ClaimOperation, EdgeOperation, NewOperation, NextOperation, PollClaimOperation,
    QueuedWriteRequest, QueuedWriteResponse, QuickNewOperation, StateOperation,
    StepAnnotateOperation, UpdateOperation, WriteOperation,
};

pub fn maybe_run_queued_command(cli: &Cli) -> Result<Option<String>, AppError> {
    let Some(operation) = operation_from_command(&cli.command) else {
        return Ok(None);
    };

    let response =
        write_queue::enqueue_and_wait(&cli.repo_root, &cli.db, operation, execute_queued_request)
            .map_err(|err| AppError::InvalidArgument(format!("write queue error: {}", err)))?;

    if response.success {
        Ok(Some(response.output))
    } else {
        Err(AppError::InvalidArgument(
            response
                .error
                .unwrap_or_else(|| "queued write failed".to_string()),
        ))
    }
}

fn operation_from_command(command: &Commands) -> Option<WriteOperation> {
    match command {
        Commands::New(args) => Some(WriteOperation::New(NewOperation {
            title: args.title.clone(),
            description: args.desc.clone(),
            state: args.state.clone(),
            profile: args.profile.clone(),
            fast: args.fast,
        })),
        Commands::Q(args) => Some(WriteOperation::QuickNew(QuickNewOperation {
            title: args.title.clone(),
            description: args.desc.clone(),
            state: args.state.clone(),
        })),
        Commands::State(args) => Some(WriteOperation::State(StateOperation {
            id: args.id.clone(),
            state: args.state.clone(),
            force: args.force,
            approve_terminal_cascade: args.cascade_terminal_descendants,
            if_match: args.if_match.clone(),
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        })),
        Commands::Update(args) => Some(WriteOperation::Update(UpdateOperation {
            id: args.id.clone(),
            title: args.title.clone(),
            description: args.description.clone(),
            priority: args.priority,
            status: args.status.clone(),
            knot_type: args.knot_type.clone(),
            add_tags: args.add_tags.clone(),
            remove_tags: args.remove_tags.clone(),
            add_note: args.add_note.clone(),
            note_username: args.note_username.clone(),
            note_datetime: args.note_datetime.clone(),
            note_agentname: args.note_agentname.clone(),
            note_model: args.note_model.clone(),
            note_version: args.note_version.clone(),
            add_handoff_capsule: args.add_handoff_capsule.clone(),
            handoff_username: args.handoff_username.clone(),
            handoff_datetime: args.handoff_datetime.clone(),
            handoff_agentname: args.handoff_agentname.clone(),
            handoff_model: args.handoff_model.clone(),
            handoff_version: args.handoff_version.clone(),
            add_invariants: args.add_invariants.clone(),
            remove_invariants: args.remove_invariants.clone(),
            clear_invariants: args.clear_invariants,
            if_match: args.if_match.clone(),
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
            force: args.force,
            approve_terminal_cascade: args.cascade_terminal_descendants,
        })),
        Commands::Next(args) => Some(WriteOperation::Next(NextOperation {
            id: args.id.clone(),
            expected_state: args
                .expected_state
                .clone()
                .or_else(|| args.current_state.clone()),
            json: args.json,
            approve_terminal_cascade: args.cascade_terminal_descendants,
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        })),
        Commands::Claim(args) if !args.peek => Some(WriteOperation::Claim(ClaimOperation {
            id: args.id.clone(),
            json: args.json,
            verbose: args.verbose,
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        })),
        Commands::Poll(args) if args.claim => Some(WriteOperation::PollClaim(PollClaimOperation {
            stage: args.stage.clone(),
            owner: args.owner.clone(),
            json: args.json,
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        })),
        Commands::Edge(args) => match &args.command {
            EdgeSubcommands::Add(edge) => Some(WriteOperation::EdgeAdd(EdgeOperation {
                src: edge.src.clone(),
                kind: edge.kind.clone(),
                dst: edge.dst.clone(),
            })),
            EdgeSubcommands::Remove(edge) => Some(WriteOperation::EdgeRemove(EdgeOperation {
                src: edge.src.clone(),
                kind: edge.kind.clone(),
                dst: edge.dst.clone(),
            })),
            EdgeSubcommands::List(_) => None,
        },
        Commands::Step(args) => match &args.command {
            StepSubcommands::Annotate(a) => {
                Some(WriteOperation::StepAnnotate(StepAnnotateOperation {
                    id: a.id.clone(),
                    actor_kind: a.actor_kind.clone(),
                    agent_name: a.agent_name.clone(),
                    agent_model: a.agent_model.clone(),
                    agent_version: a.agent_version.clone(),
                    json: a.json,
                }))
            }
        },
        _ => None,
    }
}

fn execute_queued_request(request: &QueuedWriteRequest) -> QueuedWriteResponse {
    let repo_root = PathBuf::from(&request.repo_root);
    let app = match App::open(&request.db_path, repo_root) {
        Ok(app) => app,
        Err(err) => return QueuedWriteResponse::failure(err.to_string()),
    };
    match execute_operation(&app, &request.operation) {
        Ok(output) => QueuedWriteResponse::success(output),
        Err(err) => QueuedWriteResponse::failure(err.to_string()),
    }
}

fn execute_operation(app: &App, operation: &WriteOperation) -> Result<String, AppError> {
    match operation {
        WriteOperation::New(args) => {
            let profile_override = if args.fast {
                Some(app.default_quick_profile_id()?)
            } else {
                None
            };
            let profile = profile_override.as_deref().or(args.profile.as_deref());
            let knot = app.create_knot(
                &args.title,
                args.description.as_deref(),
                args.state.as_deref(),
                profile,
            )?;
            let palette = ui::Palette::auto();
            Ok(format!(
                "created {} {} {}\n",
                palette.id(&knot_ref(&knot)),
                palette.state(&knot.state),
                knot.title
            ))
        }
        WriteOperation::QuickNew(args) => {
            let quick_profile = app.default_quick_profile_id()?;
            let knot = app.create_knot(
                &args.title,
                args.description.as_deref(),
                args.state.as_deref(),
                Some(&quick_profile),
            )?;
            let palette = ui::Palette::auto();
            Ok(format!(
                "created {} {} {}\n",
                palette.id(&knot_ref(&knot)),
                palette.state(&knot.state),
                knot.title
            ))
        }
        WriteOperation::State(args) => {
            let knot = execute_with_terminal_cascade_prompt(
                args.approve_terminal_cascade,
                |approve_terminal_cascade| {
                    app.set_state_with_actor_and_options(
                        &args.id,
                        &args.state,
                        args.force,
                        args.if_match.as_deref(),
                        StateActorMetadata {
                            actor_kind: args.actor_kind.clone(),
                            agent_name: args.agent_name.clone(),
                            agent_model: args.agent_model.clone(),
                            agent_version: args.agent_version.clone(),
                        },
                        approve_terminal_cascade,
                    )
                },
            )?;
            let palette = ui::Palette::auto();
            Ok(format!(
                "updated {} -> {}\n",
                palette.id(&knot_ref(&knot)),
                palette.state(&knot.state)
            ))
        }
        WriteOperation::Update(args) => {
            let add_note = args.add_note.clone().map(|content| MetadataEntryInput {
                content,
                username: args.note_username.clone(),
                datetime: args.note_datetime.clone(),
                agentname: args.note_agentname.clone(),
                model: args.note_model.clone(),
                version: args.note_version.clone(),
            });
            let add_handoff_capsule =
                args.add_handoff_capsule
                    .clone()
                    .map(|content| MetadataEntryInput {
                        content,
                        username: args.handoff_username.clone(),
                        datetime: args.handoff_datetime.clone(),
                        agentname: args.handoff_agentname.clone(),
                        model: args.handoff_model.clone(),
                        version: args.handoff_version.clone(),
                    });
            let patch = UpdateKnotPatch {
                title: args.title.clone(),
                description: args.description.clone(),
                priority: args.priority,
                status: args.status.clone(),
                knot_type: args
                    .knot_type
                    .as_deref()
                    .map(|raw| raw.parse::<KnotType>().unwrap_or_default()),
                add_tags: args.add_tags.clone(),
                remove_tags: args.remove_tags.clone(),
                add_invariants: args
                    .add_invariants
                    .iter()
                    .map(|raw| {
                        parse_invariant_spec(raw)
                            .map_err(|err| AppError::InvalidArgument(err.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                remove_invariants: args
                    .remove_invariants
                    .iter()
                    .map(|raw| {
                        parse_invariant_spec(raw)
                            .map_err(|err| AppError::InvalidArgument(err.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                clear_invariants: args.clear_invariants,
                add_note,
                add_handoff_capsule,
                expected_profile_etag: args.if_match.clone(),
                force: args.force,
                state_actor: StateActorMetadata {
                    actor_kind: args.actor_kind.clone(),
                    agent_name: args.agent_name.clone(),
                    agent_model: args.agent_model.clone(),
                    agent_version: args.agent_version.clone(),
                },
            };
            let knot = execute_with_terminal_cascade_prompt(
                args.approve_terminal_cascade,
                |approve_terminal_cascade| {
                    app.update_knot_with_options(&args.id, patch.clone(), approve_terminal_cascade)
                },
            )?;
            let palette = ui::Palette::auto();
            Ok(format!(
                "updated {} {} {}\n",
                palette.id(&knot_ref(&knot)),
                palette.state(&knot.state),
                knot.title
            ))
        }
        WriteOperation::Next(args) => {
            let knot = app
                .show_knot(&args.id)?
                .ok_or_else(|| AppError::NotFound(args.id.clone()))?;
            if let Some(expected_state_raw) = args.expected_state.as_deref() {
                let expected_state = normalize_expected_state(expected_state_raw);
                if knot.state != expected_state {
                    return Err(AppError::InvalidArgument(format!(
                        "expected state '{expected_state}' but knot is currently '{}'",
                        knot.state
                    )));
                }
            }
            let (knot, next, owner_kind) = resolve_next_state(app, &knot.id)?;
            let previous_state = knot.state.clone();
            let updated = execute_with_terminal_cascade_prompt(
                args.approve_terminal_cascade,
                |approve_terminal_cascade| {
                    app.set_state_with_actor_and_options(
                        &knot.id,
                        &next,
                        false,
                        None,
                        StateActorMetadata {
                            actor_kind: args.actor_kind.clone(),
                            agent_name: args.agent_name.clone(),
                            agent_model: args.agent_model.clone(),
                            agent_version: args.agent_version.clone(),
                        },
                        approve_terminal_cascade,
                    )
                },
            )?;
            Ok(format_next_output(
                &updated,
                &previous_state,
                owner_kind,
                args.json,
            ))
        }
        WriteOperation::Claim(args) => {
            let actor = StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: args.agent_name.clone(),
                agent_model: args.agent_model.clone(),
                agent_version: args.agent_version.clone(),
            };
            let claimed = poll_claim::claim_knot(app, &args.id, actor)?;
            if args.json {
                let value = poll_claim::render_json_verbose(&claimed, args.verbose);
                Ok(format_json(&value))
            } else {
                Ok(poll_claim::render_text_verbose(&claimed, args.verbose))
            }
        }
        WriteOperation::PollClaim(args) => {
            let polled = poll_claim::poll_queue(app, args.stage.as_deref(), args.owner.as_deref())?;
            let Some(polled) = polled else {
                return Err(AppError::InvalidArgument(
                    "no claimable knots found".to_string(),
                ));
            };
            let actor = StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: args.agent_name.clone(),
                agent_model: args.agent_model.clone(),
                agent_version: args.agent_version.clone(),
            };
            let claimed = poll_claim::claim_knot(app, &polled.knot.id, actor)?;
            if args.json {
                let value = poll_claim::render_json(&claimed);
                Ok(format_json(&value))
            } else {
                Ok(poll_claim::render_text(&claimed))
            }
        }
        WriteOperation::EdgeAdd(args) => {
            let edge = app.add_edge(&args.src, &args.kind, &args.dst)?;
            Ok(format!(
                "edge added: {} -[{}]-> {}\n",
                edge.src, edge.kind, edge.dst
            ))
        }
        WriteOperation::EdgeRemove(args) => {
            let edge = app.remove_edge(&args.src, &args.kind, &args.dst)?;
            Ok(format!(
                "edge removed: {} -[{}]-> {}\n",
                edge.src, edge.kind, edge.dst
            ))
        }
        WriteOperation::StepAnnotate(args) => {
            let actor = StepActorInfo {
                actor_kind: args.actor_kind.clone(),
                agent_name: args.agent_name.clone(),
                agent_model: args.agent_model.clone(),
                agent_version: args.agent_version.clone(),
                ..Default::default()
            };
            let knot = app.step_annotate(&args.id, &actor)?;
            if args.json {
                let result = serde_json::json!({
                    "id": &knot.id,
                    "state": &knot.state,
                    "step_history": &knot.step_history,
                });
                Ok(format_json(&result))
            } else {
                let palette = ui::Palette::auto();
                Ok(format!("step annotated {}\n", palette.id(&knot_ref(&knot))))
            }
        }
    }
}

fn execute_with_terminal_cascade_prompt<T, F>(
    preapproved: bool,
    mut action: F,
) -> Result<T, AppError>
where
    F: FnMut(bool) -> Result<T, AppError>,
{
    let mut approved = preapproved;
    loop {
        match action(approved) {
            Ok(value) => return Ok(value),
            Err(AppError::TerminalCascadeApprovalRequired {
                knot_id,
                target_state,
                descendants,
            }) if !approved => {
                if !io::stdin().is_terminal() {
                    return Err(AppError::TerminalCascadeApprovalRequired {
                        knot_id,
                        target_state,
                        descendants,
                    });
                }
                if prompt_for_terminal_cascade_approval(&knot_id, &target_state, &descendants)? {
                    approved = true;
                    continue;
                }
                return Err(AppError::InvalidArgument(
                    "terminal cascade cancelled; no changes written".to_string(),
                ));
            }
            Err(err) => return Err(err),
        }
    }
}

fn prompt_for_terminal_cascade_approval(
    knot_id: &str,
    target_state: &str,
    descendants: &[crate::state_hierarchy::HierarchyKnot],
) -> Result<bool, AppError> {
    let mut stderr = io::stderr();
    let mut stdin = io::stdin().lock();
    terminal_cascade_prompt(&mut stderr, &mut stdin, knot_id, target_state, descendants)
}

fn terminal_cascade_prompt<W: Write, R: BufRead>(
    writer: &mut W,
    reader: &mut R,
    knot_id: &str,
    target_state: &str,
    descendants: &[crate::state_hierarchy::HierarchyKnot],
) -> Result<bool, AppError> {
    writeln!(
        writer,
        "moving '{}' to '{}' will also move descendant knots to that terminal state:",
        knot_id, target_state
    )?;
    writeln!(
        writer,
        "  {}",
        crate::state_hierarchy::format_hierarchy_knots(descendants)
    )?;
    write!(writer, "continue? [y/N]: ")?;
    writer.flush()?;

    let mut input = String::new();
    reader.read_line(&mut input)?;
    Ok(is_terminal_cascade_approval(&input))
}

fn is_terminal_cascade_approval(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn format_json(value: &serde_json::Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("queued json serialization should succeed")
    )
}

fn normalize_expected_state(raw: &str) -> String {
    let trimmed = raw.trim();
    KnotState::from_str(trimmed)
        .map(|state| state.as_str().to_string())
        .unwrap_or_else(|_| trimmed.to_ascii_lowercase().replace('-', "_"))
}

fn format_next_output(
    knot: &crate::app::KnotView,
    previous_state: &str,
    owner_kind: Option<&str>,
    json: bool,
) -> String {
    if json {
        let result = serde_json::json!({
            "id": &knot.id,
            "previous_state": previous_state,
            "state": &knot.state,
            "owner_kind": owner_kind,
        });
        return format_json(&result);
    }
    let palette = ui::Palette::auto();
    let owner_suffix = owner_kind
        .map(|kind| format!(" (owner: {kind})"))
        .unwrap_or_default();
    format!(
        "updated {} -> {}{}\n",
        palette.id(&knot_ref(knot)),
        palette.state(&knot.state),
        owner_suffix,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use uuid::Uuid;

    fn unique_workspace(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
        root
    }
    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git").arg("-C").arg(root).args(args).output();
        let output = output.expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn setup_repo(root: &Path) {
        run_git(root, &["init"]);
        run_git(root, &["config", "user.email", "knots@example.com"]);
        run_git(root, &["config", "user.name", "Knots Test"]);
        std::fs::write(root.join("README.md"), "# knots\n").expect("readme should be writable");
        run_git(root, &["add", "README.md"]);
        run_git(root, &["commit", "-m", "init"]);
        run_git(root, &["branch", "-M", "main"]);
    }
    #[test]
    fn execute_queued_request_returns_failure_when_app_open_fails() {
        let root = unique_workspace("knots-write-dispatch-open-fail");
        setup_repo(&root);
        let bad_db_dir = root.join("db-directory");
        std::fs::create_dir_all(&bad_db_dir).expect("bad db directory should be creatable");
        let request = QueuedWriteRequest {
            request_id: "req-open-fail".to_string(),
            repo_root: root.to_string_lossy().into_owned(),
            db_path: bad_db_dir.to_string_lossy().into_owned(),
            response_path: String::new(),
            operation: WriteOperation::New(NewOperation {
                title: "queued".to_string(),
                description: None,
                state: None,
                profile: None,
                fast: false,
            }),
        };
        let response = execute_queued_request(&request);
        assert!(!response.success);
        assert!(response
            .error
            .expect("error should be present")
            .contains("database"));
    }
    #[test]
    fn execute_operation_poll_claim_covers_empty_and_json_paths() {
        let root = unique_workspace("knots-write-dispatch-poll-claim");
        setup_repo(&root);
        let db_path = root.join(".knots/cache/state.sqlite");
        let app = App::open(
            db_path.to_str().expect("db path should be utf8"),
            root.clone(),
        )
        .expect("app should open");
        let empty_poll = WriteOperation::PollClaim(PollClaimOperation {
            stage: None,
            owner: None,
            json: false,
            agent_name: None,
            agent_model: None,
            agent_version: None,
        });
        let err = execute_operation(&app, &empty_poll).expect_err("empty poll should fail");
        match err {
            AppError::InvalidArgument(message) => {
                assert!(message.contains("no claimable knots found"))
            }
            other => panic!("unexpected poll error: {other}"),
        }
        app.create_knot("Claim me", None, None, None)
            .expect("knot should be created");
        let json_poll = WriteOperation::PollClaim(PollClaimOperation {
            stage: None,
            owner: None,
            json: true,
            agent_name: Some("agent".to_string()),
            agent_model: Some("model".to_string()),
            agent_version: Some("1.0".to_string()),
        });
        let output = execute_operation(&app, &json_poll).expect("poll claim json should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("json poll output should parse");
        assert!(parsed
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_some());
    }

    #[test]
    fn execute_operation_next_rejects_mismatched_expected_state() {
        let root = unique_workspace("knots-write-dispatch-next-mismatch");
        setup_repo(&root);
        let db_path = root.join(".knots/cache/state.sqlite");
        let app = App::open(
            db_path.to_str().expect("db path should be utf8"),
            root.clone(),
        )
        .expect("app should open");
        let created = app
            .create_knot(
                "Mismatch expected state",
                None,
                Some("ready_for_implementation"),
                None,
            )
            .expect("knot should be created");

        let operation = WriteOperation::Next(NextOperation {
            id: created.id.clone(),
            expected_state: Some("planning".to_string()),
            json: false,
            approve_terminal_cascade: false,
            actor_kind: None,
            agent_name: None,
            agent_model: None,
            agent_version: None,
        });

        let err = execute_operation(&app, &operation).expect_err("state mismatch should fail");
        match err {
            AppError::InvalidArgument(message) => {
                assert!(message.contains("expected state 'planning'"));
                assert!(message.contains("ready_for_implementation"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn execute_with_terminal_cascade_prompt_returns_error_in_noninteractive_mode() {
        let descendants = vec![crate::state_hierarchy::HierarchyKnot {
            id: "knots-child".to_string(),
            state: "planning".to_string(),
            deferred_from_state: None,
        }];
        let err = execute_with_terminal_cascade_prompt(false, |_| -> Result<(), AppError> {
            Err(AppError::TerminalCascadeApprovalRequired {
                knot_id: "knots-parent".to_string(),
                target_state: "abandoned".to_string(),
                descendants: descendants.clone(),
            })
        })
        .expect_err("non-interactive execution should return approval error");
        match err {
            AppError::TerminalCascadeApprovalRequired {
                knot_id,
                target_state,
                descendants,
            } => {
                assert_eq!(knot_id, "knots-parent");
                assert_eq!(target_state, "abandoned");
                assert_eq!(descendants.len(), 1);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn terminal_cascade_prompt_accepts_yes_and_renders_descendants() {
        let descendants = vec![crate::state_hierarchy::HierarchyKnot {
            id: "knots-child".to_string(),
            state: "deferred".to_string(),
            deferred_from_state: Some("implementation".to_string()),
        }];
        let mut output = Vec::new();
        let mut input = Cursor::new("yes\n");
        let approved = terminal_cascade_prompt(
            &mut output,
            &mut input,
            "knots-parent",
            "shipped",
            &descendants,
        )
        .expect("prompt should succeed");
        assert!(approved);

        let rendered = String::from_utf8(output).expect("output should be utf8");
        assert!(rendered.contains("knots-parent"));
        assert!(rendered.contains("knots-child [deferred from implementation]"));
        assert!(rendered.contains("continue? [y/N]:"));
    }

    #[test]
    fn terminal_cascade_prompt_rejects_non_yes_answers() {
        let descendants = vec![crate::state_hierarchy::HierarchyKnot {
            id: "knots-child".to_string(),
            state: "planning".to_string(),
            deferred_from_state: None,
        }];
        let mut output = Vec::new();
        let mut input = Cursor::new("no\n");
        let approved = terminal_cascade_prompt(
            &mut output,
            &mut input,
            "knots-parent",
            "abandoned",
            &descendants,
        )
        .expect("prompt should succeed");
        assert!(!approved);
    }

    #[test]
    fn terminal_cascade_input_normalizes_common_yes_values() {
        assert!(is_terminal_cascade_approval("y"));
        assert!(is_terminal_cascade_approval(" YES "));
        assert!(!is_terminal_cascade_approval("n"));
        assert!(!is_terminal_cascade_approval(""));
    }

    #[test]
    fn operation_from_command_threads_terminal_cascade_flags() {
        let state_cli = crate::cli::Cli::parse_from([
            "kno",
            "state",
            "knots-1",
            "abandoned",
            "--cascade-terminal-descendants",
        ]);
        let update_cli = crate::cli::Cli::parse_from([
            "kno",
            "update",
            "knots-1",
            "--status",
            "abandoned",
            "--cascade-terminal-descendants",
        ]);
        let next_cli = crate::cli::Cli::parse_from([
            "kno",
            "next",
            "knots-1",
            "--cascade-terminal-descendants",
        ]);

        match operation_from_command(&state_cli.command).expect("state should queue") {
            WriteOperation::State(operation) => assert!(operation.approve_terminal_cascade),
            other => panic!("unexpected state operation: {other:?}"),
        }
        match operation_from_command(&update_cli.command).expect("update should queue") {
            WriteOperation::Update(operation) => assert!(operation.approve_terminal_cascade),
            other => panic!("unexpected update operation: {other:?}"),
        }
        match operation_from_command(&next_cli.command).expect("next should queue") {
            WriteOperation::Next(operation) => assert!(operation.approve_terminal_cascade),
            other => panic!("unexpected next operation: {other:?}"),
        }
    }

    #[test]
    fn operation_from_command_maps_step_annotate() {
        let cli = crate::cli::Cli::parse_from([
            "kno",
            "step",
            "annotate",
            "knots-1",
            "--actor-kind",
            "agent",
            "--agent-name",
            "codex",
            "--json",
        ]);

        match operation_from_command(&cli.command).expect("step annotate should queue") {
            WriteOperation::StepAnnotate(operation) => {
                assert_eq!(operation.id, "knots-1");
                assert_eq!(operation.actor_kind.as_deref(), Some("agent"));
                assert_eq!(operation.agent_name.as_deref(), Some("codex"));
                assert!(operation.json);
            }
            other => panic!("unexpected step annotate operation: {other:?}"),
        }
    }

    #[test]
    fn maybe_run_queued_command_returns_none_for_read_only_commands() {
        let cli = crate::cli::Cli::parse_from(["kno", "show", "knots-1"]);
        let result = maybe_run_queued_command(&cli).expect("read-only commands should skip queue");
        assert!(result.is_none());
    }

    #[test]
    fn normalize_expected_state_and_format_next_output_cover_helpers() {
        assert_eq!(
            normalize_expected_state("implemented"),
            "ready_for_implementation_review"
        );
        assert_eq!(normalize_expected_state("State-Name"), "state_name");

        let knot = crate::app::KnotView {
            id: "knots-1".to_string(),
            alias: Some("root.1".to_string()),
            title: "Example".to_string(),
            state: "planning".to_string(),
            updated_at: "2026-03-10T00:00:00Z".to_string(),
            body: None,
            description: None,
            priority: None,
            knot_type: KnotType::Work,
            tags: vec![],
            notes: vec![],
            handoff_capsules: vec![],
            invariants: vec![],
            step_history: vec![],
            profile_id: "default".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            created_at: None,
            edges: vec![],
        };

        let text = format_next_output(&knot, "idea", Some("agent"), false);
        assert!(text.contains("root.1"));
        assert!(text.contains("owner: agent"));

        let json = format_next_output(&knot, "idea", None, true);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("json next output should parse");
        assert_eq!(parsed["previous_state"], "idea");
        assert_eq!(parsed["state"], "planning");
    }

    #[test]
    fn execute_operation_step_annotate_covers_text_and_json_paths() {
        let root = unique_workspace("knots-write-dispatch-step-annotate");
        setup_repo(&root);
        let db_path = root.join(".knots/cache/state.sqlite");
        let app = App::open(
            db_path.to_str().expect("db path should be utf8"),
            root.clone(),
        )
        .expect("app should open");
        let created = app
            .create_knot("Step annotate", None, Some("ready_for_planning"), None)
            .expect("knot should be created");
        let claimed = app
            .set_state_with_actor(
                &created.id,
                "planning",
                false,
                created.profile_etag.as_deref(),
                StateActorMetadata {
                    actor_kind: Some("agent".to_string()),
                    agent_name: Some("claimer".to_string()),
                    agent_model: Some("model".to_string()),
                    agent_version: Some("1.0".to_string()),
                },
            )
            .expect("claim should start a step");

        let text_output = execute_operation(
            &app,
            &WriteOperation::StepAnnotate(StepAnnotateOperation {
                id: claimed.id.clone(),
                actor_kind: Some("agent".to_string()),
                agent_name: Some("annotator".to_string()),
                agent_model: Some("model".to_string()),
                agent_version: Some("2.0".to_string()),
                json: false,
            }),
        )
        .expect("text step annotate should succeed");
        assert!(text_output.contains("step annotated"));

        let json_output = execute_operation(
            &app,
            &WriteOperation::StepAnnotate(StepAnnotateOperation {
                id: claimed.id.clone(),
                actor_kind: Some("agent".to_string()),
                agent_name: Some("annotator-json".to_string()),
                agent_model: Some("model".to_string()),
                agent_version: Some("3.0".to_string()),
                json: true,
            }),
        )
        .expect("json step annotate should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&json_output).expect("json step annotate output should parse");
        assert_eq!(parsed["id"], claimed.id);
        assert!(parsed["step_history"].as_array().is_some());
    }
}

use std::path::PathBuf;
use std::str::FromStr;

use crate::app::{App, AppError, StateActorMetadata, UpdateKnotPatch};
use crate::cli::{Cli, Commands, EdgeSubcommands};
use crate::dispatch::{knot_ref, owner_kind_for_state, resolve_next_state};
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntryInput;
use crate::domain::state::KnotState;
use crate::poll_claim;
use crate::ui;
use crate::write_queue::{
    self, ClaimOperation, EdgeOperation, NewOperation, NextOperation, PollClaimOperation,
    QueuedWriteRequest, QueuedWriteResponse, QuickNewOperation, StateOperation, UpdateOperation,
    WriteOperation,
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
            if_match: args.if_match.clone(),
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
            force: args.force,
        })),
        Commands::Next(args) => Some(WriteOperation::Next(NextOperation {
            id: args.id.clone(),
            current_state: args.current_state.clone(),
            json: args.json,
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        })),
        Commands::Claim(args) if !args.peek => Some(WriteOperation::Claim(ClaimOperation {
            id: args.id.clone(),
            json: args.json,
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
            let knot = app.set_state_with_actor(
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
            let knot = app.update_knot(&args.id, patch)?;
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
            let expected_state = normalize_expected_state(&args.current_state);
            if knot.state != expected_state {
                let owner_kind = owner_kind_for_state(&knot.profile_id, &knot.state)?;
                return Ok(format_next_output(
                    &knot,
                    &expected_state,
                    owner_kind,
                    args.json,
                ));
            }
            let (knot, next, owner_kind) = resolve_next_state(app, &knot.id)?;
            let previous_state = knot.state.clone();
            let updated = app.set_state_with_actor(
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
                let value = poll_claim::render_json(&claimed);
                Ok(format_json(&value))
            } else {
                Ok(poll_claim::render_text(&claimed))
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
    }
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

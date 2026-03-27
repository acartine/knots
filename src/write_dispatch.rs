use std::path::PathBuf;
use std::str::FromStr;
use std::{io, io::BufRead, io::IsTerminal, io::Write};

use crate::app::{
    App, AppError, CreateKnotOptions, GateDecision, StateActorMetadata, UpdateKnotPatch,
};
use crate::cli::{
    Cli, Commands, EdgeSubcommands, GateSubcommands, LeaseSubcommands, StepSubcommands,
};
use crate::dispatch::{knot_ref, resolve_next_state};
use crate::domain::gate::{parse_failure_mode_spec, GateData, GateOwnerKind};
use crate::domain::invariant::parse_invariant_spec;
use crate::domain::knot_type::KnotType;
use crate::domain::metadata::MetadataEntryInput;
use crate::domain::state::KnotState;
use crate::domain::step_history::StepActorInfo;
use crate::poll_claim;
use crate::rollback::resolve_rollback_state;
use crate::ui;
use crate::write_queue::{
    self, ClaimOperation, EdgeOperation, GateEvaluateOperation, LeaseCreateOperation,
    LeaseTerminateOperation, NewOperation, NextOperation, PollClaimOperation, QueuedWriteRequest,
    QueuedWriteResponse, QuickNewOperation, RollbackOperation, StateOperation,
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
            acceptance: args.acceptance.clone(),
            state: args.state.clone(),
            profile: args.profile.clone(),
            fast: args.fast,
            knot_type: args.knot_type.clone(),
            gate_owner_kind: args.gate_owner_kind.clone(),
            gate_failure_modes: args.gate_failure_modes.clone(),
            lease_id: args.lease.clone(),
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
            acceptance: args.acceptance.clone(),
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
            gate_owner_kind: args.gate_owner_kind.clone(),
            gate_failure_modes: args.gate_failure_modes.clone(),
            clear_gate_failure_modes: args.clear_gate_failure_modes,
            if_match: args.if_match.clone(),
            actor_kind: args.actor_kind.clone(),
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
            force: args.force,
            approve_terminal_cascade: args.cascade_terminal_descendants,
            lease_id: args.lease.clone(),
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
            lease_id: args.lease.clone(),
        })),
        Commands::Rollback(args) => Some(WriteOperation::Rollback(RollbackOperation {
            id: args.id.clone(),
            dry_run: args.dry_run,
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
            lease_id: args.lease.clone(),
        })),
        Commands::Poll(args) if args.claim => Some(WriteOperation::PollClaim(PollClaimOperation {
            stage: args.stage.clone(),
            owner: args.owner.clone(),
            json: args.json,
            agent_name: args.agent_name.clone(),
            agent_model: args.agent_model.clone(),
            agent_version: args.agent_version.clone(),
        })),
        Commands::Gate(args) => match &args.command {
            GateSubcommands::Evaluate(gate) => {
                Some(WriteOperation::GateEvaluate(GateEvaluateOperation {
                    id: gate.id.clone(),
                    decision: gate.decision.clone(),
                    invariant: gate.invariant.clone(),
                    json: gate.json,
                    actor_kind: gate.actor_kind.clone(),
                    agent_name: gate.agent_name.clone(),
                    agent_model: gate.agent_model.clone(),
                    agent_version: gate.agent_version.clone(),
                }))
            }
        },
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
        Commands::Lease(args) => match &args.command {
            LeaseSubcommands::Create(create) => {
                Some(WriteOperation::LeaseCreate(LeaseCreateOperation {
                    nickname: create.nickname.clone(),
                    lease_type: create.lease_type.clone(),
                    provider: create.provider.clone(),
                    agent_type: create.agent_type.clone(),
                    agent_name: create.agent_name.clone(),
                    model: create.model.clone(),
                    model_version: create.model_version.clone(),
                    json: create.json,
                }))
            }
            LeaseSubcommands::Terminate(term) => {
                Some(WriteOperation::LeaseTerminate(LeaseTerminateOperation {
                    id: term.id.clone(),
                }))
            }
            _ => None, // Show and List are read operations
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
            let knot_type = parse_knot_type_arg(args.knot_type.as_deref())?;
            let gate_data = parse_gate_data_args(
                args.gate_owner_kind.as_deref(),
                &args.gate_failure_modes,
                knot_type,
            )?;
            let knot = app.create_knot_with_options(
                &args.title,
                args.description.as_deref(),
                args.state.as_deref(),
                profile,
                CreateKnotOptions {
                    acceptance: args.acceptance.clone(),
                    knot_type,
                    gate_data,
                    ..CreateKnotOptions::default()
                },
            )?;
            if let Some(lid) = &args.lease_id {
                crate::lease::bind_lease(app, &knot.id, lid)?;
            }
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
                        false,
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
            let lease_agent = resolve_lease_agent_info(app, &args.id);
            let add_note = args.add_note.clone().map(|content| {
                let lai = lease_agent.as_ref();
                MetadataEntryInput {
                    content,
                    username: args
                        .note_username
                        .clone()
                        .or_else(|| lai.map(|i| i.provider.clone())),
                    datetime: args.note_datetime.clone(),
                    agentname: args
                        .note_agentname
                        .clone()
                        .or_else(|| lai.map(|i| i.agent_name.clone())),
                    model: args
                        .note_model
                        .clone()
                        .or_else(|| lai.map(|i| i.model.clone())),
                    version: args
                        .note_version
                        .clone()
                        .or_else(|| lai.map(|i| i.model_version.clone())),
                }
            });
            let add_handoff_capsule = args.add_handoff_capsule.clone().map(|content| {
                let lai = lease_agent.as_ref();
                MetadataEntryInput {
                    content,
                    username: args
                        .handoff_username
                        .clone()
                        .or_else(|| lai.map(|i| i.provider.clone())),
                    datetime: args.handoff_datetime.clone(),
                    agentname: args
                        .handoff_agentname
                        .clone()
                        .or_else(|| lai.map(|i| i.agent_name.clone())),
                    model: args
                        .handoff_model
                        .clone()
                        .or_else(|| lai.map(|i| i.model.clone())),
                    version: args
                        .handoff_version
                        .clone()
                        .or_else(|| lai.map(|i| i.model_version.clone())),
                }
            });
            let patch = UpdateKnotPatch {
                title: args.title.clone(),
                description: args.description.clone(),
                acceptance: args.acceptance.clone(),
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
                gate_owner_kind: parse_gate_owner_kind_arg(args.gate_owner_kind.as_deref())?,
                gate_failure_modes: parse_gate_failure_modes_option(&args.gate_failure_modes)?,
                clear_gate_failure_modes: args.clear_gate_failure_modes,
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
            if let Some(lid) = &args.lease_id {
                crate::lease::bind_lease(app, &knot.id, lid)?;
            }
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
            // Lease ownership validation
            if let Some(ref provided_lease) = args.lease_id {
                match &knot.lease_id {
                    Some(knot_lease) if knot_lease == provided_lease => {
                        // Lease matches — proceed
                    }
                    Some(knot_lease) => {
                        return Err(AppError::InvalidArgument(format!(
                            "lease mismatch: knot has '{}', caller provided '{}'",
                            knot_lease, provided_lease
                        )));
                    }
                    None => {
                        return Err(AppError::InvalidArgument(format!(
                            "knot has no active lease but caller provided '{}'",
                            provided_lease
                        )));
                    }
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
                        false,
                    )
                },
            )?;
            // Terminate and unbind lease if present (silent)
            if updated.lease_id.is_some() {
                let _ = crate::lease::unbind_lease(app, &updated.id);
            }
            Ok(format_next_output(
                &updated,
                &previous_state,
                owner_kind,
                args.json,
            ))
        }
        WriteOperation::Rollback(args) => {
            let resolution = resolve_rollback_state(app, &args.id)?;
            if args.dry_run {
                return Ok(format_rollback_output(
                    &resolution.knot,
                    &resolution.target_state,
                    resolution.owner_kind,
                    &resolution.reason,
                    true,
                ));
            }
            let updated = app.set_state_with_actor_and_options(
                &resolution.knot.id,
                &resolution.target_state,
                resolution.requires_force,
                None,
                StateActorMetadata {
                    actor_kind: args.actor_kind.clone(),
                    agent_name: args.agent_name.clone(),
                    agent_model: args.agent_model.clone(),
                    agent_version: args.agent_version.clone(),
                },
                false,
                false,
            )?;
            Ok(format_rollback_output(
                &updated,
                &resolution.target_state,
                resolution.owner_kind,
                &resolution.reason,
                false,
            ))
        }
        WriteOperation::Claim(args) => {
            let actor = StateActorMetadata {
                actor_kind: Some("agent".to_string()),
                agent_name: args.agent_name.clone(),
                agent_model: args.agent_model.clone(),
                agent_version: args.agent_version.clone(),
            };
            let claimed = poll_claim::claim_knot(app, &args.id, actor, args.lease_id.as_deref())?;
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
            let claimed = poll_claim::claim_knot(app, &polled.knot.id, actor, None)?;
            if args.json {
                let value = poll_claim::render_json(&claimed);
                Ok(format_json(&value))
            } else {
                Ok(poll_claim::render_text(&claimed))
            }
        }
        WriteOperation::GateEvaluate(args) => {
            let result = app.evaluate_gate(
                &args.id,
                parse_gate_decision(&args.decision)?,
                args.invariant.as_deref(),
                StateActorMetadata {
                    actor_kind: args.actor_kind.clone(),
                    agent_name: args.agent_name.clone(),
                    agent_model: args.agent_model.clone(),
                    agent_version: args.agent_version.clone(),
                },
            )?;
            if args.json {
                Ok(format_json(
                    &serde_json::to_value(&result).expect("gate evaluation should serialize"),
                ))
            } else {
                let palette = ui::Palette::auto();
                let reopened = if result.reopened.is_empty() {
                    String::new()
                } else {
                    format!(" reopened={}", result.reopened.len())
                };
                Ok(format!(
                    "evaluated {} -> {} decision={}{}\n",
                    palette.id(&knot_ref(&result.gate)),
                    palette.state(&result.gate.state),
                    result.decision,
                    reopened
                ))
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
        WriteOperation::LeaseCreate(op) => execute_lease_create(app, op),
        WriteOperation::LeaseTerminate(op) => execute_lease_terminate(app, op),
    }
}

fn resolve_lease_agent_info(app: &App, knot_id: &str) -> Option<crate::domain::lease::AgentInfo> {
    let knot = app.show_knot(knot_id).ok()??;
    let lease_id = knot.lease_id.as_ref()?;
    let lease_knot = app.show_knot(lease_id).ok()??;
    lease_knot.lease.as_ref()?.agent_info.clone()
}

fn execute_lease_create(app: &App, op: &LeaseCreateOperation) -> Result<String, AppError> {
    use crate::domain::lease::{AgentInfo, LeaseType};
    let lease_type = match op.lease_type.as_str() {
        "manual" => LeaseType::Manual,
        _ => LeaseType::Agent,
    };
    let agent_info = if lease_type == LeaseType::Agent {
        Some(AgentInfo {
            agent_type: op.agent_type.clone().unwrap_or_default(),
            provider: op.provider.clone().unwrap_or_default(),
            agent_name: op.agent_name.clone().unwrap_or_default(),
            model: op.model.clone().unwrap_or_default(),
            model_version: op.model_version.clone().unwrap_or_default(),
        })
    } else {
        None
    };
    let view = crate::lease::create_lease(app, &op.nickname, lease_type, agent_info)?;
    if op.json {
        Ok(format_json(
            &serde_json::to_value(&view).expect("serialize"),
        ))
    } else {
        let palette = ui::Palette::auto();
        Ok(format!(
            "created lease {} {}\n",
            palette.id(&knot_ref(&view)),
            view.title,
        ))
    }
}

fn execute_lease_terminate(app: &App, op: &LeaseTerminateOperation) -> Result<String, AppError> {
    let view = crate::lease::terminate_lease(app, &op.id)?;
    // Best-effort: run any queued sync now that a lease has ended.
    let _ = app.trigger_queued_sync();
    let palette = ui::Palette::auto();
    Ok(format!(
        "terminated lease {} -> {}\n",
        palette.id(&knot_ref(&view)),
        palette.state(&view.state),
    ))
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

fn parse_gate_decision(raw: &str) -> Result<GateDecision, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "yes" | "pass" => Ok(GateDecision::Yes),
        "no" | "fail" => Ok(GateDecision::No),
        _ => Err(AppError::InvalidArgument(
            "--decision must be one of: yes, no".to_string(),
        )),
    }
}

fn parse_knot_type_arg(raw: Option<&str>) -> Result<KnotType, AppError> {
    raw.unwrap_or("work")
        .parse::<KnotType>()
        .map_err(|err| AppError::InvalidArgument(err.to_string()))
}

fn parse_gate_owner_kind_arg(raw: Option<&str>) -> Result<Option<GateOwnerKind>, AppError> {
    raw.map(|value| {
        value
            .parse::<GateOwnerKind>()
            .map_err(|err| AppError::InvalidArgument(err.to_string()))
    })
    .transpose()
}

fn parse_gate_failure_modes_option(
    raw_specs: &[String],
) -> Result<Option<std::collections::BTreeMap<String, Vec<String>>>, AppError> {
    if raw_specs.is_empty() {
        return Ok(None);
    }
    let mut failure_modes = std::collections::BTreeMap::new();
    for raw in raw_specs {
        let (invariant, targets) = parse_failure_mode_spec(raw)
            .map_err(|err| AppError::InvalidArgument(err.to_string()))?;
        failure_modes.insert(invariant, targets);
    }
    Ok(Some(failure_modes))
}

fn parse_gate_data_args(
    owner_kind: Option<&str>,
    raw_failure_modes: &[String],
    knot_type: KnotType,
) -> Result<GateData, AppError> {
    let owner_kind = parse_gate_owner_kind_arg(owner_kind)?;
    let failure_modes = parse_gate_failure_modes_option(raw_failure_modes)?.unwrap_or_default();
    if knot_type != KnotType::Gate && (owner_kind.is_some() || !failure_modes.is_empty()) {
        return Err(AppError::InvalidArgument(
            "gate owner/failure mode fields require knot type 'gate'".to_string(),
        ));
    }
    Ok(GateData {
        owner_kind: owner_kind.unwrap_or_default(),
        failure_modes,
    })
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

fn format_rollback_output(
    knot: &crate::app::KnotView,
    target_state: &str,
    owner_kind: Option<&str>,
    reason: &str,
    dry_run: bool,
) -> String {
    let palette = ui::Palette::auto();
    let owner_suffix = owner_kind
        .map(|kind| format!(" (owner: {kind})"))
        .unwrap_or_default();
    let verb = if dry_run {
        "would roll back"
    } else {
        "rolled back"
    };
    format!(
        "{verb} {} -> {}{} ({reason})\n",
        palette.id(&knot_ref(knot)),
        palette.state(target_state),
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
                acceptance: None,
                state: None,
                profile: None,
                fast: false,
                knot_type: None,
                gate_owner_kind: None,
                gate_failure_modes: vec![],
                lease_id: None,
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
            lease_id: None,
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
    fn operation_from_command_maps_rollback() {
        let cli = crate::cli::Cli::parse_from([
            "kno",
            "rb",
            "knots-1",
            "--dry-run",
            "--actor-kind",
            "agent",
        ]);

        match operation_from_command(&cli.command).expect("rollback should queue") {
            WriteOperation::Rollback(operation) => {
                assert_eq!(operation.id, "knots-1");
                assert!(operation.dry_run);
                assert_eq!(operation.actor_kind.as_deref(), Some("agent"));
            }
            other => panic!("unexpected rollback operation: {other:?}"),
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
    fn execute_operation_rollback_covers_dry_run_real_and_rejection_paths() {
        let root = unique_workspace("knots-write-dispatch-rollback");
        setup_repo(&root);
        let db_path = root.join(".knots/cache/state.sqlite");
        let app = App::open(
            db_path.to_str().expect("db path should be utf8"),
            root.clone(),
        )
        .expect("app should open");
        let created = app
            .create_knot("Rollback", None, Some("ready_for_implementation"), None)
            .expect("knot should be created");

        let queue_err = execute_operation(
            &app,
            &WriteOperation::Rollback(RollbackOperation {
                id: created.id.clone(),
                dry_run: false,
                actor_kind: None,
                agent_name: None,
                agent_model: None,
                agent_version: None,
            }),
        )
        .expect_err("queue-state rollback should fail");
        match queue_err {
            AppError::InvalidArgument(message) => assert!(message.contains("queue state")),
            other => panic!("unexpected rollback rejection: {other}"),
        }

        let implementation = app
            .set_state_with_actor(
                &created.id,
                "implementation",
                false,
                created.profile_etag.as_deref(),
                StateActorMetadata {
                    actor_kind: Some("agent".to_string()),
                    agent_name: Some("claimer".to_string()),
                    agent_model: Some("model".to_string()),
                    agent_version: Some("1.0".to_string()),
                },
            )
            .expect("implementation claim should succeed");
        let dry_run = execute_operation(
            &app,
            &WriteOperation::Rollback(RollbackOperation {
                id: implementation.id.clone(),
                dry_run: true,
                actor_kind: None,
                agent_name: None,
                agent_model: None,
                agent_version: None,
            }),
        )
        .expect("dry-run rollback should succeed");
        assert!(dry_run.contains("would roll back"));
        assert!(dry_run.contains("ready_for_implementation"));

        let after_dry_run = app
            .show_knot(&implementation.id)
            .expect("knot should load")
            .expect("knot should exist");
        assert_eq!(after_dry_run.state, "implementation");
        assert_eq!(
            after_dry_run.step_history.len(),
            implementation.step_history.len()
        );

        app.set_state_with_actor(
            &implementation.id,
            "ready_for_implementation_review",
            false,
            implementation.profile_etag.as_deref(),
            StateActorMetadata::default(),
        )
        .expect("queue review transition should succeed");
        let in_review = app
            .set_state_with_actor(
                &implementation.id,
                "implementation_review",
                false,
                None,
                StateActorMetadata {
                    actor_kind: Some("agent".to_string()),
                    agent_name: Some("reviewer".to_string()),
                    agent_model: Some("model".to_string()),
                    agent_version: Some("2.0".to_string()),
                },
            )
            .expect("review claim should succeed");

        let output = execute_operation(
            &app,
            &WriteOperation::Rollback(RollbackOperation {
                id: in_review.id.clone(),
                dry_run: false,
                actor_kind: Some("agent".to_string()),
                agent_name: Some("rollbacker".to_string()),
                agent_model: Some("model".to_string()),
                agent_version: Some("3.0".to_string()),
            }),
        )
        .expect("rollback should succeed");
        assert!(output.contains("rolled back"));
        assert!(output.contains("ready_for_implementation"));

        let after_rollback = app
            .show_knot(&in_review.id)
            .expect("knot should load")
            .expect("knot should exist");
        assert_eq!(after_rollback.state, "ready_for_implementation");
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
            acceptance: None,
            priority: None,
            knot_type: KnotType::Work,
            tags: vec![],
            notes: vec![],
            handoff_capsules: vec![],
            invariants: vec![],
            step_history: vec![],
            gate: None,
            lease: None,
            lease_id: None,
            workflow_id: "compatibility".to_string(),
            profile_id: "default".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            created_at: None,
            edges: vec![],
            child_summaries: vec![],
        };

        let text = format_next_output(&knot, "idea", Some("agent"), false);
        assert!(text.contains("root.1"));
        assert!(text.contains("owner: agent"));

        let json = format_next_output(&knot, "idea", None, true);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("json next output should parse");
        assert_eq!(parsed["previous_state"], "idea");
        assert_eq!(parsed["state"], "planning");

        let rollback = format_rollback_output(
            &knot,
            "ready_for_implementation",
            Some("agent"),
            "implementation is an action state",
            true,
        );
        assert!(rollback.contains("would roll back"));
        assert!(rollback.contains("owner: agent"));
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

#[cfg(test)]
#[path = "write_dispatch/tests_gate_ext.rs"]
mod tests_gate_ext;

#[cfg(test)]
#[path = "write_dispatch/tests_lease_ext.rs"]
mod tests_lease_ext;

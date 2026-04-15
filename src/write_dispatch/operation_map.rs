use crate::cli::{
    Commands, EdgeSubcommands, GateSubcommands, LeaseSubcommands, PlanStepSubcommands,
    PlanSubcommands, PlanWaveSubcommands, StepSubcommands,
};
use crate::write_queue::{
    ClaimOperation, EdgeOperation, GateEvaluateOperation, LeaseCreateOperation,
    LeaseExtendOperation, LeaseTerminateOperation, NewOperation, NextOperation,
    PlanStepAddOperation, PlanStepMoveOperation, PlanStepRemoveOperation, PlanWaveAddOperation,
    PlanWaveMoveOperation, PlanWaveRemoveOperation, PollClaimOperation, QuickNewOperation,
    RollbackOperation, StateOperation, StepAnnotateOperation, UpdateOperation, WriteOperation,
};

pub(crate) fn operation_from_command(command: &Commands) -> Option<WriteOperation> {
    match command {
        Commands::New(args) => Some(map_new(args)),
        Commands::Q(args) => Some(map_quick_new(args)),
        Commands::State(args) => Some(map_state(args)),
        Commands::Update(args) => Some(map_update(args)),
        Commands::Next(args) => Some(map_next(args)),
        Commands::Rollback(args) => Some(map_rollback(args)),
        Commands::Claim(args) if !args.peek => Some(map_claim(args)),
        Commands::Poll(args) if args.claim => Some(map_poll_claim(args)),
        Commands::Gate(args) => match &args.command {
            GateSubcommands::Evaluate(gate) => Some(map_gate_evaluate(gate)),
        },
        Commands::Plan(args) => map_plan(args),
        Commands::Edge(args) => map_edge(args),
        Commands::Step(args) => match &args.command {
            StepSubcommands::Annotate(a) => Some(map_step_annotate(a)),
        },
        Commands::Lease(args) => map_lease(args),
        _ => None,
    }
}

fn map_new(args: &crate::cli::NewArgs) -> WriteOperation {
    WriteOperation::New(NewOperation {
        title: args.title.clone(),
        description: args.desc.clone(),
        acceptance: args.acceptance.clone(),
        state: args.state.clone(),
        profile: args.profile.clone(),
        workflow: args.workflow.clone(),
        fast: args.fast,
        exploration: args.exploration,
        knot_type: args.knot_type.clone(),
        gate_owner_kind: args.gate_owner_kind.clone(),
        gate_failure_modes: args.gate_failure_modes.clone(),
        tags: args.tags.clone(),
        lease_id: args.lease.clone(),
    })
}

fn map_quick_new(args: &crate::cli::QuickNewArgs) -> WriteOperation {
    WriteOperation::QuickNew(QuickNewOperation {
        title: args.title.clone(),
        description: args.desc.clone(),
        state: args.state.clone(),
    })
}

fn map_state(args: &crate::cli::StateArgs) -> WriteOperation {
    WriteOperation::State(StateOperation {
        id: args.id.clone(),
        state: args.state.clone(),
        force: args.force,
        approve_terminal_cascade: args.cascade_terminal_descendants,
        if_match: args.if_match.clone(),
        actor_kind: args.actor_kind.clone(),
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
    })
}

fn map_update(args: &crate::cli::UpdateArgs) -> WriteOperation {
    WriteOperation::Update(UpdateOperation {
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
        execution_plan_file: args
            .execution_plan_file
            .as_ref()
            .map(|path| absolutize_path(path).to_string_lossy().into_owned()),
        if_match: args.if_match.clone(),
        actor_kind: args.actor_kind.clone(),
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
        force: args.force,
        approve_terminal_cascade: args.cascade_terminal_descendants,
        lease_id: args.lease.clone(),
    })
}

fn absolutize_path(path: &std::path::Path) -> std::path::PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    }
}

fn map_next(args: &crate::cli::NextArgs) -> WriteOperation {
    WriteOperation::Next(NextOperation {
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
    })
}

fn map_rollback(args: &crate::cli::RollbackArgs) -> WriteOperation {
    WriteOperation::Rollback(RollbackOperation {
        id: args.id.clone(),
        dry_run: args.dry_run,
        actor_kind: args.actor_kind.clone(),
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
    })
}

fn map_claim(args: &crate::cli::ClaimArgs) -> WriteOperation {
    WriteOperation::Claim(ClaimOperation {
        id: args.id.clone(),
        json: args.json,
        verbose: args.verbose,
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
        lease_id: args.lease.clone(),
        timeout_seconds: args.timeout_seconds,
    })
}

fn map_poll_claim(args: &crate::cli::PollArgs) -> WriteOperation {
    WriteOperation::PollClaim(PollClaimOperation {
        stage: args.stage.clone(),
        owner: args.owner.clone(),
        json: args.json,
        agent_name: args.agent_name.clone(),
        agent_model: args.agent_model.clone(),
        agent_version: args.agent_version.clone(),
        timeout_seconds: args.timeout_seconds,
    })
}

fn map_gate_evaluate(gate: &crate::cli::GateEvaluateArgs) -> WriteOperation {
    WriteOperation::GateEvaluate(GateEvaluateOperation {
        id: gate.id.clone(),
        decision: gate.decision.clone(),
        invariant: gate.invariant.clone(),
        json: gate.json,
        actor_kind: gate.actor_kind.clone(),
        agent_name: gate.agent_name.clone(),
        agent_model: gate.agent_model.clone(),
        agent_version: gate.agent_version.clone(),
    })
}

fn map_plan(args: &crate::cli::PlanArgs) -> Option<WriteOperation> {
    match &args.command {
        PlanSubcommands::Wave(wave) => match &wave.command {
            PlanWaveSubcommands::Add(add) => {
                Some(WriteOperation::PlanWaveAdd(PlanWaveAddOperation {
                    id: add.id.clone(),
                    name: add.name.clone(),
                    objective: add.objective.clone(),
                    at: add.at,
                }))
            }
            PlanWaveSubcommands::Remove(remove) => {
                Some(WriteOperation::PlanWaveRemove(PlanWaveRemoveOperation {
                    id: remove.id.clone(),
                    wave: remove.wave,
                    force: remove.force,
                }))
            }
            PlanWaveSubcommands::Move(mv) => {
                Some(WriteOperation::PlanWaveMove(PlanWaveMoveOperation {
                    id: mv.id.clone(),
                    from_index: mv.from_index,
                    to_index: mv.to_index,
                }))
            }
        },
        PlanSubcommands::Step(step) => match &step.command {
            PlanStepSubcommands::Add(add) => {
                Some(WriteOperation::PlanStepAdd(PlanStepAddOperation {
                    id: add.id.clone(),
                    wave: add.wave,
                    knot_ids: add.knot_ids.clone(),
                    notes: add.notes.clone(),
                    at: add.at,
                }))
            }
            PlanStepSubcommands::Remove(remove) => {
                Some(WriteOperation::PlanStepRemove(PlanStepRemoveOperation {
                    id: remove.id.clone(),
                    wave: remove.wave,
                    step: remove.step,
                    force: remove.force,
                }))
            }
            PlanStepSubcommands::Move(mv) => {
                Some(WriteOperation::PlanStepMove(PlanStepMoveOperation {
                    id: mv.id.clone(),
                    wave: mv.wave,
                    from_index: mv.from_index,
                    to_index: mv.to_index,
                }))
            }
        },
    }
}

fn map_edge(args: &crate::cli::EdgeArgs) -> Option<WriteOperation> {
    match &args.command {
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
    }
}

fn map_step_annotate(a: &crate::cli::StepAnnotateArgs) -> WriteOperation {
    WriteOperation::StepAnnotate(StepAnnotateOperation {
        id: a.id.clone(),
        actor_kind: a.actor_kind.clone(),
        agent_name: a.agent_name.clone(),
        agent_model: a.agent_model.clone(),
        agent_version: a.agent_version.clone(),
        json: a.json,
    })
}

fn map_lease(args: &crate::cli::LeaseArgs) -> Option<WriteOperation> {
    match &args.command {
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
                timeout_seconds: create.timeout_seconds,
            }))
        }
        LeaseSubcommands::Terminate(term) => {
            Some(WriteOperation::LeaseTerminate(LeaseTerminateOperation {
                id: term.id.clone(),
            }))
        }
        LeaseSubcommands::Extend(ext) => Some(WriteOperation::LeaseExtend(LeaseExtendOperation {
            lease_id: ext.lease_id.clone(),
            timeout_seconds: ext.timeout_seconds,
            json: ext.json,
        })),
        _ => None, // Show and List are read operations
    }
}

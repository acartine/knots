use std::io::{self, IsTerminal};

use crate::app::{App, AppError};
use crate::domain::execution_plan_edit::CascadeInfo;
use crate::domain::execution_plan_edit::{remove_step, remove_wave};
use crate::ui;
use crate::write_dispatch::helpers::plan_cascade_prompt;
use crate::write_queue::{
    PlanStepAddOperation, PlanStepMoveOperation, PlanStepRemoveOperation, PlanWaveAddOperation,
    PlanWaveMoveOperation, PlanWaveRemoveOperation,
};

pub(super) fn execute_wave_add(app: &App, args: &PlanWaveAddOperation) -> Result<String, AppError> {
    let knot =
        app.plan_edit_wave_add(&args.id, args.name.clone(), args.objective.clone(), args.at)?;
    Ok(format_edit_output("wave added", &knot.id))
}

pub(crate) fn execute_wave_remove(
    app: &App,
    args: &PlanWaveRemoveOperation,
) -> Result<String, AppError> {
    let cascade = preview_wave_cascade(app, &args.id, args.wave)?;
    confirm_plan_cascade(
        args.force,
        &format!("removing wave {} from {}", args.wave, args.id),
        &cascade,
    )?;
    let (knot, _) = app.plan_edit_wave_remove(&args.id, args.wave)?;
    Ok(format!(
        "wave removed {} wave={}\n",
        palette_id(&knot.id),
        args.wave,
    ))
}

pub(super) fn execute_wave_move(
    app: &App,
    args: &PlanWaveMoveOperation,
) -> Result<String, AppError> {
    let knot = app.plan_edit_wave_move(&args.id, args.from_index, args.to_index)?;
    Ok(format!(
        "wave moved {} {}->{}\n",
        palette_id(&knot.id),
        args.from_index,
        args.to_index,
    ))
}

pub(super) fn execute_step_add(app: &App, args: &PlanStepAddOperation) -> Result<String, AppError> {
    let knot = app.plan_edit_step_add(
        &args.id,
        args.wave,
        args.knot_ids.clone(),
        args.notes.clone(),
        args.at,
    )?;
    Ok(format_edit_output("step added", &knot.id))
}

pub(crate) fn execute_step_remove(
    app: &App,
    args: &PlanStepRemoveOperation,
) -> Result<String, AppError> {
    let cascade = preview_step_cascade(app, &args.id, args.wave, args.step)?;
    confirm_plan_cascade(
        args.force,
        &format!(
            "removing step {} from wave {} in {}",
            args.step, args.wave, args.id
        ),
        &cascade,
    )?;
    let (knot, _) = app.plan_edit_step_remove(&args.id, args.wave, args.step)?;
    Ok(format!(
        "step removed {} wave={} step={}\n",
        palette_id(&knot.id),
        args.wave,
        args.step,
    ))
}

pub(crate) fn execute_step_move(
    app: &App,
    args: &PlanStepMoveOperation,
) -> Result<String, AppError> {
    let knot = app.plan_edit_step_move(&args.id, args.wave, args.from_index, args.to_index)?;
    Ok(format!(
        "step moved {} wave={} {}->{}\n",
        palette_id(&knot.id),
        args.wave,
        args.from_index,
        args.to_index,
    ))
}

pub(crate) fn confirm_plan_cascade(
    force: bool,
    summary: &str,
    cascade: &CascadeInfo,
) -> Result<(), AppError> {
    if !requires_confirmation(cascade) || force {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        return Err(AppError::InvalidArgument(
            "plan cascade requires a TTY unless --force is set".to_string(),
        ));
    }
    let mut stderr = io::stderr();
    let mut stdin = io::stdin().lock();
    if plan_cascade_prompt(&mut stderr, &mut stdin, summary, cascade)? {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(
            "plan cascade cancelled; no changes written".to_string(),
        ))
    }
}

pub(crate) fn requires_confirmation(cascade: &CascadeInfo) -> bool {
    cascade.step_count > 0 || !cascade.affected_knot_ids.is_empty()
}

fn format_edit_output(action: &str, id: &str) -> String {
    format!("{} {}\n", action, palette_id(id))
}

fn palette_id(id: &str) -> String {
    let palette = ui::Palette::auto();
    palette.id(id)
}

fn preview_wave_cascade(app: &App, id: &str, wave_index: u32) -> Result<CascadeInfo, AppError> {
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.to_string()))?;
    let plan = knot.execution_plan.unwrap_or_default();
    let (_, cascade) =
        remove_wave(&plan, wave_index).map_err(|err| AppError::InvalidArgument(err.to_string()))?;
    Ok(cascade)
}

fn preview_step_cascade(
    app: &App,
    id: &str,
    wave_index: u32,
    step_index: u32,
) -> Result<CascadeInfo, AppError> {
    let knot = app
        .show_knot(id)?
        .ok_or_else(|| AppError::NotFound(id.to_string()))?;
    let plan = knot.execution_plan.unwrap_or_default();
    let (_, cascade) = remove_step(&plan, wave_index, step_index)
        .map_err(|err| AppError::InvalidArgument(err.to_string()))?;
    Ok(cascade)
}

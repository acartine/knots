#![allow(dead_code)]

use std::collections::HashSet;
use std::fmt;

use super::execution_plan::{ExecutionPlanData, ExecutionPlanStep, ExecutionPlanWave};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CascadeInfo {
    pub affected_knot_ids: Vec<String>,
    pub step_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanEditError {
    WaveNotFound(u32),
    StepNotFound {
        wave_index: u32,
        step_index: u32,
    },
    IndexOutOfBounds {
        kind: &'static str,
        index: u32,
        len: usize,
    },
}

impl fmt::Display for PlanEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanEditError::WaveNotFound(index) => {
                write!(f, "wave {} not found", index)
            }
            PlanEditError::StepNotFound {
                wave_index,
                step_index,
            } => write!(f, "step {} not found in wave {}", step_index, wave_index),
            PlanEditError::IndexOutOfBounds { kind, index, len } => {
                write!(
                    f,
                    "{} index {} is out of bounds for {} {}",
                    kind,
                    index,
                    len,
                    if *len == 1 { "item" } else { "items" }
                )
            }
        }
    }
}

impl std::error::Error for PlanEditError {}

pub fn add_wave(
    plan: &ExecutionPlanData,
    name: String,
    objective: String,
    at: Option<u32>,
) -> Result<ExecutionPlanData, PlanEditError> {
    let mut next = canonicalize_plan(plan);
    let insert_at = insertion_offset(next.waves.len(), at, "wave")?;
    next.waves.insert(
        insert_at,
        ExecutionPlanWave {
            name,
            objective,
            ..Default::default()
        },
    );
    renumber_waves(&mut next);
    Ok(next)
}

pub fn remove_wave(
    plan: &ExecutionPlanData,
    wave_index: u32,
) -> Result<(ExecutionPlanData, CascadeInfo), PlanEditError> {
    let mut next = canonicalize_plan(plan);
    let index = existing_offset(next.waves.len(), wave_index, "wave")?;
    let wave = next.waves.remove(index);
    let cascade = cascade_from_wave(&wave);
    renumber_waves(&mut next);
    Ok((next, cascade))
}

pub fn move_wave(
    plan: &ExecutionPlanData,
    from: u32,
    to: u32,
) -> Result<ExecutionPlanData, PlanEditError> {
    let mut next = canonicalize_plan(plan);
    move_item(&mut next.waves, from, to, "wave")?;
    renumber_waves(&mut next);
    Ok(next)
}

pub fn add_step(
    plan: &ExecutionPlanData,
    wave_index: u32,
    knot_ids: Vec<String>,
    notes: Option<String>,
    at: Option<u32>,
) -> Result<ExecutionPlanData, PlanEditError> {
    let mut next = canonicalize_plan(plan);
    let wave = wave_mut_by_index(&mut next, wave_index)?;
    let insert_at = insertion_offset(wave.steps.len(), at, "step")?;
    wave.steps.insert(
        insert_at,
        ExecutionPlanStep {
            knot_ids,
            notes,
            ..Default::default()
        },
    );
    renumber_steps(wave);
    Ok(next)
}

pub fn remove_step(
    plan: &ExecutionPlanData,
    wave_index: u32,
    step_index: u32,
) -> Result<(ExecutionPlanData, CascadeInfo), PlanEditError> {
    let mut next = canonicalize_plan(plan);
    let wave = wave_mut_by_index(&mut next, wave_index)?;
    let index = existing_offset(wave.steps.len(), step_index, "step")?;
    let step = wave.steps.remove(index);
    let cascade = cascade_from_step(&step);
    renumber_steps(wave);
    Ok((next, cascade))
}

pub fn move_step(
    plan: &ExecutionPlanData,
    wave_index: u32,
    from: u32,
    to: u32,
) -> Result<ExecutionPlanData, PlanEditError> {
    let mut next = canonicalize_plan(plan);
    let wave = wave_mut_by_index(&mut next, wave_index)?;
    move_item(&mut wave.steps, from, to, "step")?;
    renumber_steps(wave);
    Ok(next)
}

fn canonicalize_plan(plan: &ExecutionPlanData) -> ExecutionPlanData {
    let mut next = plan.clone();
    next.waves.sort_by_key(|wave| wave.wave_index);
    for wave in &mut next.waves {
        wave.steps.sort_by_key(|step| step.step_index);
    }
    next
}

fn wave_mut_by_index(
    plan: &mut ExecutionPlanData,
    wave_index: u32,
) -> Result<&mut ExecutionPlanWave, PlanEditError> {
    let offset = existing_offset(plan.waves.len(), wave_index, "wave")?;
    Ok(&mut plan.waves[offset])
}

fn move_item<T>(
    items: &mut Vec<T>,
    from: u32,
    to: u32,
    kind: &'static str,
) -> Result<(), PlanEditError> {
    let from_index = existing_offset(items.len(), from, kind)?;
    let item = items.remove(from_index);
    let to_index = insertion_offset(items.len(), Some(to), kind)?;
    items.insert(to_index, item);
    Ok(())
}

fn insertion_offset(
    len: usize,
    at: Option<u32>,
    kind: &'static str,
) -> Result<usize, PlanEditError> {
    let index = at.unwrap_or((len + 1) as u32);
    if index == 0 || index as usize > len + 1 {
        return Err(PlanEditError::IndexOutOfBounds { kind, index, len });
    }
    Ok(index as usize - 1)
}

fn existing_offset(len: usize, index: u32, kind: &'static str) -> Result<usize, PlanEditError> {
    if index == 0 {
        return Err(PlanEditError::IndexOutOfBounds { kind, index, len });
    }
    if index as usize > len {
        return Err(match kind {
            "wave" => PlanEditError::WaveNotFound(index),
            "step" => PlanEditError::StepNotFound {
                wave_index: 0,
                step_index: index,
            },
            _ => PlanEditError::IndexOutOfBounds { kind, index, len },
        });
    }
    Ok(index as usize - 1)
}

fn renumber_waves(plan: &mut ExecutionPlanData) {
    for (index, wave) in plan.waves.iter_mut().enumerate() {
        wave.wave_index = (index + 1) as u32;
        renumber_steps(wave);
    }
}

fn renumber_steps(wave: &mut ExecutionPlanWave) {
    for (index, step) in wave.steps.iter_mut().enumerate() {
        step.step_index = (index + 1) as u32;
    }
}

fn cascade_from_wave(wave: &ExecutionPlanWave) -> CascadeInfo {
    let mut affected_knot_ids = Vec::new();
    let mut seen = HashSet::new();
    for knot in &wave.knots {
        push_unique(&mut affected_knot_ids, &mut seen, &knot.id);
    }
    for step in &wave.steps {
        for knot_id in &step.knot_ids {
            push_unique(&mut affected_knot_ids, &mut seen, knot_id);
        }
    }
    CascadeInfo {
        affected_knot_ids,
        step_count: wave.steps.len(),
    }
}

fn cascade_from_step(step: &ExecutionPlanStep) -> CascadeInfo {
    let mut affected_knot_ids = Vec::new();
    let mut seen = HashSet::new();
    for knot_id in &step.knot_ids {
        push_unique(&mut affected_knot_ids, &mut seen, knot_id);
    }
    CascadeInfo {
        affected_knot_ids,
        step_count: 1,
    }
}

fn push_unique(ids: &mut Vec<String>, seen: &mut HashSet<String>, id: &str) {
    if seen.insert(id.to_string()) {
        ids.push(id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> ExecutionPlanData {
        ExecutionPlanData {
            repo_path: Some("/repo".to_string()),
            objective: Some("Ship".to_string()),
            summary: Some("summary".to_string()),
            mode: Some("autopilot".to_string()),
            model: Some("gpt-5".to_string()),
            assumptions: vec!["assume".to_string()],
            knot_ids: vec!["root".to_string()],
            unassigned_knot_ids: vec!["spare".to_string()],
            waves: vec![
                ExecutionPlanWave {
                    wave_index: 1,
                    name: "first".to_string(),
                    objective: "one".to_string(),
                    knots: vec![super::super::execution_plan::ExecutionPlanKnot {
                        id: "k-1".to_string(),
                        title: "k1".to_string(),
                    }],
                    steps: vec![ExecutionPlanStep {
                        step_index: 1,
                        knot_ids: vec!["s-1".to_string()],
                        notes: Some("a".to_string()),
                    }],
                    ..Default::default()
                },
                ExecutionPlanWave {
                    wave_index: 2,
                    name: "second".to_string(),
                    objective: "two".to_string(),
                    knots: vec![super::super::execution_plan::ExecutionPlanKnot {
                        id: "k-2".to_string(),
                        title: "k2".to_string(),
                    }],
                    steps: vec![
                        ExecutionPlanStep {
                            step_index: 1,
                            knot_ids: vec!["s-2".to_string()],
                            notes: None,
                        },
                        ExecutionPlanStep {
                            step_index: 2,
                            knot_ids: vec!["s-3".to_string(), "s-4".to_string()],
                            notes: None,
                        },
                    ],
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn add_wave_appends_to_empty_plan() {
        let plan = ExecutionPlanData::default();
        let next =
            add_wave(&plan, "alpha".to_string(), "objective".to_string(), None).expect("add wave");
        assert_eq!(next.waves.len(), 1);
        assert_eq!(next.waves[0].wave_index, 1);
        assert_eq!(next.waves[0].name, "alpha");
        assert_eq!(next.waves[0].objective, "objective");
        assert_eq!(next.repo_path, None);
    }

    #[test]
    fn add_wave_inserts_at_position() {
        let plan = sample_plan();
        let next =
            add_wave(&plan, "inserted".to_string(), "obj".to_string(), Some(2)).expect("add wave");
        assert_eq!(next.waves.len(), 3);
        assert_eq!(next.waves[1].name, "inserted");
        assert_eq!(next.waves[1].wave_index, 2);
        assert_eq!(next.waves[2].wave_index, 3);
    }

    #[test]
    fn remove_wave_cascades_knot_ids() {
        let plan = sample_plan();
        let (next, cascade) = remove_wave(&plan, 2).expect("remove wave");
        assert_eq!(next.waves.len(), 1);
        assert_eq!(next.waves[0].wave_index, 1);
        assert_eq!(cascade.step_count, 2);
        assert_eq!(cascade.affected_knot_ids, vec!["k-2", "s-2", "s-3", "s-4"]);
    }

    #[test]
    fn remove_wave_from_empty_plan_errors() {
        let err = remove_wave(&ExecutionPlanData::default(), 1).unwrap_err();
        assert_eq!(err, PlanEditError::WaveNotFound(1));
    }

    #[test]
    fn remove_wave_out_of_bounds_errors() {
        let err = remove_wave(&sample_plan(), 3).unwrap_err();
        assert_eq!(err, PlanEditError::WaveNotFound(3));
    }

    #[test]
    fn move_wave_swaps_and_renumbers() {
        let plan = sample_plan();
        let next = move_wave(&plan, 1, 2).expect("move wave");
        assert_eq!(next.waves[0].name, "second");
        assert_eq!(next.waves[0].wave_index, 1);
        assert_eq!(next.waves[1].name, "first");
        assert_eq!(next.waves[1].wave_index, 2);
    }

    #[test]
    fn move_wave_out_of_bounds_errors() {
        let err = move_wave(&sample_plan(), 0, 2).unwrap_err();
        assert_eq!(
            err,
            PlanEditError::IndexOutOfBounds {
                kind: "wave",
                index: 0,
                len: 2,
            }
        );
    }

    #[test]
    fn add_step_to_wave() {
        let plan = sample_plan();
        let next = add_step(
            &plan,
            1,
            vec!["knot-a".to_string()],
            Some("notes".to_string()),
            Some(1),
        )
        .expect("add step");
        assert_eq!(next.waves[0].steps.len(), 2);
        assert_eq!(next.waves[0].steps[0].step_index, 1);
        assert_eq!(next.waves[0].steps[0].knot_ids, vec!["knot-a"]);
        assert_eq!(next.waves[0].steps[1].step_index, 2);
    }

    #[test]
    fn add_step_to_nonexistent_wave_errors() {
        let err = add_step(&sample_plan(), 3, vec![], None, None).unwrap_err();
        assert_eq!(err, PlanEditError::WaveNotFound(3));
    }

    #[test]
    fn remove_step_cascades_knot_ids() {
        let plan = sample_plan();
        let (next, cascade) = remove_step(&plan, 2, 2).expect("remove step");
        assert_eq!(next.waves[1].steps.len(), 1);
        assert_eq!(cascade.step_count, 1);
        assert_eq!(cascade.affected_knot_ids, vec!["s-3", "s-4"]);
    }

    #[test]
    fn remove_step_out_of_bounds_errors() {
        let err = remove_step(&sample_plan(), 1, 3).unwrap_err();
        assert_eq!(
            err,
            PlanEditError::StepNotFound {
                wave_index: 0,
                step_index: 3,
            }
        );
    }

    #[test]
    fn move_step_swaps_and_renumbers() {
        let plan = sample_plan();
        let next = move_step(&plan, 2, 1, 2).expect("move step");
        assert_eq!(next.waves[1].steps[0].knot_ids, vec!["s-3", "s-4"]);
        assert_eq!(next.waves[1].steps[0].step_index, 1);
        assert_eq!(next.waves[1].steps[1].knot_ids, vec!["s-2"]);
        assert_eq!(next.waves[1].steps[1].step_index, 2);
    }

    #[test]
    fn renumber_waves_fixes_gaps() {
        let mut plan = sample_plan();
        plan.waves[0].wave_index = 3;
        plan.waves[1].wave_index = 9;
        renumber_waves(&mut plan);
        assert_eq!(plan.waves[0].wave_index, 1);
        assert_eq!(plan.waves[1].wave_index, 2);
        assert_eq!(plan.waves[0].steps[0].step_index, 1);
    }

    #[test]
    fn renumber_steps_fixes_gaps() {
        let mut wave = sample_plan().waves[1].clone();
        wave.steps[0].step_index = 4;
        wave.steps[1].step_index = 8;
        renumber_steps(&mut wave);
        assert_eq!(wave.steps[0].step_index, 1);
        assert_eq!(wave.steps[1].step_index, 2);
    }

    #[test]
    fn plan_edit_error_display_covers_remaining_variants() {
        assert_eq!(
            PlanEditError::WaveNotFound(7).to_string(),
            "wave 7 not found"
        );
        assert_eq!(
            PlanEditError::StepNotFound {
                wave_index: 2,
                step_index: 3,
            }
            .to_string(),
            "step 3 not found in wave 2"
        );
        assert_eq!(
            PlanEditError::IndexOutOfBounds {
                kind: "step",
                index: 4,
                len: 1,
            }
            .to_string(),
            "step index 4 is out of bounds for 1 item"
        );
    }
}

use std::fs;
use std::path::PathBuf;

use crate::app::AppError;

use super::{managed_skills, render_skill, skill_path, write_skills, ManagedSkill, SkillLocation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SkillDoctorState {
    pub(super) missing: Vec<ManagedSkill>,
    pub(super) drifted: Vec<ManagedSkill>,
}

impl SkillDoctorState {
    pub(super) fn is_current(&self) -> bool {
        self.missing.is_empty() && self.drifted.is_empty()
    }

    fn changed_skills(&self) -> Vec<ManagedSkill> {
        self.missing
            .iter()
            .chain(self.drifted.iter())
            .copied()
            .collect()
    }
}

pub(super) fn inspect_location(location: &SkillLocation) -> SkillDoctorState {
    let mut missing = Vec::new();
    let mut drifted = Vec::new();

    for skill in managed_skills().iter().copied() {
        match skill_state(location, skill) {
            SkillState::Current => {}
            SkillState::Missing => missing.push(skill),
            SkillState::Drifted => drifted.push(skill),
        }
    }

    SkillDoctorState { missing, drifted }
}

pub(super) fn reconcile_skills(location: &SkillLocation) -> Result<Vec<PathBuf>, AppError> {
    let state = inspect_location(location);
    if state.is_current() {
        return Ok(Vec::new());
    }
    write_skills(location, &state.changed_skills())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillState {
    Current,
    Missing,
    Drifted,
}

fn skill_state(location: &SkillLocation, skill: ManagedSkill) -> SkillState {
    let path = skill_path(location, skill);
    match fs::read_to_string(&path) {
        Ok(contents) => {
            if contents == render_skill(skill) {
                SkillState::Current
            } else {
                SkillState::Drifted
            }
        }
        Err(_) => {
            if path.exists() {
                SkillState::Drifted
            } else {
                SkillState::Missing
            }
        }
    }
}

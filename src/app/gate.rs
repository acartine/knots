use std::time::Duration;

use crate::db;
use crate::domain::knot_type::{parse_knot_type, KnotType};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::helpers::non_empty;
use super::types::{GateDecision, GateEvaluationResult, KnotView, StateActorMetadata};
use super::App;

impl App {
    pub fn evaluate_gate(
        &self,
        id: &str,
        decision: GateDecision,
        invariant: Option<&str>,
        state_actor: StateActorMetadata,
    ) -> Result<GateEvaluationResult, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.clone()))?;
        if parse_knot_type(current.knot_type.as_deref()) != KnotType::Gate {
            return Err(AppError::InvalidArgument(format!(
                "knot '{}' is not a gate",
                current.id
            )));
        }
        if current.state != workflow_runtime::EVALUATING {
            return Err(AppError::InvalidArgument(format!(
                "gate '{}' must be in '{}' to evaluate",
                current.id,
                workflow_runtime::EVALUATING
            )));
        }
        match decision {
            GateDecision::Yes => self.evaluate_gate_yes(&current, &state_actor),
            GateDecision::No => self.evaluate_gate_no(&current, invariant, &state_actor),
        }
    }

    fn evaluate_gate_yes(
        &self,
        current: &crate::db::KnotCacheRecord,
        state_actor: &StateActorMetadata,
    ) -> Result<GateEvaluationResult, AppError> {
        let updated = self.write_state_change_locked(
            current,
            "shipped",
            false,
            current.profile_etag.as_deref(),
            state_actor,
            None,
        )?;
        Ok(GateEvaluationResult {
            gate: self.apply_alias_and_enrich_knot(KnotView::from(updated))?,
            decision: "yes".to_string(),
            invariant: None,
            reopened: Vec::new(),
        })
    }

    fn evaluate_gate_no(
        &self,
        current: &crate::db::KnotCacheRecord,
        invariant: Option<&str>,
        state_actor: &StateActorMetadata,
    ) -> Result<GateEvaluationResult, AppError> {
        let violated = non_empty(invariant.unwrap_or("")).ok_or_else(|| {
            AppError::InvalidArgument(
                "--invariant is required when gate decision \
                     is 'no'"
                    .to_string(),
            )
        })?;
        self.validate_gate_invariant(current, &violated)?;
        let reopen_targets = current
            .gate_data
            .find_reopen_targets(&violated)
            .cloned()
            .ok_or_else(|| {
                AppError::InvalidArgument(format!(
                    "gate '{}' has no failure mode for \
                     invariant '{}'",
                    current.id, violated
                ))
            })?;
        let reopened =
            self.reopen_gate_targets(current, &reopen_targets, &violated, state_actor)?;
        let updated = self.write_state_change_locked(
            current,
            "abandoned",
            true,
            current.profile_etag.as_deref(),
            state_actor,
            None,
        )?;
        Ok(GateEvaluationResult {
            gate: self.apply_alias_and_enrich_knot(KnotView::from(updated))?,
            decision: "no".to_string(),
            invariant: Some(violated),
            reopened,
        })
    }

    fn validate_gate_invariant(
        &self,
        current: &crate::db::KnotCacheRecord,
        violated: &str,
    ) -> Result<(), AppError> {
        let matches = current.invariants.iter().any(|item| {
            crate::domain::gate::normalize_invariant_key(&item.condition)
                == crate::domain::gate::normalize_invariant_key(violated)
        });
        if !matches {
            return Err(AppError::InvalidArgument(format!(
                "gate '{}' does not define invariant '{}'",
                current.id, violated
            )));
        }
        Ok(())
    }

    fn reopen_gate_targets(
        &self,
        current: &crate::db::KnotCacheRecord,
        targets: &[String],
        violated: &str,
        state_actor: &StateActorMetadata,
    ) -> Result<Vec<String>, AppError> {
        let mut reopened = Vec::new();
        for target in targets {
            let target_id = self.resolve_knot_token(target)?;
            let target_rec = db::get_knot_hot(&self.conn, &target_id)?
                .ok_or_else(|| AppError::NotFound(target_id.clone()))?;
            let rec = if target_rec.state == "ready_for_planning" {
                target_rec
            } else {
                self.write_state_change_locked(
                    &target_rec,
                    "ready_for_planning",
                    true,
                    target_rec.profile_etag.as_deref(),
                    state_actor,
                    None,
                )?
            };
            self.append_gate_failure_metadata_locked(&rec, &current.id, violated, state_actor)?;
            reopened.push(target_id);
        }
        Ok(reopened)
    }
}

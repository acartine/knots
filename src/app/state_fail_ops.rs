//! Atomic failure-outcome transitions.
//!
//! A failed action does not simply rewind one queue state: the workflow
//! declares where each named failure outcome lands (for example shipment's
//! `merge_conflicts` -> `ready_for_implementation`). Recording that outcome
//! has to move the knot, close the open step as failed, unbind the claim
//! lease, and terminate the lease knot. Doing that through the public `App`
//! methods would take several separately locked writes, and `FileLock` is
//! not reentrant, so this module performs the whole sequence inside one
//! locked section with the state change and the lease unbind sharing a
//! single `upsert_knot_hot`.

use std::time::Duration;

use crate::db;
use crate::domain::knot_type::parse_knot_type;
use crate::domain::step_history::{StepRecord, StepStatus};
use crate::lease_expiry::{
    compute_expiry_ts, effective_lease_state, DEFAULT_LEASE_TIMEOUT_SECONDS,
};
use crate::locks::FileLock;
use crate::workflow_runtime;

use super::error::AppError;
use super::types::{KnotView, StateActorMetadata};
use super::App;

/// Same-write overrides for `write_state_change_locked_with`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StateWriteOverrides {
    /// Clear the knot's `lease_id` in the same write as the state change.
    pub clear_lease: bool,
    /// Close the active step record as failed instead of completed, and do
    /// not open a step for the target state.
    pub fail_step: bool,
}

/// Close every active step record as failed, pointing at `to_state`.
///
/// Unlike `apply_step_transition`, this never opens a new step: a failure
/// outcome parks the knot in a queue or escape state where the next actor
/// must claim again.
///
/// `actor` and `lease_id` come from the lease bound at failure time. They
/// only fill gaps: `kno claim` transitions the state before it binds the
/// lease, so the record that opened the step can carry no identity at all,
/// and the failure is the first point where the responsible agent is known.
/// Identity already on the record always wins.
pub(crate) fn apply_step_failure_transition(
    existing: &[StepRecord],
    from_state: &str,
    to_state: &str,
    occurred_at: &str,
    actor: &StateActorMetadata,
    lease_id: Option<&str>,
) -> Vec<StepRecord> {
    let mut history: Vec<StepRecord> = existing.to_vec();
    if from_state == to_state {
        return history;
    }
    for record in &mut history {
        if record.is_active() {
            record.to_state = Some(to_state.to_string());
            record.ended_at = Some(occurred_at.to_string());
            record.status = StepStatus::Failed;
            fill_missing(&mut record.actor_kind, actor.actor_kind.as_deref());
            fill_missing(&mut record.agent_name, actor.agent_name.as_deref());
            fill_missing(&mut record.agent_model, actor.agent_model.as_deref());
            fill_missing(&mut record.agent_version, actor.agent_version.as_deref());
            fill_missing(&mut record.lease_id, lease_id);
        }
    }
    history
}

fn fill_missing(slot: &mut Option<String>, value: Option<&str>) {
    if slot.is_none() {
        *slot = value.map(ToString::to_string);
    }
}

impl App {
    /// Record a failed action outcome and rest the knot in `target_state`.
    ///
    /// The state change, the lease unbind, and the failed step close land in
    /// one write; the bound lease knot is terminated (or, for MCP session
    /// leases, returned to `lease_ready`) inside the same lock.
    pub(crate) fn fail_state_transition(
        &self,
        id: &str,
        target_state: &str,
        force: bool,
        state_actor: StateActorMetadata,
    ) -> Result<KnotView, AppError> {
        let id = self.resolve_knot_token(id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let current =
            db::get_knot_hot(&self.conn, &id)?.ok_or_else(|| AppError::NotFound(id.to_string()))?;
        let bound_lease = current.lease_id.clone();
        let updated = self.write_state_change_locked_with(
            &current,
            target_state,
            force,
            None,
            &state_actor,
            None,
            StateWriteOverrides {
                clear_lease: true,
                fail_step: true,
            },
        )?;
        if let Some(lease_id) = bound_lease.as_deref() {
            self.release_lease_knot_locked(lease_id)?;
        }
        Ok(self.apply_alias_and_enrich_knot_post_write(KnotView::from(updated)))
    }

    /// Materialize an expired lease bound to one action-state knot.
    ///
    /// Expiry is lazy, so the caller may have observed a lease that is raw
    /// `lease_active` but effectively terminated. Re-read both records while
    /// holding the write locks before changing either one.
    pub(crate) fn recover_expired_bound_lease(&self, knot_id: &str) -> Result<bool, AppError> {
        let knot_id = self.resolve_knot_token(knot_id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let knot =
            db::get_knot_hot(&self.conn, &knot_id)?.ok_or_else(|| AppError::NotFound(knot_id))?;
        let Some(lease_token) = knot.lease_id.as_deref() else {
            return Ok(false);
        };
        let lease_id = self.resolve_knot_token(lease_token)?;
        let Some(lease) = db::get_knot_hot(&self.conn, &lease_id)? else {
            return Ok(false);
        };
        if parse_knot_type(lease.knot_type.as_deref()) != crate::domain::knot_type::KnotType::Lease
            || lease.state == workflow_runtime::LEASE_TERMINATED
            || effective_lease_state(&lease.state, lease.lease_expiry_ts)
                != workflow_runtime::LEASE_TERMINATED
        {
            return Ok(false);
        }
        self.recover_bound_action_locked(&knot, &lease)
    }

    /// Terminate a lease and recover the action-state knot still bound to it.
    ///
    /// A lease may be terminated after its knot has already moved into an
    /// action state. Keep the release and rollback under one lock boundary so
    /// callers never observe a dead lease pinning an action state.
    pub(crate) fn terminate_lease_and_recover_bound_action(
        &self,
        lease_id: &str,
    ) -> Result<KnotView, AppError> {
        let lease_id = self.resolve_knot_token(lease_id)?;
        let _repo_guard = FileLock::acquire(&self.repo_lock_path(), Duration::from_millis(5_000))?;
        let _cache_guard =
            FileLock::acquire(&self.cache_lock_path(), Duration::from_millis(5_000))?;
        let lease = db::get_knot_hot(&self.conn, &lease_id)?
            .ok_or_else(|| AppError::NotFound(lease_id.clone()))?;
        if parse_knot_type(lease.knot_type.as_deref()) != crate::domain::knot_type::KnotType::Lease
        {
            return Err(AppError::InvalidArgument(
                "specified knot is not a lease".to_string(),
            ));
        }
        let mut bound_actions = Vec::new();
        for knot in db::list_knot_hot(&self.conn)? {
            let Some(bound_token) = knot.lease_id.as_deref() else {
                continue;
            };
            if parse_knot_type(knot.knot_type.as_deref())
                == crate::domain::knot_type::KnotType::Lease
                || self.resolve_knot_token(bound_token)? != lease_id
            {
                continue;
            }
            // An unrelated knot with an unresolvable profile must not abort
            // this lease release; skip it with a warning instead.
            let profile = match self.resolve_profile_for_record(&knot) {
                Ok(profile) => profile,
                Err(err) => {
                    eprintln!(
                        "warning: skipping bound-action scan of '{}': {err}",
                        knot.id
                    );
                    continue;
                }
            };
            if profile.is_action_state(&knot.state) {
                bound_actions.push(knot);
            }
        }
        if bound_actions.len() > 1 {
            return Err(AppError::InvalidArgument(format!(
                "lease '{}' is bound to multiple action-state knots",
                lease_id
            )));
        }
        if let Some(knot) = bound_actions.first() {
            self.recover_bound_action_locked(knot, &lease)?;
        } else if lease.state != workflow_runtime::LEASE_TERMINATED {
            let actor = StateActorMetadata::default();
            self.write_state_change_locked(
                &lease,
                workflow_runtime::LEASE_TERMINATED,
                true,
                None,
                &actor,
                None,
            )?;
        }

        let updated =
            db::get_knot_hot(&self.conn, &lease_id)?.ok_or_else(|| AppError::NotFound(lease_id))?;
        Ok(self.apply_alias_and_enrich_knot_post_write(KnotView::from(updated)))
    }

    fn recover_bound_action_locked(
        &self,
        knot: &db::KnotCacheRecord,
        lease: &db::KnotCacheRecord,
    ) -> Result<bool, AppError> {
        let Some(bound_token) = knot.lease_id.as_deref() else {
            return Ok(false);
        };
        let profile = self.resolve_profile_for_record(knot)?;
        if parse_knot_type(knot.knot_type.as_deref()) == crate::domain::knot_type::KnotType::Lease
            || self.resolve_knot_token(bound_token)? != lease.id
            || !profile.is_action_state(&knot.state)
        {
            return Ok(false);
        }
        let resolution = crate::rollback::resolve_rollback_state(self, &knot.id)?;
        let actor = StateActorMetadata::default();
        self.write_state_change_locked_with(
            knot,
            &resolution.target_state,
            resolution.requires_force,
            None,
            &actor,
            None,
            StateWriteOverrides {
                clear_lease: true,
                fail_step: true,
            },
        )?;
        if lease.state != workflow_runtime::LEASE_TERMINATED {
            self.write_state_change_locked(
                lease,
                workflow_runtime::LEASE_TERMINATED,
                true,
                None,
                &actor,
                None,
            )?;
        }
        Ok(true)
    }

    /// Terminate a lease knot without re-acquiring the repo/cache locks.
    fn release_lease_knot_locked(&self, lease_id: &str) -> Result<(), AppError> {
        // A bound `lease_id` may be a stripped id, so resolve it the same way
        // every other lease lookup does before hitting the cache.
        let resolved = self.resolve_knot_token(lease_id)?;
        let Some(record) = db::get_knot_hot(&self.conn, &resolved)? else {
            return Ok(());
        };
        if record.state == workflow_runtime::LEASE_TERMINATED {
            return Ok(());
        }
        let view = KnotView::from(record.clone());
        let actor = StateActorMetadata::default();
        if crate::lease::is_mcp_session_lease(&view) {
            self.write_state_change_locked(
                &record,
                workflow_runtime::LEASE_READY,
                true,
                None,
                &actor,
                None,
            )?;
            let timeout = view
                .lease
                .as_ref()
                .and_then(|data| data.timeout_seconds)
                .unwrap_or(DEFAULT_LEASE_TIMEOUT_SECONDS);
            self.set_lease_expiry(&resolved, compute_expiry_ts(timeout))?;
            return Ok(());
        }
        self.write_state_change_locked(
            &record,
            workflow_runtime::LEASE_TERMINATED,
            true,
            None,
            &actor,
            None,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::step_history::StepActorInfo;

    fn started(step: &str, from_state: &str) -> StepRecord {
        StepRecord::new_started(
            step,
            "shipment_phase",
            from_state,
            "2026-07-26T00:00:00Z",
            &StepActorInfo::default(),
        )
    }

    #[test]
    fn failure_transition_closes_active_step_as_failed() {
        let history = vec![started("shipment", "ready_for_shipment")];
        let updated = apply_step_failure_transition(
            &history,
            "shipment",
            "ready_for_implementation",
            "2026-07-26T01:00:00Z",
            &StateActorMetadata::default(),
            None,
        );
        assert_eq!(updated.len(), 1, "failure must not open a new step");
        assert_eq!(updated[0].status, StepStatus::Failed);
        assert_eq!(updated[0].from_state, "ready_for_shipment");
        assert_eq!(
            updated[0].to_state.as_deref(),
            Some("ready_for_implementation")
        );
        assert_eq!(updated[0].ended_at.as_deref(), Some("2026-07-26T01:00:00Z"));
    }

    #[test]
    fn failure_transition_is_a_noop_for_self_transitions() {
        let history = vec![started("shipment", "ready_for_shipment")];
        let updated = apply_step_failure_transition(
            &history,
            "shipment",
            "shipment",
            "2026-07-26T01:00:00Z",
            &StateActorMetadata::default(),
            None,
        );
        assert_eq!(updated, history);
    }

    #[test]
    fn failure_transition_fills_missing_identity_from_the_lease() {
        let history = vec![started("shipment", "ready_for_shipment")];
        let actor = StateActorMetadata {
            actor_kind: Some("agent".to_string()),
            agent_name: Some("shipper".to_string()),
            agent_model: Some("opus".to_string()),
            agent_version: Some("5".to_string()),
        };
        let updated = apply_step_failure_transition(
            &history,
            "shipment",
            "ready_for_implementation",
            "2026-07-26T01:00:00Z",
            &actor,
            Some("lease-1"),
        );
        assert_eq!(updated[0].agent_name.as_deref(), Some("shipper"));
        assert_eq!(updated[0].agent_model.as_deref(), Some("opus"));
        assert_eq!(updated[0].agent_version.as_deref(), Some("5"));
        assert_eq!(updated[0].lease_id.as_deref(), Some("lease-1"));
    }

    #[test]
    fn failure_transition_keeps_identity_already_on_the_record() {
        let mut record = started("shipment", "ready_for_shipment");
        record.agent_name = Some("original".to_string());
        record.lease_id = Some("lease-original".to_string());
        let actor = StateActorMetadata {
            agent_name: Some("later".to_string()),
            ..StateActorMetadata::default()
        };
        let updated = apply_step_failure_transition(
            &[record],
            "shipment",
            "ready_for_implementation",
            "2026-07-26T01:00:00Z",
            &actor,
            Some("lease-later"),
        );
        assert_eq!(updated[0].agent_name.as_deref(), Some("original"));
        assert_eq!(updated[0].lease_id.as_deref(), Some("lease-original"));
    }

    #[test]
    fn failure_transition_leaves_closed_steps_untouched() {
        let mut closed = started("implementation", "ready_for_implementation");
        closed.status = StepStatus::Completed;
        closed.to_state = Some("ready_for_implementation_review".to_string());
        let history = vec![closed.clone(), started("shipment", "ready_for_shipment")];
        let updated = apply_step_failure_transition(
            &history,
            "shipment",
            "ready_for_implementation",
            "2026-07-26T01:00:00Z",
            &StateActorMetadata::default(),
            None,
        );
        assert_eq!(updated[0], closed);
        assert_eq!(updated[1].status, StepStatus::Failed);
    }
}

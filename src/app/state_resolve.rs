use std::collections::HashSet;

use crate::db::{self, KnotCacheRecord};
use crate::state_hierarchy;

use super::error::AppError;
use super::types::StateActorMetadata;
use super::App;

impl App {
    pub(crate) fn reconcile_terminal_parent_state_locked(
        &self,
        current: &KnotCacheRecord,
        next_state: &str,
    ) -> Result<KnotCacheRecord, AppError> {
        self.write_state_change_locked(
            current,
            next_state,
            true,
            None,
            &StateActorMetadata::default(),
            None,
        )
    }

    pub(crate) fn auto_resolve_terminal_parents_locked<'a>(
        &self,
        knot_ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), AppError> {
        let mut pending = knot_ids
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<String>>();
        let mut seen = HashSet::new();
        while let Some(knot_id) = pending.pop() {
            let resolutions =
                state_hierarchy::find_ancestor_terminal_resolutions(&self.conn, &knot_id)?;
            for resolution in resolutions {
                if !seen.insert(resolution.parent.id.clone()) {
                    continue;
                }
                let Some(parent) = db::get_knot_hot(&self.conn, &resolution.parent.id)? else {
                    continue;
                };
                if !state_hierarchy::is_terminal_resolution_state(&parent.state)? {
                    self.reconcile_terminal_parent_state_locked(&parent, &resolution.target_state)?;
                    pending.push(parent.id);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn resume_blocked_dependents_locked(
        &self,
        blocker_id: &str,
        state_actor: &StateActorMetadata,
    ) -> Result<(), AppError> {
        let blocked_edges = db::list_edges_by_kind(&self.conn, "blocked_by")?;
        let mut dependents = blocked_edges
            .iter()
            .filter(|edge| edge.dst == blocker_id)
            .map(|edge| edge.src.clone())
            .collect::<Vec<_>>();
        dependents.sort();
        dependents.dedup();
        for dependent_id in dependents {
            self.try_resume_blocked_dependent(&dependent_id, &blocked_edges, state_actor)?;
        }
        Ok(())
    }

    fn try_resume_blocked_dependent(
        &self,
        dependent_id: &str,
        blocked_edges: &[crate::db::EdgeRecord],
        state_actor: &StateActorMetadata,
    ) -> Result<(), AppError> {
        let Some(record) = db::get_knot_hot(&self.conn, dependent_id)? else {
            return Ok(());
        };
        if record.state != "blocked" {
            return Ok(());
        }
        let all_shipped = blocked_edges
            .iter()
            .filter(|edge| edge.src == dependent_id)
            .all(|edge| {
                db::get_knot_hot(&self.conn, &edge.dst)
                    .ok()
                    .flatten()
                    .is_some_and(|k| k.state == "shipped")
            });
        if !all_shipped {
            return Ok(());
        }
        let Some(target) = record.blocked_from_state.as_deref() else {
            return Err(AppError::InvalidArgument(format!(
                "blocked knot '{}' is missing \
                 blocked_from_state provenance",
                record.id
            )));
        };
        self.write_state_change_locked(&record, target, true, None, state_actor, None)?;
        Ok(())
    }

    pub(crate) fn transitioned_to_terminal_resolution_state(
        &self,
        current: &KnotCacheRecord,
        updated: &KnotCacheRecord,
    ) -> Result<bool, AppError> {
        Ok(
            !state_hierarchy::is_terminal_resolution_state(&current.state)?
                && state_hierarchy::is_terminal_resolution_state(&updated.state)?,
        )
    }
}

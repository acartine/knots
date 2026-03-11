use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use rusqlite::Connection;

use crate::app::AppError;
use crate::db::{self, KnotCacheRecord};
use crate::domain::state::KnotState;

pub const HIERARCHY_PROGRESS_BLOCKED_CODE: &str = "hierarchy_progress_blocked";
pub const TERMINAL_CASCADE_APPROVAL_REQUIRED_CODE: &str = "terminal_cascade_approval_required";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyKnot {
    pub id: String,
    pub state: String,
    pub deferred_from_state: Option<String>,
}

impl HierarchyKnot {
    pub fn from_record(record: &KnotCacheRecord) -> Self {
        Self {
            id: record.id.clone(),
            state: record.state.clone(),
            deferred_from_state: record.deferred_from_state.clone(),
        }
    }

    pub fn display_state(&self) -> String {
        match self.deferred_from_state.as_deref() {
            Some(from) if self.state == "deferred" => format!("deferred from {from}"),
            _ => self.state.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionPlan {
    Allowed,
    CascadeTerminal { descendants: Vec<HierarchyKnot> },
}

pub fn plan_state_transition(
    conn: &Connection,
    knot: &KnotCacheRecord,
    target_state: &str,
    target_is_terminal: bool,
    approve_terminal_cascade: bool,
) -> Result<TransitionPlan, AppError> {
    if knot.state == target_state {
        return Ok(TransitionPlan::Allowed);
    }

    let child_graph = load_child_graph(conn)?;

    if target_is_terminal {
        let descendants = collect_descendants(&child_graph, conn, &knot.id)?;
        if descendants.is_empty() {
            return Ok(TransitionPlan::Allowed);
        }
        if approve_terminal_cascade {
            return Ok(TransitionPlan::CascadeTerminal { descendants });
        }
        return Err(AppError::TerminalCascadeApprovalRequired {
            knot_id: knot.id.clone(),
            target_state: target_state.to_string(),
            descendants,
        });
    }

    let target_rank = effective_target_rank(knot, target_state)?;
    let blockers = direct_children(&child_graph, conn, &knot.id)?
        .into_iter()
        .filter(|child| effective_record_rank(child).is_ok_and(|rank| rank < target_rank))
        .map(|child| HierarchyKnot::from_record(&child))
        .collect::<Vec<_>>();

    if blockers.is_empty() {
        Ok(TransitionPlan::Allowed)
    } else {
        Err(AppError::HierarchyProgressBlocked {
            knot_id: knot.id.clone(),
            target_state: target_state.to_string(),
            blockers,
        })
    }
}

pub fn format_hierarchy_knots(knots: &[HierarchyKnot]) -> String {
    knots
        .iter()
        .map(|knot| format!("{} [{}]", knot.id, knot.display_state()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn load_child_graph(conn: &Connection) -> Result<HashMap<String, Vec<String>>, AppError> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for edge in db::list_edges_by_kind(conn, "parent_of")? {
        graph.entry(edge.src).or_default().push(edge.dst);
    }
    Ok(graph)
}

fn direct_children(
    child_graph: &HashMap<String, Vec<String>>,
    conn: &Connection,
    knot_id: &str,
) -> Result<Vec<KnotCacheRecord>, AppError> {
    let mut children = Vec::new();
    for child_id in child_graph.get(knot_id).into_iter().flatten() {
        if let Some(child) = db::get_knot_hot(conn, child_id)? {
            children.push(child);
        }
    }
    Ok(children)
}

fn collect_descendants(
    child_graph: &HashMap<String, Vec<String>>,
    conn: &Connection,
    root_id: &str,
) -> Result<Vec<HierarchyKnot>, AppError> {
    let mut depths = HashMap::new();
    let mut path = HashSet::from([root_id.to_string()]);
    collect_descendant_depths(child_graph, root_id, 1, &mut path, &mut depths);

    let mut descendants = Vec::new();
    for (id, depth) in depths {
        if let Some(record) = db::get_knot_hot(conn, &id)? {
            descendants.push((depth, HierarchyKnot::from_record(&record)));
        }
    }
    descendants.sort_by(|(left_depth, left), (right_depth, right)| {
        right_depth
            .cmp(left_depth)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(descendants
        .into_iter()
        .map(|(_, knot)| knot)
        .collect::<Vec<_>>())
}

fn collect_descendant_depths(
    child_graph: &HashMap<String, Vec<String>>,
    node_id: &str,
    depth: usize,
    path: &mut HashSet<String>,
    depths: &mut HashMap<String, usize>,
) {
    let Some(children) = child_graph.get(node_id) else {
        return;
    };

    for child_id in children {
        if path.contains(child_id) {
            continue;
        }
        depths
            .entry(child_id.clone())
            .and_modify(|existing| *existing = (*existing).max(depth))
            .or_insert(depth);
        path.insert(child_id.clone());
        collect_descendant_depths(child_graph, child_id, depth + 1, path, depths);
        path.remove(child_id);
    }
}

fn effective_target_rank(knot: &KnotCacheRecord, target_state: &str) -> Result<u8, AppError> {
    if target_state == "deferred" {
        return if knot.state == "deferred" {
            effective_state_rank(knot.deferred_from_state.as_deref().unwrap_or("deferred"))
        } else {
            effective_state_rank(&knot.state)
        };
    }
    effective_state_rank(target_state)
}

fn effective_record_rank(knot: &KnotCacheRecord) -> Result<u8, AppError> {
    if knot.state == "deferred" {
        effective_state_rank(knot.deferred_from_state.as_deref().unwrap_or("deferred"))
    } else {
        effective_state_rank(&knot.state)
    }
}

fn effective_state_rank(state: &str) -> Result<u8, AppError> {
    let state = KnotState::from_str(state)?;
    let rank = match state {
        KnotState::ReadyForPlanning => 0,
        KnotState::Planning => 1,
        KnotState::ReadyForPlanReview => 2,
        KnotState::PlanReview => 3,
        KnotState::ReadyForImplementation => 4,
        KnotState::Implementation => 5,
        KnotState::ReadyForImplementationReview => 6,
        KnotState::ImplementationReview => 7,
        KnotState::ReadyForShipment => 8,
        KnotState::Shipment => 9,
        KnotState::ReadyForShipmentReview => 10,
        KnotState::ShipmentReview => 11,
        KnotState::ReadyToEvaluate => 12,
        KnotState::Evaluating => 13,
        KnotState::Shipped | KnotState::Abandoned => 14,
        KnotState::Deferred => 255,
    };
    Ok(rank)
}

pub fn is_terminal_state(state: &str) -> Result<bool, AppError> {
    Ok(KnotState::from_str(state)?.is_terminal())
}

#[cfg(test)]
#[path = "state_hierarchy_tests.rs"]
mod tests;

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
        KnotState::Shipped | KnotState::Abandoned => 12,
        KnotState::Deferred => 255,
    };
    Ok(rank)
}

pub fn is_terminal_state(state: &str) -> Result<bool, AppError> {
    Ok(KnotState::from_str(state)?.is_terminal())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::db::KnotCacheRecord;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    fn unique_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("knots-state-hierarchy-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("temp workspace should be creatable");
        root
    }

    fn open_app(root: &Path) -> App {
        let db = root.join(".knots/cache/state.sqlite");
        App::open(
            db.to_str().expect("db path should be utf8"),
            root.to_path_buf(),
        )
        .expect("app should open")
    }

    fn sample_record(id: &str, state: &str, deferred_from_state: Option<&str>) -> KnotCacheRecord {
        KnotCacheRecord {
            id: id.to_string(),
            title: id.to_string(),
            state: state.to_string(),
            updated_at: "2026-03-10T00:00:00Z".to_string(),
            body: None,
            description: None,
            priority: None,
            knot_type: None,
            tags: Vec::new(),
            notes: Vec::new(),
            handoff_capsules: Vec::new(),
            invariants: Vec::new(),
            step_history: Vec::new(),
            profile_id: "default".to_string(),
            profile_etag: None,
            deferred_from_state: deferred_from_state.map(ToString::to_string),
            created_at: None,
        }
    }

    #[test]
    fn hierarchy_knot_formats_deferred_state_with_provenance() {
        let knot = HierarchyKnot::from_record(&sample_record(
            "knots-child",
            "deferred",
            Some("implementation"),
        ));
        assert_eq!(knot.display_state(), "deferred from implementation");
    }

    #[test]
    fn target_rank_uses_current_progress_when_deferring() {
        let knot = sample_record("knots-parent", "implementation_review", None);
        let rank = effective_target_rank(&knot, "deferred").expect("rank should resolve");
        assert_eq!(rank, 7);
    }

    #[test]
    fn record_rank_uses_deferred_from_state() {
        let knot = sample_record("knots-child", "deferred", Some("plan_review"));
        let rank = effective_record_rank(&knot).expect("rank should resolve");
        assert_eq!(rank, 3);
    }

    #[test]
    fn terminal_state_helper_matches_terminal_states() {
        assert!(is_terminal_state("shipped").expect("shipped should parse"));
        assert!(is_terminal_state("abandoned").expect("abandoned should parse"));
        assert!(!is_terminal_state("implementation").expect("implementation should parse"));
    }

    #[test]
    fn format_hierarchy_knots_lists_each_knot_and_display_state() {
        let rendered = format_hierarchy_knots(&[
            HierarchyKnot::from_record(&sample_record("knots-a", "planning", None)),
            HierarchyKnot::from_record(&sample_record(
                "knots-b",
                "deferred",
                Some("implementation"),
            )),
        ]);
        assert!(rendered.contains("knots-a [planning]"));
        assert!(rendered.contains("knots-b [deferred from implementation]"));
    }

    #[test]
    fn plan_state_transition_blocks_direct_children_that_are_behind() {
        let root = unique_workspace();
        let app = open_app(&root);
        let db = root.join(".knots/cache/state.sqlite");
        let parent = app
            .create_knot("Parent", None, Some("planning"), Some("default"))
            .expect("parent should be created");
        let child = app
            .create_knot("Child", None, Some("planning"), Some("default"))
            .expect("child should be created");
        app.add_edge(&parent.id, "parent_of", &child.id)
            .expect("edge should be added");
        let conn =
            crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
        let parent = crate::db::get_knot_hot(&conn, &parent.id)
            .expect("db lookup should succeed")
            .expect("parent should exist");

        let err = plan_state_transition(&conn, &parent, "ready_for_plan_review", false, false)
            .expect_err("direct child should block parent");
        assert!(matches!(err, AppError::HierarchyProgressBlocked { .. }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plan_state_transition_returns_sorted_descendants_for_terminal_cascade() {
        let root = unique_workspace();
        let app = open_app(&root);
        let db = root.join(".knots/cache/state.sqlite");
        let parent = app
            .create_knot("Parent", None, Some("implementation"), Some("default"))
            .expect("parent should be created");
        let child = app
            .create_knot("Child", None, Some("planning"), Some("default"))
            .expect("child should be created");
        let grandchild = app
            .create_knot("Grandchild", None, Some("idea"), Some("default"))
            .expect("grandchild should be created");
        app.add_edge(&parent.id, "parent_of", &child.id)
            .expect("edge should be added");
        app.add_edge(&child.id, "parent_of", &grandchild.id)
            .expect("edge should be added");
        let conn =
            crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
        let parent = crate::db::get_knot_hot(&conn, &parent.id)
            .expect("db lookup should succeed")
            .expect("parent should exist");

        let plan = plan_state_transition(&conn, &parent, "abandoned", true, true)
            .expect("approved terminal cascade should plan");
        match plan {
            TransitionPlan::CascadeTerminal { descendants } => {
                let ids = descendants
                    .iter()
                    .map(|knot| knot.id.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(ids, vec![grandchild.id.as_str(), child.id.as_str()]);
            }
            other => panic!("unexpected plan: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_op_transition_is_allowed() {
        let root = unique_workspace();
        let app = open_app(&root);
        let knot = app
            .create_knot("Parent", None, Some("planning"), Some("default"))
            .expect("knot should be created");
        let db = root.join(".knots/cache/state.sqlite");
        let conn =
            crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
        let knot = crate::db::get_knot_hot(&conn, &knot.id)
            .expect("db lookup should succeed")
            .expect("knot should exist");

        let plan =
            plan_state_transition(&conn, &knot, "planning", false, false).expect("plan works");
        assert!(matches!(plan, TransitionPlan::Allowed));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn terminal_plan_without_descendants_is_allowed() {
        let root = unique_workspace();
        let app = open_app(&root);
        let parent = app
            .create_knot("Solo", None, Some("implementation"), Some("default"))
            .expect("parent should be created");
        let db = root.join(".knots/cache/state.sqlite");
        let conn =
            crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
        let parent = crate::db::get_knot_hot(&conn, &parent.id)
            .expect("db lookup should succeed")
            .expect("parent should exist");

        let plan =
            plan_state_transition(&conn, &parent, "abandoned", true, false).expect("plan works");
        assert!(matches!(plan, TransitionPlan::Allowed));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn terminal_plan_requires_approval_when_descendants_exist() {
        let root = unique_workspace();
        let app = open_app(&root);
        let parent = app
            .create_knot("Parent", None, Some("implementation"), Some("default"))
            .expect("parent should be created");
        let child = app
            .create_knot("Child", None, Some("planning"), Some("default"))
            .expect("child should be created");
        app.add_edge(&parent.id, "parent_of", &child.id)
            .expect("edge should be added");
        let db = root.join(".knots/cache/state.sqlite");
        let conn =
            crate::db::open_connection(db.to_str().expect("db path should be utf8")).expect("db");
        let parent = crate::db::get_knot_hot(&conn, &parent.id)
            .expect("db lookup should succeed")
            .expect("parent should exist");

        let err = plan_state_transition(&conn, &parent, "abandoned", true, false)
            .expect_err("approval should be required");
        match err {
            AppError::TerminalCascadeApprovalRequired { descendants, .. } => {
                assert_eq!(descendants.len(), 1);
                assert_eq!(descendants[0].id, child.id);
            }
            other => panic!("unexpected error: {other}"),
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn collect_descendant_depths_skips_cycles_and_keeps_deepest_path() {
        let child_graph = HashMap::from([
            (
                "root".to_string(),
                vec!["child".to_string(), "middle".to_string()],
            ),
            ("middle".to_string(), vec!["child".to_string()]),
            ("child".to_string(), vec!["root".to_string()]),
        ]);
        let mut path = HashSet::from(["root".to_string()]);
        let mut depths = HashMap::new();

        collect_descendant_depths(&child_graph, "root", 1, &mut path, &mut depths);

        assert_eq!(depths.get("child"), Some(&2));
        assert_eq!(depths.get("middle"), Some(&1));
        assert!(!depths.contains_key("root"));
    }

    #[test]
    fn deferred_target_without_provenance_uses_deferred_rank() {
        let knot = sample_record("knots-child", "deferred", None);
        let rank = effective_target_rank(&knot, "deferred").expect("rank should resolve");
        assert_eq!(rank, 255);
    }

    #[test]
    fn effective_state_rank_covers_remaining_shipment_and_terminal_states() {
        assert_eq!(
            effective_state_rank("ready_for_shipment").expect("state should parse"),
            8
        );
        assert_eq!(
            effective_state_rank("shipment").expect("state should parse"),
            9
        );
        assert_eq!(
            effective_state_rank("ready_for_shipment_review").expect("state should parse"),
            10
        );
        assert_eq!(
            effective_state_rank("shipment_review").expect("state should parse"),
            11
        );
        assert_eq!(
            effective_state_rank("shipped").expect("state should parse"),
            12
        );
        assert_eq!(
            effective_state_rank("abandoned").expect("state should parse"),
            12
        );
        assert_eq!(
            effective_state_rank("deferred").expect("state should parse"),
            255
        );
    }
}

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::app::{EdgeView, KnotView};

#[derive(Debug, Clone)]
pub struct DisplayKnot {
    pub knot: KnotView,
    pub depth: usize,
}

type ChildrenByParent = HashMap<String, Vec<String>>;
type ChildIds = HashSet<String>;
type BlockersByKnot = HashMap<String, Vec<String>>;

pub fn layout_knots(knots: Vec<KnotView>, edges: &[EdgeView]) -> Vec<DisplayKnot> {
    let by_id: HashMap<String, KnotView> = knots
        .into_iter()
        .map(|knot| (knot.id.clone(), knot))
        .collect();
    if by_id.is_empty() {
        return Vec::new();
    }

    let (mut children_by_parent, child_ids, blockers_by_knot) = build_layout_maps(edges, &by_id);

    for children in children_by_parent.values_mut() {
        children.sort_by(|left, right| compare_knot_id(left, right, &by_id, &blockers_by_knot));
        children.dedup();
    }

    let mut roots: Vec<String> = by_id
        .keys()
        .filter(|id| !child_ids.contains(*id))
        .cloned()
        .collect();
    roots.sort_by(|left, right| compare_knot_id(left, right, &by_id, &blockers_by_knot));

    let mut visited: HashSet<String> = HashSet::new();
    let mut ordered = Vec::new();

    for root in roots {
        append_component_post_order(
            &root,
            &by_id,
            &children_by_parent,
            &mut visited,
            &mut ordered,
        );
    }

    let mut remaining: Vec<String> = by_id
        .keys()
        .filter(|id| !visited.contains(*id))
        .cloned()
        .collect();
    remaining.sort_by(|left, right| compare_knot_id(left, right, &by_id, &blockers_by_knot));
    for id in remaining {
        append_component_post_order(&id, &by_id, &children_by_parent, &mut visited, &mut ordered);
    }

    ordered
}

fn append_component_post_order(
    root_id: &str,
    by_id: &HashMap<String, KnotView>,
    children_by_parent: &ChildrenByParent,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<DisplayKnot>,
) {
    if visited.contains(root_id) {
        return;
    }

    let mut visiting = HashSet::new();
    let mut component_rows: Vec<(String, usize)> = Vec::new();
    collect_component_post_order(
        root_id,
        0,
        by_id,
        children_by_parent,
        visited,
        &mut visiting,
        &mut component_rows,
    );

    let max_depth = component_rows
        .iter()
        .map(|(_, depth)| *depth)
        .max()
        .unwrap_or(0);

    for (id, source_depth) in component_rows {
        let Some(knot) = by_id.get(&id) else {
            continue;
        };
        ordered.push(DisplayKnot {
            knot: knot.clone(),
            depth: max_depth.saturating_sub(source_depth),
        });
    }
}

fn collect_component_post_order(
    id: &str,
    depth: usize,
    by_id: &HashMap<String, KnotView>,
    children_by_parent: &ChildrenByParent,
    visited: &mut HashSet<String>,
    visiting: &mut HashSet<String>,
    rows: &mut Vec<(String, usize)>,
) {
    if visited.contains(id) || !by_id.contains_key(id) {
        return;
    }
    if !visiting.insert(id.to_string()) {
        return;
    }

    if let Some(children) = children_by_parent.get(id) {
        for child in children {
            collect_component_post_order(
                child,
                depth + 1,
                by_id,
                children_by_parent,
                visited,
                visiting,
                rows,
            );
        }
    }

    visiting.remove(id);
    if visited.insert(id.to_string()) {
        rows.push((id.to_string(), depth));
    }
}

fn build_layout_maps(
    edges: &[EdgeView],
    by_id: &HashMap<String, KnotView>,
) -> (ChildrenByParent, ChildIds, BlockersByKnot) {
    let mut children_by_parent: ChildrenByParent = HashMap::new();
    let mut child_ids: ChildIds = HashSet::new();
    let mut blockers_by_knot: BlockersByKnot = HashMap::new();

    for edge in edges {
        if edge.src == edge.dst {
            continue;
        }

        if edge.kind.eq_ignore_ascii_case("parent_of") {
            if by_id.contains_key(&edge.src) && by_id.contains_key(&edge.dst) {
                children_by_parent
                    .entry(edge.src.clone())
                    .or_default()
                    .push(edge.dst.clone());
                child_ids.insert(edge.dst.clone());
            }
            continue;
        }

        if edge.kind.eq_ignore_ascii_case("blocked_by") {
            if by_id.contains_key(&edge.src) && by_id.contains_key(&edge.dst) {
                blockers_by_knot
                    .entry(edge.src.clone())
                    .or_default()
                    .push(edge.dst.clone());
            }
            continue;
        }

        if edge.kind.eq_ignore_ascii_case("blocks")
            && by_id.contains_key(&edge.src)
            && by_id.contains_key(&edge.dst)
        {
            blockers_by_knot
                .entry(edge.dst.clone())
                .or_default()
                .push(edge.src.clone());
        }
    }

    for blockers in blockers_by_knot.values_mut() {
        blockers.sort();
        blockers.dedup();
    }

    (children_by_parent, child_ids, blockers_by_knot)
}

fn compare_knot_id(
    left: &str,
    right: &str,
    by_id: &HashMap<String, KnotView>,
    blockers_by_knot: &BlockersByKnot,
) -> Ordering {
    let Some(left_knot) = by_id.get(left) else {
        return left.cmp(right);
    };
    let Some(right_knot) = by_id.get(right) else {
        return left.cmp(right);
    };
    compare_knot(left_knot, right_knot, by_id, blockers_by_knot)
}

fn compare_knot(
    left: &KnotView,
    right: &KnotView,
    by_id: &HashMap<String, KnotView>,
    blockers_by_knot: &BlockersByKnot,
) -> Ordering {
    let left_rank = readiness_rank(left, by_id, blockers_by_knot);
    let right_rank = readiness_rank(right, by_id, blockers_by_knot);

    left_rank
        .cmp(&right_rank)
        .then_with(|| compare_sequence(left, right))
        .then_with(|| state_rank(&left.state).cmp(&state_rank(&right.state)))
        .then_with(|| left.priority.unwrap_or(9).cmp(&right.priority.unwrap_or(9)))
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.title.cmp(&right.title))
        .then_with(|| left.id.cmp(&right.id))
}

fn readiness_rank(
    knot: &KnotView,
    by_id: &HashMap<String, KnotView>,
    blockers_by_knot: &BlockersByKnot,
) -> usize {
    if is_terminal_state(&knot.state) {
        return 2;
    }
    if has_unresolved_blocker(&knot.id, by_id, blockers_by_knot) {
        return 1;
    }
    0
}

fn has_unresolved_blocker(
    knot_id: &str,
    by_id: &HashMap<String, KnotView>,
    blockers_by_knot: &BlockersByKnot,
) -> bool {
    let Some(blockers) = blockers_by_knot.get(knot_id) else {
        return false;
    };

    blockers.iter().any(|blocker_id| {
        by_id
            .get(blocker_id)
            .map(|blocker| !is_terminal_state(&blocker.state))
            .unwrap_or(true)
    })
}

fn is_terminal_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "done" | "closed" | "shipped" | "deferred" | "abandoned"
    )
}

fn compare_sequence(left: &KnotView, right: &KnotView) -> Ordering {
    let left_key = sequence_key(left);
    let right_key = sequence_key(right);

    match (left_key, right_key) {
        (Some(left_key), Some(right_key)) => compare_sequence_key(&left_key, &right_key),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_sequence_key(left: &SequenceKey, right: &SequenceKey) -> Ordering {
    let prefix_cmp = left.prefix.cmp(&right.prefix);
    if prefix_cmp != Ordering::Equal {
        return prefix_cmp;
    }

    let min_len = left.segments.len().min(right.segments.len());
    for index in 0..min_len {
        let segment_cmp = left.segments[index].cmp(&right.segments[index]);
        if segment_cmp != Ordering::Equal {
            return segment_cmp;
        }
    }

    right.segments.len().cmp(&left.segments.len())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SequenceKey {
    prefix: String,
    segments: Vec<u64>,
}

fn sequence_key(knot: &KnotView) -> Option<SequenceKey> {
    extract_sequence_key(&knot.id).or_else(|| extract_sequence_key(&knot.title))
}

fn extract_sequence_key(input: &str) -> Option<SequenceKey> {
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if !bytes[index].is_ascii_alphanumeric() {
            index += 1;
            continue;
        }

        let prefix_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'.' {
            continue;
        }

        let prefix = input[prefix_start..index].to_ascii_lowercase();
        let mut cursor = index;
        let mut segments = Vec::new();

        while cursor < bytes.len() && bytes[cursor] == b'.' {
            cursor += 1;
            let segment_start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if segment_start == cursor {
                segments.clear();
                break;
            }

            let segment = input[segment_start..cursor].parse::<u64>().ok()?;
            segments.push(segment);
        }

        if !segments.is_empty() {
            return Some(SequenceKey { prefix, segments });
        }
    }

    None
}

fn state_rank(state: &str) -> usize {
    match state.trim().to_ascii_lowercase().as_str() {
        "implementing" => 0,
        "reviewing" => 1,
        "work_item" => 2,
        "idea" => 3,
        "refining" => 4,
        "blocked" => 5,
        "approved" => 6,
        "done" | "closed" => 7,
        "shipped" => 8,
        "deferred" => 9,
        "abandoned" => 10,
        _ => 11,
    }
}

#[cfg(test)]
mod tests {
    use super::layout_knots;
    use crate::app::{EdgeView, KnotView};

    fn knot(id: &str, state: &str) -> KnotView {
        KnotView {
            id: id.to_string(),
            title: id.to_string(),
            state: state.to_string(),
            updated_at: "2026-02-24T10:00:00Z".to_string(),
            body: None,
            description: None,
            priority: None,
            knot_type: None,
            tags: Vec::new(),
            notes: Vec::new(),
            handoff_capsules: Vec::new(),
            workflow_etag: None,
            created_at: None,
        }
    }

    #[test]
    fn renders_children_before_parent_footer() {
        let knots = vec![knot("K-1", "work_item"), knot("K-2", "work_item")];
        let edges = vec![EdgeView {
            src: "K-1".to_string(),
            kind: "parent_of".to_string(),
            dst: "K-2".to_string(),
        }];

        let rows = layout_knots(knots, &edges);
        assert_eq!(rows[0].knot.id, "K-2");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].knot.id, "K-1");
        assert_eq!(rows[1].depth, 1);
    }

    #[test]
    fn sequence_order_is_child_specific_then_parent() {
        let knots = vec![
            knot("knots-q3e.5", "blocked"),
            knot("knots-q3e.5.3", "work_item"),
            knot("knots-q3e.5.2", "work_item"),
            knot("knots-q3e.5.1", "work_item"),
        ];

        let rows = layout_knots(knots, &[]);
        assert_eq!(rows[0].knot.id, "knots-q3e.5.1");
        assert_eq!(rows[1].knot.id, "knots-q3e.5.2");
        assert_eq!(rows[2].knot.id, "knots-q3e.5.3");
        assert_eq!(rows[3].knot.id, "knots-q3e.5");
    }

    #[test]
    fn blocked_items_sort_after_actionable_peers() {
        let knots = vec![
            knot("knots-q3e.5", "work_item"),
            knot("knots-q3e.5.3", "work_item"),
            knot("knots-q3e.5.2", "work_item"),
            knot("knots-q3e.5.1", "work_item"),
        ];
        let edges = vec![EdgeView {
            src: "knots-q3e.5".to_string(),
            kind: "blocked_by".to_string(),
            dst: "knots-q3e.5.3".to_string(),
        }];

        let rows = layout_knots(knots, &edges);
        assert_eq!(rows[3].knot.id, "knots-q3e.5");
    }

    #[test]
    fn nested_epic_footer_depth_increases_by_level() {
        let knots = vec![
            knot("knots-q3e", "work_item"),
            knot("knots-q3e.5", "work_item"),
            knot("knots-q3e.5.1", "work_item"),
        ];
        let edges = vec![
            EdgeView {
                src: "knots-q3e".to_string(),
                kind: "parent_of".to_string(),
                dst: "knots-q3e.5".to_string(),
            },
            EdgeView {
                src: "knots-q3e.5".to_string(),
                kind: "parent_of".to_string(),
                dst: "knots-q3e.5.1".to_string(),
            },
        ];

        let rows = layout_knots(knots, &edges);
        assert_eq!(rows[0].knot.id, "knots-q3e.5.1");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].knot.id, "knots-q3e.5");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].knot.id, "knots-q3e");
        assert_eq!(rows[2].depth, 2);
    }

    #[test]
    fn handles_cycles_without_infinite_loop() {
        let knots = vec![knot("K-1", "work_item"), knot("K-2", "work_item")];
        let edges = vec![
            EdgeView {
                src: "K-1".to_string(),
                kind: "parent_of".to_string(),
                dst: "K-2".to_string(),
            },
            EdgeView {
                src: "K-2".to_string(),
                kind: "parent_of".to_string(),
                dst: "K-1".to_string(),
            },
        ];

        let rows = layout_knots(knots, &edges);
        assert_eq!(rows.len(), 2);
    }
}

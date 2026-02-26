use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AliasMaps {
    pub id_to_alias: HashMap<String, String>,
    pub alias_to_id: HashMap<String, String>,
}

pub fn build_alias_maps(ids: Vec<String>, parent_edges: &[(String, String)]) -> AliasMaps {
    let mut ids = ids;
    ids.sort();
    ids.dedup();
    let id_set: HashSet<String> = ids.iter().cloned().collect();

    let mut canonical_parent: HashMap<String, String> = HashMap::new();
    for (parent, child) in parent_edges {
        if parent == child {
            continue;
        }
        if !id_set.contains(parent) || !id_set.contains(child) {
            continue;
        }
        canonical_parent
            .entry(child.clone())
            .and_modify(|current| {
                if parent < current {
                    *current = parent.clone();
                }
            })
            .or_insert_with(|| parent.clone());
    }

    let mut children_by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for (child, parent) in &canonical_parent {
        children_by_parent
            .entry(parent.clone())
            .or_default()
            .push(child.clone());
    }

    for children in children_by_parent.values_mut() {
        children.sort();
    }

    let mut roots = ids
        .iter()
        .filter(|id| !canonical_parent.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    roots.sort();

    let mut maps = AliasMaps::default();
    let mut visited = HashSet::new();
    for root in &roots {
        assign_alias(root, root, &children_by_parent, &mut visited, &mut maps);
    }

    for id in ids {
        if !visited.contains(&id) {
            assign_alias(&id, &id, &children_by_parent, &mut visited, &mut maps);
        }
    }

    maps
}

fn assign_alias(
    id: &str,
    alias: &str,
    children_by_parent: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    maps: &mut AliasMaps,
) {
    if !visited.insert(id.to_string()) {
        return;
    }

    maps.id_to_alias.insert(id.to_string(), alias.to_string());
    maps.alias_to_id.insert(alias.to_string(), id.to_string());

    let children = children_by_parent.get(id).cloned().unwrap_or_default();
    for (idx, child) in children.iter().enumerate() {
        let child_alias = format!("{}.{}", alias, idx + 1);
        assign_alias(child, &child_alias, children_by_parent, visited, maps);
    }
}

#[cfg(test)]
mod tests {
    use super::build_alias_maps;

    #[test]
    fn assigns_hierarchical_aliases_for_parent_chain() {
        let ids = vec![
            "abc-1111".to_string(),
            "def-2222".to_string(),
            "ghi-3333".to_string(),
        ];
        let edges = vec![
            ("abc-1111".to_string(), "def-2222".to_string()),
            ("def-2222".to_string(), "ghi-3333".to_string()),
        ];

        let maps = build_alias_maps(ids, &edges);
        assert_eq!(
            maps.id_to_alias.get("abc-1111").map(String::as_str),
            Some("abc-1111")
        );
        assert_eq!(
            maps.id_to_alias.get("def-2222").map(String::as_str),
            Some("abc-1111.1")
        );
        assert_eq!(
            maps.id_to_alias.get("ghi-3333").map(String::as_str),
            Some("abc-1111.1.1")
        );
        assert_eq!(
            maps.alias_to_id.get("abc-1111.1").map(String::as_str),
            Some("def-2222")
        );
    }

    #[test]
    fn self_edges_and_unknown_ids_are_skipped() {
        let ids = vec!["a-1".to_string(), "b-2".to_string()];
        let edges = vec![
            ("a-1".to_string(), "a-1".to_string()),  // self-edge → skipped
            ("x-9".to_string(), "b-2".to_string()),  // unknown parent → skipped
            ("a-1".to_string(), "z-99".to_string()), // unknown child → skipped
        ];
        let maps = build_alias_maps(ids, &edges);
        assert_eq!(maps.id_to_alias.len(), 2);
        // Both should be roots (no valid parent edges survived)
        assert_eq!(maps.id_to_alias.get("a-1").map(String::as_str), Some("a-1"));
        assert_eq!(maps.id_to_alias.get("b-2").map(String::as_str), Some("b-2"));
    }

    #[test]
    fn cycle_edges_do_not_cause_infinite_loop() {
        let ids = vec!["a-1".to_string(), "b-2".to_string()];
        // Mutual parent edges — only the lex-smallest parent wins
        let edges = vec![
            ("a-1".to_string(), "b-2".to_string()),
            ("b-2".to_string(), "a-1".to_string()),
        ];
        let maps = build_alias_maps(ids, &edges);
        assert_eq!(maps.id_to_alias.len(), 2);
    }

    #[test]
    fn picks_lexicographically_smallest_parent_for_alias_path() {
        let ids = vec![
            "a-1111".to_string(),
            "b-2222".to_string(),
            "c-3333".to_string(),
        ];
        let edges = vec![
            ("b-2222".to_string(), "c-3333".to_string()),
            ("a-1111".to_string(), "c-3333".to_string()),
        ];

        let maps = build_alias_maps(ids, &edges);
        assert_eq!(
            maps.id_to_alias.get("c-3333").map(String::as_str),
            Some("a-1111.1")
        );
    }
}

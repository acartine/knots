use crate::app::{KnotView, PaginatedList};
use crate::listing;

pub fn compute_paginated_list(
    knots: Vec<KnotView>,
    filter: &listing::KnotListFilter,
    offset: usize,
    limit: usize,
) -> PaginatedList<KnotView> {
    let filtered = listing::apply_filters(knots, filter);
    let total = filtered.len() as i64;
    let page: Vec<_> = filtered.into_iter().skip(offset).take(limit).collect();
    PaginatedList::new(page, total, offset, limit)
}

#[cfg(test)]
mod tests {
    use super::compute_paginated_list;
    use crate::app::KnotView;
    use crate::domain::knot_type::KnotType;
    use crate::listing::KnotListFilter;

    fn knot_view(id: &str, title: &str, description: Option<&str>) -> KnotView {
        KnotView {
            id: id.to_string(),
            alias: None,
            title: title.to_string(),
            state: "implementation".to_string(),
            updated_at: "2026-02-23T10:00:00Z".to_string(),
            body: None,
            description: description.map(|d| d.to_string()),
            acceptance: None,
            priority: None,
            knot_type: KnotType::Work,
            tags: Vec::new(),
            notes: Vec::new(),
            handoff_capsules: Vec::new(),
            invariants: Vec::new(),
            step_history: Vec::new(),
            gate: None,
            lease: None,
            execution_plan: None,
            lease_id: None,
            lease_expiry_ts: 0,
            lease_agent: None,
            workflow_id: "work_sdlc".to_string(),
            profile_id: "autopilot".to_string(),
            profile_etag: None,
            deferred_from_state: None,
            blocked_from_state: None,
            created_at: None,
            step_metadata: None,
            next_step_metadata: None,
            edges: Vec::new(),
            child_summaries: Vec::new(),
        }
    }

    fn matching_query_filter() -> KnotListFilter {
        KnotListFilter {
            include_all: true,
            state: None,
            knot_type: None,
            profile_id: None,
            tags: Vec::new(),
            query: Some("escalation".to_string()),
        }
    }

    #[test]
    fn empty_match_set_reports_zero_total_and_no_more() {
        let knots: Vec<KnotView> = (0..50)
            .map(|i| {
                knot_view(
                    &format!("K-{i}"),
                    &format!("Routine knot {i}"),
                    Some("ordinary description"),
                )
            })
            .collect();

        let page = compute_paginated_list(knots, &matching_query_filter(), 0, 50);
        assert!(
            page.data.is_empty(),
            "expected empty data for non-matching query"
        );
        assert_eq!(page.total, 0, "total must reflect filtered match count");
        assert!(!page.has_more, "has_more must be false when no rows match");
        assert_eq!(page.offset, 0);
        assert_eq!(page.limit, 50);
    }

    #[test]
    fn multi_page_match_set_preserves_totals_and_has_more() {
        let make_knots = || -> Vec<KnotView> {
            (0..120)
                .map(|i| {
                    knot_view(
                        &format!("K-{i}"),
                        &format!("escalation flow {i}"),
                        Some("matches the query"),
                    )
                })
                .collect()
        };

        let page = compute_paginated_list(make_knots(), &matching_query_filter(), 0, 50);
        assert_eq!(page.data.len(), 50);
        assert_eq!(page.total, 120);
        assert!(page.has_more);

        let last_page = compute_paginated_list(make_knots(), &matching_query_filter(), 100, 50);
        assert_eq!(last_page.data.len(), 20);
        assert_eq!(last_page.total, 120);
        assert!(!last_page.has_more);
    }
}

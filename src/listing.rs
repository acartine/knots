use crate::app::KnotView;
use crate::workflow::normalize_workflow_id;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnotListFilter {
    pub include_all: bool,
    pub state: Option<String>,
    pub knot_type: Option<String>,
    pub workflow_id: Option<String>,
    pub tags: Vec<String>,
    pub query: Option<String>,
}

pub fn apply_filters(knots: Vec<KnotView>, filter: &KnotListFilter) -> Vec<KnotView> {
    let normalized = NormalizedFilter::from(filter);
    if normalized.has_no_user_filters() && normalized.include_all {
        return knots;
    }

    knots
        .into_iter()
        .filter(|knot| matches_filter(knot, &normalized))
        .collect()
}

#[derive(Debug, Clone, Default)]
struct NormalizedFilter {
    include_all: bool,
    state: Option<String>,
    knot_type: Option<String>,
    workflow_id: Option<String>,
    tags: Vec<String>,
    query: Option<String>,
}

impl NormalizedFilter {
    fn has_no_user_filters(&self) -> bool {
        self.state.is_none()
            && self.knot_type.is_none()
            && self.workflow_id.is_none()
            && self.tags.is_empty()
            && self.query.is_none()
    }
}

impl From<&KnotListFilter> for NormalizedFilter {
    fn from(value: &KnotListFilter) -> Self {
        Self {
            include_all: value.include_all,
            state: normalize_scalar(value.state.as_deref()),
            knot_type: normalize_scalar(value.knot_type.as_deref()),
            workflow_id: value.workflow_id.as_deref().and_then(normalize_workflow_id),
            tags: value
                .tags
                .iter()
                .filter_map(|tag| normalize_scalar(Some(tag)))
                .collect(),
            query: normalize_scalar(value.query.as_deref()),
        }
    }
}

fn matches_filter(knot: &KnotView, filter: &NormalizedFilter) -> bool {
    if should_hide_shipped(knot, filter) {
        return false;
    }

    if let Some(expected_state) = filter.state.as_deref() {
        let actual_state = knot.state.to_ascii_lowercase();
        if actual_state != expected_state {
            return false;
        }
    }

    if let Some(expected_type) = filter.knot_type.as_deref() {
        let actual_type = knot.knot_type.as_deref().unwrap_or("").to_ascii_lowercase();
        if actual_type != expected_type {
            return false;
        }
    }

    if let Some(expected_workflow) = filter.workflow_id.as_deref() {
        if knot.workflow_id.to_ascii_lowercase() != expected_workflow {
            return false;
        }
    }

    if !has_all_tags(knot, &filter.tags) {
        return false;
    }

    if let Some(query) = filter.query.as_deref() {
        return matches_query(knot, query);
    }

    true
}

fn should_hide_shipped(knot: &KnotView, filter: &NormalizedFilter) -> bool {
    if filter.include_all {
        return false;
    }
    if filter.state.as_deref() == Some("shipped") {
        return false;
    }
    knot.state.trim().eq_ignore_ascii_case("shipped")
}

fn has_all_tags(knot: &KnotView, required_tags: &[String]) -> bool {
    if required_tags.is_empty() {
        return true;
    }
    let knot_tags: Vec<String> = knot
        .tags
        .iter()
        .map(|tag| tag.trim().to_ascii_lowercase())
        .collect();
    required_tags
        .iter()
        .all(|tag| knot_tags.iter().any(|existing| existing == tag))
}

fn matches_query(knot: &KnotView, query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    let alias = knot.alias.as_deref().unwrap_or("").to_ascii_lowercase();
    let description = knot
        .description
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let body = knot.body.as_deref().unwrap_or("").to_ascii_lowercase();

    knot.id.to_ascii_lowercase().contains(&query)
        || alias.contains(&query)
        || knot.title.to_ascii_lowercase().contains(&query)
        || description.contains(&query)
        || body.contains(&query)
        || knot.workflow_id.to_ascii_lowercase().contains(&query)
}

fn normalize_scalar(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_filters, KnotListFilter};
    use crate::app::KnotView;

    fn knot(
        id: &str,
        title: &str,
        state: &str,
        knot_type: Option<&str>,
        tags: &[&str],
        description: Option<&str>,
    ) -> KnotView {
        KnotView {
            id: id.to_string(),
            alias: None,
            title: title.to_string(),
            state: state.to_string(),
            updated_at: "2026-02-23T10:00:00Z".to_string(),
            body: None,
            description: description.map(|value| value.to_string()),
            priority: None,
            knot_type: knot_type.map(|value| value.to_string()),
            tags: tags.iter().map(|value| (*value).to_string()).collect(),
            notes: Vec::new(),
            handoff_capsules: Vec::new(),
            workflow_id: "default".to_string(),
            workflow_etag: None,
            created_at: None,
        }
    }

    #[test]
    fn filters_by_state_case_insensitive() {
        let knots = vec![
            knot("K-1", "Plan filters", "idea", Some("task"), &["ux"], None),
            knot(
                "K-2",
                "Ship UI",
                "implementing",
                Some("task"),
                &["release"],
                None,
            ),
        ];
        let filter = KnotListFilter {
            include_all: false,
            state: Some("ImPlementing".to_string()),
            knot_type: None,
            workflow_id: None,
            tags: Vec::new(),
            query: None,
        };

        let filtered = apply_filters(knots, &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "K-2");
    }

    #[test]
    fn filters_by_multiple_tags() {
        let knots = vec![
            knot(
                "K-1",
                "Importer",
                "work_item",
                Some("task"),
                &["migration", "sync"],
                None,
            ),
            knot(
                "K-2",
                "Docs",
                "work_item",
                Some("task"),
                &["migration"],
                None,
            ),
        ];
        let filter = KnotListFilter {
            include_all: false,
            state: None,
            knot_type: None,
            workflow_id: None,
            tags: vec!["migration".to_string(), "sync".to_string()],
            query: None,
        };

        let filtered = apply_filters(knots, &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "K-1");
    }

    #[test]
    fn filters_by_query_across_title_and_description() {
        let knots = vec![
            knot(
                "K-1",
                "Polish ls output",
                "reviewing",
                Some("task"),
                &["ux"],
                Some("needs style"),
            ),
            knot(
                "K-2",
                "Refactor imports",
                "implementing",
                Some("task"),
                &["infra"],
                Some("carry checkpoint"),
            ),
        ];
        let filter = KnotListFilter {
            include_all: false,
            state: None,
            knot_type: None,
            workflow_id: None,
            tags: Vec::new(),
            query: Some("STYLE".to_string()),
        };

        let filtered = apply_filters(knots, &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "K-1");
    }

    #[test]
    fn combines_filters() {
        let knots = vec![
            knot(
                "K-1",
                "Release flow",
                "implementing",
                Some("task"),
                &["release", "cli"],
                None,
            ),
            knot(
                "K-2",
                "Release docs",
                "implementing",
                Some("doc"),
                &["release", "docs"],
                None,
            ),
        ];
        let filter = KnotListFilter {
            include_all: false,
            state: Some("implementing".to_string()),
            knot_type: Some("task".to_string()),
            workflow_id: None,
            tags: vec!["release".to_string()],
            query: Some("flow".to_string()),
        };

        let filtered = apply_filters(knots, &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "K-1");
    }

    #[test]
    fn excludes_shipped_by_default() {
        let knots = vec![
            knot("K-1", "Active", "implementing", Some("task"), &[], None),
            knot("K-2", "Done", "shipped", Some("task"), &[], None),
        ];
        let filter = KnotListFilter::default();

        let filtered = apply_filters(knots, &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "K-1");
    }

    #[test]
    fn includes_shipped_with_all_flag() {
        let knots = vec![
            knot("K-1", "Active", "implementing", Some("task"), &[], None),
            knot("K-2", "Done", "shipped", Some("task"), &[], None),
        ];
        let filter = KnotListFilter {
            include_all: true,
            state: None,
            knot_type: None,
            workflow_id: None,
            tags: Vec::new(),
            query: None,
        };

        let filtered = apply_filters(knots, &filter);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn allows_state_shipped_without_all_flag() {
        let knots = vec![
            knot("K-1", "Active", "implementing", Some("task"), &[], None),
            knot("K-2", "Done", "shipped", Some("task"), &[], None),
        ];
        let filter = KnotListFilter {
            include_all: false,
            state: Some("shipped".to_string()),
            knot_type: None,
            workflow_id: None,
            tags: Vec::new(),
            query: None,
        };

        let filtered = apply_filters(knots, &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "K-2");
    }

    #[test]
    fn filters_by_alias_query() {
        let mut with_alias = knot(
            "repo-a1b2",
            "Alias Target",
            "work_item",
            Some("task"),
            &[],
            None,
        );
        with_alias.alias = Some("repo-root.1".to_string());
        let without_alias = knot("repo-c3d4", "Other", "work_item", Some("task"), &[], None);
        let filter = KnotListFilter {
            include_all: false,
            state: None,
            knot_type: None,
            workflow_id: None,
            tags: Vec::new(),
            query: Some("root.1".to_string()),
        };

        let filtered = apply_filters(vec![with_alias, without_alias], &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "repo-a1b2");
    }

    #[test]
    fn filters_by_workflow_id() {
        let mut triage = knot("K-1", "Triage item", "work_item", Some("task"), &[], None);
        triage.workflow_id = "triage".to_string();
        let default = knot("K-2", "Default item", "work_item", Some("task"), &[], None);
        let filter = KnotListFilter {
            include_all: false,
            state: None,
            knot_type: None,
            workflow_id: Some("triage".to_string()),
            tags: Vec::new(),
            query: None,
        };

        let filtered = apply_filters(vec![triage, default], &filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "K-1");
    }
}

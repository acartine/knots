use super::{filter_summary, format_show_fields, knot_show_fields, Palette, ShowField};
use crate::app::KnotView;
use crate::domain::metadata::MetadataEntry;
use crate::listing::KnotListFilter;
#[test]
fn filter_summary_formats_only_active_filters() {
    let f = KnotListFilter {
        include_all: false,
        state: Some("implementing".into()),
        knot_type: Some("task".into()),
        profile_id: Some("default".into()),
        tags: vec!["release".into(), "".into()],
        query: Some("sync".into()),
    };
    assert_eq!(
        filter_summary(&f).expect("s"),
        "state=implementing type=task profile=default tags=release query=sync"
    );
}
#[test]
fn filter_summary_is_none_for_empty() {
    assert!(filter_summary(&KnotListFilter::default()).is_none());
}
#[test]
fn filter_summary_includes_all_flag() {
    let f = KnotListFilter {
        include_all: true,
        state: None,
        knot_type: None,
        profile_id: None,
        tags: Vec::new(),
        query: None,
    };
    assert_eq!(filter_summary(&f).expect("s"), "all=true");
}
#[test]
fn show_fields_right_align_labels() {
    let f = vec![
        ShowField::new("id", "knot-123"),
        ShowField::new("profile_id", "default"),
    ];
    let p = Palette { enabled: false };
    let l = format_show_fields(&f, &p, 80);
    assert_eq!(l[0], "        id:  knot-123");
    assert_eq!(l[1], "profile_id:  default");
}
#[test]
fn show_fields_wrap_values() {
    let v = format!("{} {}", "a".repeat(40), "b".repeat(50));
    let l = format_show_fields(
        &[ShowField::new("body", v)],
        &Palette { enabled: false },
        20,
    );
    assert_eq!(l.len(), 5);
    assert_eq!(l[0], "body:  aaaaaaaaaaaaaaaaaaaa");
}
fn make_entry(id: &str, c: &str) -> MetadataEntry {
    MetadataEntry {
        entry_id: id.into(),
        content: c.into(),
        username: "u".into(),
        datetime: "2026-02-25T10:00:00Z".into(),
        agentname: "a".into(),
        model: "m".into(),
        version: "v".into(),
    }
}
fn minimal_knot() -> KnotView {
    KnotView {
        id: "K-1".into(),
        alias: None,
        title: "T".into(),
        state: "implementing".into(),
        updated_at: "2026-02-25T10:00:00Z".into(),
        body: None,
        description: None,
        acceptance: None,
        priority: None,
        knot_type: crate::domain::knot_type::KnotType::default(),
        tags: vec![],
        notes: vec![],
        handoff_capsules: vec![],
        invariants: vec![],
        step_history: vec![],
        gate: None,
        lease: None,
        lease_id: None,
        lease_agent: None,
        workflow_id: "compatibility".into(),
        profile_id: "default".into(),
        profile_etag: None,
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: None,
        step_metadata: None,
        next_step_metadata: None,
        edges: vec![],
        child_summaries: vec![],
    }
}
#[test]
fn knot_show_fields_include_optional_sections() {
    let k = KnotView {
        id: "knot-123".into(),
        alias: Some("alpha".into()),
        title: "Fix".into(),
        state: "implementing".into(),
        updated_at: "2026-02-25T15:00:00Z".into(),
        body: Some("Body".into()),
        description: Some("Desc".into()),
        acceptance: None,
        priority: Some(2),
        knot_type: crate::domain::knot_type::KnotType::Work,
        tags: vec!["cli".into()],
        notes: vec![MetadataEntry {
            entry_id: "n1".into(),
            content: "note".into(),
            username: "t".into(),
            datetime: "2026-02-25T15:00:00Z".into(),
            agentname: "codex".into(),
            model: "gpt-5".into(),
            version: "1".into(),
        }],
        handoff_capsules: vec![MetadataEntry {
            entry_id: "h1".into(),
            content: "handoff".into(),
            username: "t".into(),
            datetime: "2026-02-25T15:00:00Z".into(),
            agentname: "codex".into(),
            model: "gpt-5".into(),
            version: "1".into(),
        }],
        invariants: vec![],
        step_history: vec![],
        gate: None,
        lease: None,
        lease_id: None,
        lease_agent: None,
        workflow_id: "compatibility".into(),
        profile_id: "default".into(),
        profile_etag: Some("etag-1".into()),
        deferred_from_state: None,
        blocked_from_state: None,
        created_at: Some("2026-02-25T14:00:00Z".into()),
        step_metadata: None,
        next_step_metadata: None,
        edges: vec![],
        child_summaries: vec![],
    };
    let labels = knot_show_fields(&k, false)
        .iter()
        .map(|f| f.label.clone())
        .collect::<Vec<_>>();
    for l in [
        "alias",
        "body",
        "description",
        "priority",
        "type",
        "tags",
        "note",
        "handoff_capsule",
    ] {
        assert!(labels.iter().any(|x| x == l), "missing {l}");
    }
}
#[test]
fn hidden_hint_empty_single() {
    let mut k = minimal_knot();
    k.notes = vec![make_entry("n1", "x")];
    k.handoff_capsules = vec![make_entry("h1", "x")];
    assert_eq!(super::hidden_metadata_hint(&k), "");
}
#[test]
fn hidden_hint_multiple() {
    let mut k = minimal_knot();
    k.notes = vec![
        make_entry("n1", "a"),
        make_entry("n2", "b"),
        make_entry("n3", "c"),
    ];
    k.handoff_capsules = vec![make_entry("h1", "a"), make_entry("h2", "b")];
    let h = super::hidden_metadata_hint(&k);
    assert!(h.contains("2 older notes"));
    assert!(h.contains("1 older handoff capsule"));
    assert!(h.contains("-v/--verbose"));
}
#[test]
fn verbose_all() {
    let mut k = minimal_knot();
    k.notes = vec![make_entry("n1", "a"), make_entry("n2", "b")];
    assert_eq!(
        knot_show_fields(&k, true)
            .iter()
            .filter(|f| f.label == "note")
            .count(),
        2
    );
}
#[test]
fn non_verbose_latest() {
    let mut k = minimal_knot();
    k.notes = vec![make_entry("n1", "a"), make_entry("n2", "b")];
    let nf: Vec<_> = knot_show_fields(&k, false)
        .into_iter()
        .filter(|f| f.label == "note")
        .collect();
    assert_eq!(nf.len(), 1);
    assert!(nf[0].value.contains("b"));
}
#[test]
fn entry_inline_agentname() {
    assert!(super::format_entry_inline(&make_entry("e1", "c")).contains("[a 2026-02-25]"));
}
#[test]
fn entry_inline_username() {
    let mut e = make_entry("e1", "c");
    e.agentname = "unknown".into();
    assert!(super::format_entry_inline(&e).contains("[u 2026-02-25]"));
}
#[test]
fn trim_json_adds_other() {
    let mut k = minimal_knot();
    k.notes = vec![make_entry("n1", "a"), make_entry("n2", "b")];
    let mut v = serde_json::to_value(&k).unwrap();
    super::trim_json_metadata(&mut v, &k);
    assert_eq!(v["notes"].as_array().unwrap().len(), 1);
    assert!(v["other"].as_str().unwrap().contains("1 older note"));
}
#[test]
fn trim_json_no_other() {
    let mut k = minimal_knot();
    k.notes = vec![make_entry("n1", "x")];
    let mut v = serde_json::to_value(&k).unwrap();
    super::trim_json_metadata(&mut v, &k);
    assert!(v.get("other").is_none());
}
#[test]
fn show_hint_hidden() {
    let mut k = minimal_knot();
    k.notes = vec![make_entry("n1", "a"), make_entry("n2", "b")];
    let j = super::format_knot_show(&k, &Palette { enabled: false }, 80, false).join("\n");
    assert!(j.contains("1 older note"));
    assert!(j.contains("-v/--verbose"));
}
#[test]
fn show_verbose_no_hint() {
    let mut k = minimal_knot();
    k.notes = vec![make_entry("n1", "a"), make_entry("n2", "b")];
    let l = super::format_knot_show(&k, &Palette { enabled: false }, 80, true);
    assert!(!l.join("\n").contains("not shown"));
    assert_eq!(l.iter().filter(|x| x.contains("note:")).count(), 2);
}
#[test]
fn edges_grouped() {
    use crate::app::EdgeView;
    let mut k = minimal_knot();
    k.edges = vec![
        EdgeView {
            src: "K-1".into(),
            kind: "parent_of".into(),
            dst: "knots-abc1".into(),
        },
        EdgeView {
            src: "K-1".into(),
            kind: "parent_of".into(),
            dst: "knots-abc2".into(),
        },
        EdgeView {
            src: "K-1".into(),
            kind: "blocked_by".into(),
            dst: "knots-xyz1".into(),
        },
        EdgeView {
            src: "knots-other".into(),
            kind: "blocks".into(),
            dst: "K-1".into(),
        },
    ];
    let f = knot_show_fields(&k, false);
    let l: Vec<&str> = f.iter().map(|x| x.label.as_str()).collect();
    assert!(l.contains(&"blocked_by"));
    assert!(l.contains(&"parent_of"));
    assert!(l.contains(&"blocks (incoming)"));
    let pf = f.iter().find(|x| x.label == "parent_of").unwrap();
    assert!(pf.value.contains("abc1"));
    assert!(pf.value.contains("abc2"));
    assert!(f
        .iter()
        .find(|x| x.label == "blocks (incoming)")
        .unwrap()
        .value
        .contains("other"));
}
#[test]
fn no_edges_when_empty() {
    assert!(!knot_show_fields(&minimal_knot(), false)
        .iter()
        .any(|f| f.label == "parent_of" || f.label == "blocked_by" || f.label == "blocks"));
}
#[test]
fn lease_agent_fields_shown_when_present() {
    use crate::app::types::LeaseAgentView;
    let mut k = minimal_knot();
    k.lease_agent = Some(LeaseAgentView {
        lease_type: "agent".into(),
        nickname: "claude-bot".into(),
        agent_name: Some("claude".into()),
        agent_model: Some("opus".into()),
        agent_provider: Some("Anthropic".into()),
    });
    let fields = knot_show_fields(&k, false);
    let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
    assert!(labels.contains(&"lease_agent"));
    assert!(labels.contains(&"lease_type"));
    assert!(labels.contains(&"agent_name"));
    assert!(labels.contains(&"agent_model"));
    assert!(labels.contains(&"agent_provider"));
    let la = fields.iter().find(|f| f.label == "lease_agent").unwrap();
    assert_eq!(la.value, "claude-bot");
}
#[test]
fn lease_agent_fields_hidden_when_absent() {
    let k = minimal_knot();
    let fields = knot_show_fields(&k, false);
    let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
    assert!(!labels.contains(&"lease_agent"));
    assert!(!labels.contains(&"lease_type"));
    assert!(!labels.contains(&"agent_name"));
}
#[test]
fn lease_id_not_in_json() {
    let mut k = minimal_knot();
    k.lease_id = Some("knots-secret-lease".into());
    let v = serde_json::to_value(&k).unwrap();
    assert!(v.get("lease_id").is_none());
}
#[test]
fn lease_agent_in_json_when_present() {
    use crate::app::types::LeaseAgentView;
    let mut k = minimal_knot();
    k.lease_agent = Some(LeaseAgentView {
        lease_type: "agent".into(),
        nickname: "claude-bot".into(),
        agent_name: Some("claude".into()),
        agent_model: Some("opus".into()),
        agent_provider: Some("Anthropic".into()),
    });
    let v = serde_json::to_value(&k).unwrap();
    let la = v.get("lease_agent").expect("lease_agent present");
    assert_eq!(la["nickname"].as_str().unwrap(), "claude-bot");
    assert_eq!(la["lease_type"].as_str().unwrap(), "agent");
    assert_eq!(la["agent_name"].as_str().unwrap(), "claude");
    assert_eq!(la["agent_model"].as_str().unwrap(), "opus");
    assert_eq!(la["agent_provider"].as_str().unwrap(), "Anthropic");
    assert!(v.get("lease_id").is_none());
}
#[test]
fn lease_agent_not_in_json_when_absent() {
    let k = minimal_knot();
    let v = serde_json::to_value(&k).unwrap();
    assert!(v.get("lease_agent").is_none());
}
#[test]
fn json_text_parity_for_lease_agent() {
    use crate::app::types::LeaseAgentView;
    let mut k = minimal_knot();
    k.lease_agent = Some(LeaseAgentView {
        lease_type: "agent".into(),
        nickname: "bot-1".into(),
        agent_name: Some("claude".into()),
        agent_model: None,
        agent_provider: None,
    });
    let fields = knot_show_fields(&k, false);
    let text_labels: Vec<&str> = fields
        .iter()
        .filter(|f| f.label.starts_with("lease_") || f.label.starts_with("agent_"))
        .map(|f| f.label.as_str())
        .collect();
    let json = serde_json::to_value(&k).unwrap();
    let la = json.get("lease_agent").unwrap();
    assert_eq!(la["nickname"].as_str().unwrap(), "bot-1");
    assert_eq!(la["lease_type"].as_str().unwrap(), "agent");
    assert_eq!(la["agent_name"].as_str().unwrap(), "claude");
    assert!(la.get("agent_model").is_none());
    assert!(la.get("agent_provider").is_none());
    assert!(text_labels.contains(&"lease_agent"));
    assert!(text_labels.contains(&"lease_type"));
    assert!(text_labels.contains(&"agent_name"));
    assert!(!text_labels.contains(&"agent_model"));
    assert!(!text_labels.contains(&"agent_provider"));
}

use std::str::FromStr;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::domain::state::KnotState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTier {
    Hot,
    Warm,
    Cold,
}

pub fn classify_knot_tier(
    state: &str,
    updated_at: &str,
    hot_window_days: i64,
    now: OffsetDateTime,
) -> CacheTier {
    let parsed = KnotState::from_str(state);
    if parsed.map(|value| value.is_terminal()).unwrap_or(false) {
        return CacheTier::Cold;
    }

    let Ok(updated) = OffsetDateTime::parse(updated_at, &Rfc3339) else {
        return CacheTier::Warm;
    };

    let window_days = hot_window_days.max(0);
    let hot_cutoff = now - Duration::days(window_days);
    if updated >= hot_cutoff {
        CacheTier::Hot
    } else {
        CacheTier::Warm
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_knot_tier, CacheTier};
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    #[test]
    fn terminal_state_is_always_cold() {
        let now =
            OffsetDateTime::parse("2026-02-24T12:00:00Z", &Rfc3339).expect("now should parse");
        let tier = classify_knot_tier("shipped", "2026-02-24T11:00:00Z", 7, now);
        assert_eq!(tier, CacheTier::Cold);
    }

    #[test]
    fn recent_non_terminal_is_hot() {
        let now =
            OffsetDateTime::parse("2026-02-24T12:00:00Z", &Rfc3339).expect("now should parse");
        let tier = classify_knot_tier("implementing", "2026-02-23T11:00:00Z", 7, now);
        assert_eq!(tier, CacheTier::Hot);
    }

    #[test]
    fn old_non_terminal_is_warm() {
        let now =
            OffsetDateTime::parse("2026-02-24T12:00:00Z", &Rfc3339).expect("now should parse");
        let tier = classify_knot_tier("work_item", "2025-12-01T00:00:00Z", 7, now);
        assert_eq!(tier, CacheTier::Warm);
    }

    #[test]
    fn unparseable_date_falls_back_to_warm() {
        let now =
            OffsetDateTime::parse("2026-02-24T12:00:00Z", &Rfc3339).expect("now should parse");
        let tier = classify_knot_tier("implementing", "not-a-date", 7, now);
        assert_eq!(tier, CacheTier::Warm);
    }
}

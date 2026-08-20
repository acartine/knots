#[cfg(not(tarpaulin_include))]
use std::time::Duration;

#[cfg(not(tarpaulin_include))]
use crate::runner::KnoRunner;
use serde_json::Value;

#[cfg(not(tarpaulin_include))]
pub fn spawn_background_sync(runner: KnoRunner, interval: Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        loop {
            tick.tick().await;
            match runner.run("sync", &[]) {
                Ok(value) => {
                    if let Some(detail) = held_back_sync_detail(&value) {
                        eprintln!("kno-mcp sync completed; {detail}");
                    }
                }
                Err(err) => eprintln!("kno-mcp sync retry pending: {}", err.stderr),
            }
        }
    });
}

/// Describe a sync that held back some knots. Sync no longer defers
/// wholesale on an active lease -- it filters per knot, so this only
/// surfaces the knots this machine is mid-action on and skipped for now.
fn held_back_sync_detail(value: &Value) -> Option<String> {
    let held_back = value
        .get("pull")
        .and_then(|pull| pull.get("held_back_knots"))
        .and_then(Value::as_array)?;
    if held_back.is_empty() {
        return None;
    }
    let ids: Vec<&str> = held_back.iter().filter_map(Value::as_str).collect();
    Some(format!(
        "{} knot(s) held back (locally leased): {}",
        ids.len(),
        ids.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn held_back_sync_detail_reports_the_held_back_knot_ids() {
        let detail = held_back_sync_detail(&json!({
            "status": "completed",
            "push": { "copied_files": 0, "pushed": false },
            "pull": { "held_back_knots": ["K-1", "K-2"] }
        }));
        assert_eq!(
            detail,
            Some("2 knot(s) held back (locally leased): K-1, K-2".to_string())
        );
    }

    #[test]
    fn held_back_sync_detail_is_none_when_nothing_was_held_back() {
        assert_eq!(
            held_back_sync_detail(&json!({
                "status": "completed",
                "pull": { "held_back_knots": [] }
            })),
            None
        );
    }

    #[test]
    fn held_back_sync_detail_covers_missing_fields() {
        assert_eq!(
            held_back_sync_detail(&json!({ "status": "completed" })),
            None
        );
        assert_eq!(held_back_sync_detail(&json!({})), None);
    }
}

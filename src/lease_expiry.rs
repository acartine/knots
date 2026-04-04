use crate::workflow_runtime;

/// Default lease timeout: 10 minutes.
pub const DEFAULT_LEASE_TIMEOUT_SECONDS: u64 = 600;

/// Compute an absolute expiry timestamp from the current time.
pub fn compute_expiry_ts(timeout_seconds: u64) -> i64 {
    now_unix() + timeout_seconds as i64
}

/// Return the effective lease state, accounting for expiry.
///
/// If the raw state is `lease_ready` or `lease_active` but the expiry
/// timestamp has passed, the effective state is `lease_terminated`.
/// An expiry of 0 means the lease was never given a timeout (legacy or
/// migration default) and is also treated as expired.
pub fn effective_lease_state(raw_state: &str, expiry_ts: i64) -> &'static str {
    match raw_state {
        "lease_ready" | "lease_active" if now_unix() >= expiry_ts => {
            workflow_runtime::LEASE_TERMINATED
        }
        "lease_ready" => workflow_runtime::LEASE_READY,
        "lease_active" => workflow_runtime::LEASE_ACTIVE,
        "lease_terminated" => workflow_runtime::LEASE_TERMINATED,
        _ => workflow_runtime::LEASE_TERMINATED,
    }
}

/// Check whether a lease has expired based on its expiry timestamp.
/// An expiry of 0 is treated as expired (legacy/migration default).
#[allow(dead_code)]
pub fn is_lease_expired(expiry_ts: i64) -> bool {
    now_unix() >= expiry_ts
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_lease_with_future_expiry_stays_active() {
        let ts = now_unix() + 600;
        assert_eq!(effective_lease_state("lease_active", ts), "lease_active");
        assert!(!is_lease_expired(ts));
    }

    #[test]
    fn active_lease_with_past_expiry_becomes_terminated() {
        let ts = now_unix() - 10;
        assert_eq!(
            effective_lease_state("lease_active", ts),
            "lease_terminated"
        );
        assert!(is_lease_expired(ts));
    }

    #[test]
    fn ready_lease_with_past_expiry_becomes_terminated() {
        let ts = now_unix() - 10;
        assert_eq!(effective_lease_state("lease_ready", ts), "lease_terminated");
    }

    #[test]
    fn terminated_lease_stays_terminated() {
        let ts = now_unix() + 600;
        assert_eq!(
            effective_lease_state("lease_terminated", ts),
            "lease_terminated"
        );
    }

    #[test]
    fn zero_expiry_with_active_state_is_treated_as_expired() {
        assert_eq!(effective_lease_state("lease_active", 0), "lease_terminated");
        assert!(is_lease_expired(0));
    }

    #[test]
    fn unknown_state_is_treated_as_terminated() {
        assert_eq!(
            effective_lease_state("garbage", now_unix() + 600),
            "lease_terminated"
        );
    }
}

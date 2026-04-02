use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static ACTIVE_TRACE: RefCell<Option<TraceState>> = const { RefCell::new(None) };
}

struct TraceState {
    cmd: String,
    args: Vec<String>,
    start: Instant,
    phases: Vec<TracePhase>,
}

struct TracePhase {
    name: String,
    elapsed: Duration,
    detail: Option<String>,
}

pub struct TraceSession {
    enabled: bool,
}

pub struct TracePhaseGuard {
    enabled: bool,
    name: String,
    start: Instant,
    detail: Option<String>,
}

impl TraceSession {
    pub fn start(cmd: &str, args: &[String], enabled: bool) -> Self {
        if enabled {
            ACTIVE_TRACE.with(|slot| {
                *slot.borrow_mut() = Some(TraceState {
                    cmd: cmd.to_string(),
                    args: args.to_vec(),
                    start: Instant::now(),
                    phases: Vec::new(),
                });
            });
        }
        Self { enabled }
    }
}

impl Drop for TraceSession {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        ACTIVE_TRACE.with(|slot| {
            let Some(state) = slot.borrow_mut().take() else {
                return;
            };
            let total_ms = state.start.elapsed().as_millis();
            let args = if state.args.is_empty() {
                String::from("[]")
            } else {
                format!("[{}]", state.args.join(", "))
            };
            eprintln!("[kno] cmd={} args={} total={}ms", state.cmd, args, total_ms);
            for phase in state.phases {
                match phase.detail {
                    Some(detail) => {
                        eprintln!(
                            "  {}={}ms({})",
                            phase.name,
                            phase.elapsed.as_millis(),
                            detail
                        );
                    }
                    None => {
                        eprintln!("  {}={}ms", phase.name, phase.elapsed.as_millis());
                    }
                }
            }
        });
    }
}

impl TracePhaseGuard {
    pub fn detail(&mut self, detail: impl Into<String>) {
        self.detail = Some(detail.into());
    }
}

impl Drop for TracePhaseGuard {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        record_phase(self.name.clone(), self.start.elapsed(), self.detail.clone());
    }
}

pub fn phase(name: impl Into<String>) -> TracePhaseGuard {
    TracePhaseGuard {
        enabled: is_enabled(),
        name: name.into(),
        start: Instant::now(),
        detail: None,
    }
}

pub fn measure<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let _phase = phase(name);
    f()
}

pub fn record(name: &str, elapsed: Duration, detail: Option<String>) {
    if !is_enabled() {
        return;
    }
    ACTIVE_TRACE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.phases.push(TracePhase {
                name: name.to_string(),
                elapsed,
                detail,
            });
        }
    });
}

fn is_enabled() -> bool {
    ACTIVE_TRACE.with(|slot| slot.borrow().is_some())
}

fn record_phase(name: String, elapsed: Duration, detail: Option<String>) {
    ACTIVE_TRACE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.phases.push(TracePhase {
                name,
                elapsed,
                detail,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{measure, phase, TraceSession};

    #[test]
    fn trace_session_records_manual_and_measured_phases() {
        let _session = TraceSession::start("ls", &["--json".to_string()], true);
        {
            let mut lock = phase("repo_lock");
            lock.detail("acquired");
        }
        measure("query", || std::thread::sleep(Duration::from_millis(1)));
    }
}

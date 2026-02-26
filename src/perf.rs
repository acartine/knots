use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use uuid::Uuid;

use crate::app::{App, UpdateKnotPatch};
use crate::db;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PerfMeasurement {
    pub name: String,
    pub elapsed_ms: f64,
    pub budget_ms: f64,
    pub within_budget: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PerfReport {
    pub iterations: u32,
    pub measurements: Vec<PerfMeasurement>,
}

impl PerfReport {
    pub fn over_budget_count(&self) -> usize {
        self.measurements
            .iter()
            .filter(|m| !m.within_budget)
            .count()
    }
}

#[derive(Debug)]
pub enum PerfError {
    Io(std::io::Error),
    Db(rusqlite::Error),
    Other(String),
}

impl fmt::Display for PerfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PerfError::Io(err) => write!(f, "I/O error: {}", err),
            PerfError::Db(err) => write!(f, "database error: {}", err),
            PerfError::Other(message) => write!(f, "{}", message),
        }
    }
}

impl Error for PerfError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PerfError::Io(err) => Some(err),
            PerfError::Db(err) => Some(err),
            PerfError::Other(_) => None,
        }
    }
}

impl From<std::io::Error> for PerfError {
    fn from(value: std::io::Error) -> Self {
        PerfError::Io(value)
    }
}

impl From<rusqlite::Error> for PerfError {
    fn from(value: rusqlite::Error) -> Self {
        PerfError::Db(value)
    }
}

pub fn run_perf_harness(iterations: u32) -> Result<PerfReport, PerfError> {
    let iterations = iterations.max(1);
    let root = setup_workspace()?;
    let db_path = root.join(".knots/cache/state.sqlite");
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("db parent should exist for perf harness"),
    )?;

    let app = App::open(db_path.to_str().expect("utf8 path"), root.clone())
        .map_err(|err| PerfError::Other(err.to_string()))?;
    let conn = db::open_connection(db_path.to_str().expect("utf8 path"))?;
    db::set_meta(&conn, "sync_policy", "never")?;

    let write_elapsed = benchmark_write_path(&app, iterations)?;
    let read_elapsed = benchmark_hot_reads(&app, iterations)?;

    app.init_remote()
        .map_err(|err| PerfError::Other(err.to_string()))?;
    let _ = app
        .push()
        .map_err(|err| PerfError::Other(err.to_string()))?;
    let sync_start = Instant::now();
    let _ = app
        .sync()
        .map_err(|err| PerfError::Other(err.to_string()))?;
    let sync_elapsed = sync_start.elapsed().as_secs_f64() * 1000.0;

    let measurements = vec![
        measurement("read_hot_avg", read_elapsed, 20.0),
        measurement("write_avg", write_elapsed, 150.0),
        measurement("sync", sync_elapsed, 1000.0),
    ];

    let _ = std::fs::remove_dir_all(root);

    Ok(PerfReport {
        iterations,
        measurements,
    })
}

fn benchmark_write_path(app: &App, iterations: u32) -> Result<f64, PerfError> {
    let mut total_ms = 0.0;
    for idx in 0..iterations {
        let start = Instant::now();
        let knot = app
            .create_knot(
                &format!("perf-write-{idx}"),
                Some("body"),
                Some("work_item"),
                Some("default"),
            )
            .map_err(|err| PerfError::Other(err.to_string()))?;
        let _ = app
            .update_knot(
                &knot.id,
                UpdateKnotPatch {
                    title: None,
                    description: Some("updated".to_string()),
                    priority: Some(1),
                    status: Some("implementing".to_string()),
                    knot_type: Some(crate::domain::knot_type::KnotType::Work),
                    add_tags: vec![],
                    remove_tags: vec![],
                    add_note: None,
                    add_handoff_capsule: None,
                    expected_profile_etag: knot.profile_etag.clone(),
                    force: false,
                    state_actor: crate::app::StateActorMetadata::default(),
                },
            )
            .map_err(|err| PerfError::Other(err.to_string()))?;
        total_ms += start.elapsed().as_secs_f64() * 1000.0;
    }

    Ok(total_ms / iterations as f64)
}

fn benchmark_hot_reads(app: &App, iterations: u32) -> Result<f64, PerfError> {
    for idx in 0..20 {
        let _ = app
            .create_knot(
                &format!("perf-read-{idx}"),
                None,
                Some("work_item"),
                Some("default"),
            )
            .map_err(|err| PerfError::Other(err.to_string()))?;
    }

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = app
            .list_knots()
            .map_err(|err| PerfError::Other(err.to_string()))?;
    }
    Ok((start.elapsed().as_secs_f64() * 1000.0) / iterations as f64)
}

fn setup_workspace() -> Result<PathBuf, PerfError> {
    let root = std::env::temp_dir().join(format!("knots-perf-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root)?;

    let origin = root.join("origin.git");
    let local = root.join("local");
    std::fs::create_dir_all(&local)?;

    run_git(
        &root,
        &["init", "--bare", origin.to_str().expect("utf8 path")],
    )?;
    run_git(&local, &["init"])?;
    run_git(&local, &["config", "user.email", "knots@example.com"])?;
    run_git(&local, &["config", "user.name", "Knots Test"])?;

    std::fs::write(local.join("README.md"), "# perf\n")?;
    std::fs::write(local.join(".gitignore"), "/.knots/\n")?;
    run_git(&local, &["add", "README.md", ".gitignore"])?;
    run_git(&local, &["commit", "-m", "init"])?;
    run_git(&local, &["branch", "-M", "main"])?;
    run_git(
        &local,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("utf8 path"),
        ],
    )?;
    run_git(&local, &["push", "-u", "origin", "main"])?;
    set_bare_head_to_main(&origin)?;

    Ok(local)
}

fn set_bare_head_to_main(origin: &Path) -> Result<(), PerfError> {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(origin)
        .args(["symbolic-ref", "HEAD", "refs/heads/main"])
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(PerfError::Io(std::io::Error::other(
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
    )))
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<(), PerfError> {
    let output = Command::new("git").arg("-C").arg(cwd).args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(PerfError::Io(std::io::Error::other(format!(
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    ))))
}

fn measurement(name: &str, elapsed_ms: f64, budget_ms: f64) -> PerfMeasurement {
    PerfMeasurement {
        name: name.to_string(),
        elapsed_ms,
        budget_ms,
        within_budget: elapsed_ms <= budget_ms,
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::path::Path;

    use super::{
        run_git, run_perf_harness, set_bare_head_to_main, PerfError, PerfMeasurement, PerfReport,
    };

    #[test]
    fn produces_measurements() {
        let report = run_perf_harness(2).expect("perf harness should run");
        assert_eq!(report.measurements.len(), 3);
        assert!(report
            .measurements
            .iter()
            .all(|measurement| measurement.elapsed_ms >= 0.0));
    }

    #[test]
    fn over_budget_and_error_helpers_cover_edge_paths() {
        let report = PerfReport {
            iterations: 1,
            measurements: vec![
                PerfMeasurement {
                    name: "ok".to_string(),
                    elapsed_ms: 1.0,
                    budget_ms: 2.0,
                    within_budget: true,
                },
                PerfMeasurement {
                    name: "slow".to_string(),
                    elapsed_ms: 3.0,
                    budget_ms: 2.0,
                    within_budget: false,
                },
            ],
        };
        assert_eq!(report.over_budget_count(), 1);

        let io_err: PerfError = std::io::Error::other("disk").into();
        assert!(io_err.to_string().contains("I/O error"));
        assert!(io_err.source().is_some());

        let db_err: PerfError = rusqlite::Error::InvalidQuery.into();
        assert!(db_err.to_string().contains("database error"));
        assert!(db_err.source().is_some());

        let other = PerfError::Other("other".to_string());
        assert_eq!(other.to_string(), "other");
        assert!(other.source().is_none());
    }

    #[test]
    fn git_helpers_report_failures() {
        let bad_git = run_git(Path::new("."), &["definitely-not-a-valid-git-command"]);
        assert!(bad_git.is_err());

        let bad_head = set_bare_head_to_main(Path::new("/no/such/bare-repo.git"));
        assert!(bad_head.is_err());
    }
}

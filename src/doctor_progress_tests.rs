use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

use super::{run_doctor_with_fix_at_with_progress, DoctorCheck, DoctorReport, DoctorStatus};
use crate::doctor_fix::announce_and_apply_fixes;
use crate::progress::{ProgressKind, ProgressReporter};
use crate::project::DistributionMode;

#[derive(Default)]
struct CapturingReporter {
    events: Vec<(ProgressKind, String)>,
}

impl ProgressReporter for CapturingReporter {
    fn emit(&mut self, kind: ProgressKind, message: &str) -> std::io::Result<()> {
        self.events.push((kind, message.to_string()));
        Ok(())
    }
}

fn unique_workspace() -> PathBuf {
    let root = std::env::temp_dir().join(format!("knots-doctor-progress-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("workspace should be creatable");
    root
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo_with_origin() -> (PathBuf, PathBuf) {
    let root = unique_workspace();
    let origin = root.join("origin.git");
    let local = root.join("local");

    std::fs::create_dir_all(&local).expect("local directory should be creatable");
    run_git(
        &root,
        &["init", "--bare", origin.to_str().expect("utf8 origin path")],
    );
    run_git(&local, &["init"]);
    run_git(&local, &["config", "user.email", "knots@example.com"]);
    run_git(&local, &["config", "user.name", "Knots Test"]);
    std::fs::write(local.join("README.md"), "# doctor\n").expect("readme should be writable");
    run_git(&local, &["add", "README.md"]);
    run_git(&local, &["commit", "-m", "init"]);
    run_git(&local, &["branch", "-M", "main"]);
    run_git(
        &local,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("utf8 origin path"),
        ],
    );
    run_git(&local, &["push", "-u", "origin", "main"]);

    (root, local)
}

#[test]
fn run_doctor_with_fix_progress_is_silent_when_fix_false() {
    let (root, local) = setup_repo_with_origin();
    let baseline = run_doctor_with_fix_at_with_progress(
        &local,
        &local.join(".knots"),
        DistributionMode::Git,
        false,
        &mut None,
    )
    .expect("baseline doctor should run");
    let mut reporter = CapturingReporter::default();
    let mut dyn_reporter: Option<&mut dyn ProgressReporter> = Some(&mut reporter);
    let captured = run_doctor_with_fix_at_with_progress(
        &local,
        &local.join(".knots"),
        DistributionMode::Git,
        false,
        &mut dyn_reporter,
    )
    .expect("doctor should run");
    assert!(
        reporter.events.is_empty(),
        "plain `kno doctor` (fix=false) must not emit any progress lines; got: {:?}",
        reporter.events
    );
    assert_eq!(captured.checks, baseline.checks);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_doctor_with_fix_progress_announces_diagnostics_then_per_fix_and_summary() {
    let (root, local) = setup_repo_with_origin();
    let mut reporter = CapturingReporter::default();
    let mut dyn_reporter: Option<&mut dyn ProgressReporter> = Some(&mut reporter);
    let _ = run_doctor_with_fix_at_with_progress(
        &local,
        &local.join(".knots"),
        DistributionMode::Git,
        true,
        &mut dyn_reporter,
    )
    .expect("doctor fix should run");

    let first = reporter.events.first().expect("at least one event");
    assert_eq!(first.0, ProgressKind::Stage);
    assert!(
        first.1.contains("Running diagnostics"),
        "first line should announce diagnostics, got {:?}",
        first
    );

    let fix_lines: Vec<&str> = reporter
        .events
        .iter()
        .filter_map(|(_, msg)| msg.starts_with("Fixing ").then_some(msg.as_str()))
        .collect();
    assert!(
        !fix_lines.is_empty(),
        "a freshly initialized repo should produce at least one fix line; got events: {:?}",
        reporter.events
    );
    assert!(fix_lines
        .iter()
        .all(|msg| msg.ends_with(" ok") || msg.ends_with(" skip")));
    assert!(fix_lines.iter().any(|msg| msg.ends_with(" ok")));
    assert!(reporter
        .events
        .iter()
        .filter(|(_, msg)| msg.starts_with("Fixing "))
        .all(|(kind, _)| *kind == ProgressKind::Info));

    let last = reporter.events.last().expect("at least one event");
    assert_eq!(last.0, ProgressKind::Success);
    assert!(
        last.1.contains("fixed") && last.1.contains("skipped") && last.1.contains("failed"),
        "summary line should report result, got {:?}",
        last
    );
    assert!(
        last.1.contains("0 failed"),
        "unexpected failed fix: {:?}",
        last
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn announce_and_apply_fixes_reports_ordered_results_then_final_summary() {
    let root = unique_workspace();
    let report = DoctorReport {
        checks: vec![
            DoctorCheck::simple("lock_health", DoctorStatus::Pass, "ok"),
            DoctorCheck::simple("hooks", DoctorStatus::Warn, "install hooks"),
            DoctorCheck::simple("remote", DoctorStatus::Fail, "missing remote"),
            DoctorCheck::simple("unknown_check", DoctorStatus::Warn, "unsupported"),
        ],
    };
    let mut reporter = CapturingReporter::default();
    let mut dyn_reporter: Option<&mut dyn ProgressReporter> = Some(&mut reporter);

    // Captured events verify ordering, but not real-time flushing on a TTY.
    // Manual check: run `kno doctor --fix` in a repo with a fixable warning and
    // observe each progress line appears before the next fix starts.
    announce_and_apply_fixes(&root, DistributionMode::Git, &report, &mut dyn_reporter);

    let messages: Vec<&str> = reporter.events.iter().map(|(_, m)| m.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "Fixing hooks... ok",
            "Fixing remote... ok",
            "Fixing unknown_check... skip",
            "2 fixed, 1 skipped, 0 failed",
        ]
    );
    assert_eq!(reporter.events[0].0, ProgressKind::Info);
    assert_eq!(reporter.events[1].0, ProgressKind::Info);
    assert_eq!(reporter.events[2].0, ProgressKind::Info);
    assert_eq!(reporter.events[3].0, ProgressKind::Success);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn announce_and_apply_fixes_reports_no_issues_when_all_checks_pass() {
    let root = unique_workspace();
    let report = DoctorReport {
        checks: vec![
            DoctorCheck::simple("lock_health", DoctorStatus::Pass, "ok"),
            DoctorCheck::simple("worktree", DoctorStatus::Pass, "ok"),
        ],
    };
    let mut reporter = CapturingReporter::default();
    let mut dyn_reporter: Option<&mut dyn ProgressReporter> = Some(&mut reporter);
    announce_and_apply_fixes(&root, DistributionMode::Git, &report, &mut dyn_reporter);

    assert_eq!(reporter.events.len(), 1);
    assert_eq!(reporter.events[0].0, ProgressKind::Success);
    assert_eq!(reporter.events[0].1, "No issues found.");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn announce_and_apply_fixes_local_only_reports_count_without_fixing() {
    let root = unique_workspace();
    let report = DoctorReport {
        checks: vec![
            DoctorCheck::simple("worktree", DoctorStatus::Pass, "ok"),
            DoctorCheck::simple("version", DoctorStatus::Warn, "update available"),
            DoctorCheck::simple("lock_health", DoctorStatus::Fail, "busy"),
        ],
    };
    let mut reporter = CapturingReporter::default();
    let mut dyn_reporter: Option<&mut dyn ProgressReporter> = Some(&mut reporter);
    announce_and_apply_fixes(
        &root,
        DistributionMode::LocalOnly,
        &report,
        &mut dyn_reporter,
    );

    let messages: Vec<&str> = reporter.events.iter().map(|(_, m)| m.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|msg| msg.contains("local-only mode") && msg.contains("2 issue(s) found")),
        "local-only should announce the count without running fixes; got: {:?}",
        messages
    );
    assert!(
        !messages.iter().any(|msg| msg.starts_with("Fixing ")),
        "local-only must not emit per-fix lines; got: {:?}",
        messages
    );
    let _ = std::fs::remove_dir_all(root);
}

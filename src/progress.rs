use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressKind {
    Stage,
    Info,
    Success,
    Warn,
}

pub(crate) trait ProgressReporter {
    fn emit(&mut self, kind: ProgressKind, message: &str) -> io::Result<()>;
}

pub(crate) fn emit_progress(
    reporter: &mut Option<&mut dyn ProgressReporter>,
    kind: ProgressKind,
    message: impl AsRef<str>,
) -> io::Result<()> {
    if let Some(reporter) = reporter.as_deref_mut() {
        reporter.emit(kind, message.as_ref())
    } else {
        Ok(())
    }
}

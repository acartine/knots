use std::io::{self, Write};

use crate::progress::{ProgressKind, ProgressReporter};

use super::Palette;

pub(crate) struct StdoutProgressReporter {
    stdout: io::Stdout,
    palette: Palette,
}

impl StdoutProgressReporter {
    pub(crate) fn auto() -> Self {
        Self {
            stdout: io::stdout(),
            palette: Palette::auto(),
        }
    }
}

impl ProgressReporter for StdoutProgressReporter {
    fn emit(&mut self, kind: ProgressKind, message: &str) -> io::Result<()> {
        writeln!(
            self.stdout,
            "{}",
            format_progress_line(&self.palette, kind, message)
        )?;
        self.stdout.flush()
    }
}

pub(crate) fn format_progress_line(palette: &Palette, kind: ProgressKind, message: &str) -> String {
    let (code, icon) = match kind {
        ProgressKind::Stage => ("1;36", "•"),
        ProgressKind::Info => ("2;36", "·"),
        ProgressKind::Success => ("1;32", "✓"),
        ProgressKind::Warn => ("1;33", "!"),
    };
    format!("{} {}", palette.paint(code, icon), message)
}

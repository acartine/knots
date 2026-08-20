use std::path::{Path, PathBuf};
use std::process::Command;

use crate::profile::ProfileError;

pub(crate) trait LoomBundleBuilder {
    fn build_knots_bundle(&self, source: &Path) -> Result<String, ProfileError>;
}

pub(crate) struct CommandLoomBundleBuilder {
    binary: PathBuf,
}

impl CommandLoomBundleBuilder {
    /// Production resolution: `KNOTS_LOOM_BIN` if set, else the bare `loom`
    /// name resolved through `PATH` by the OS at spawn time.
    pub(crate) fn new() -> Self {
        let binary = std::env::var_os("KNOTS_LOOM_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("loom"));
        Self { binary }
    }

    /// Test seam: point directly at an absolute binary/script path so tests
    /// never have to mutate the process-global `PATH` (or any other
    /// process-global environment variable) to control which binary a test
    /// shells out to.
    #[cfg(test)]
    pub(crate) fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl LoomBundleBuilder for CommandLoomBundleBuilder {
    fn build_knots_bundle(&self, source: &Path) -> Result<String, ProfileError> {
        let output = Command::new(&self.binary)
            .arg("build")
            .arg(source)
            .arg("--emit")
            .arg("knots-bundle")
            .output()
            .map_err(|err| ProfileError::InvalidBundle(format!("failed to execute loom: {err}")))?;
        if !output.status.success() {
            return Err(ProfileError::InvalidBundle(format!(
                "loom build --emit knots-bundle failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        String::from_utf8(output.stdout).map_err(|err| {
            ProfileError::InvalidBundle(format!("invalid UTF-8 bundle output: {err}"))
        })
    }
}

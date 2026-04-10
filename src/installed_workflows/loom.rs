use std::path::Path;
use std::process::Command;

use crate::profile::ProfileError;

pub(crate) trait LoomBundleBuilder {
    fn build_knots_bundle(&self, source: &Path) -> Result<String, ProfileError>;
}

pub(crate) struct CommandLoomBundleBuilder;

impl LoomBundleBuilder for CommandLoomBundleBuilder {
    fn build_knots_bundle(&self, source: &Path) -> Result<String, ProfileError> {
        let loom_bin = std::env::var("KNOTS_LOOM_BIN").unwrap_or_else(|_| "loom".to_string());
        let output = Command::new(loom_bin)
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

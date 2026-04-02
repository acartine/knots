use std::fmt;
use std::str::FromStr;

/// Canonical artifact/output target types for workflow actions.
///
/// Each variant represents a kind of artifact a workflow step produces.
/// `FromStr` accepts these canonical names and rejects unknown strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactTarget {
    Local,
    Remote,
    RemoteMain,
    Pr,
    Branch,
    Note,
    Approval,
    LiveDeployment,
}

impl ArtifactTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
            Self::RemoteMain => "remote_main",
            Self::Pr => "pr",
            Self::Branch => "branch",
            Self::Note => "note",
            Self::Approval => "approval",
            Self::LiveDeployment => "live_deployment",
        }
    }
}

impl fmt::Display for ArtifactTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ArtifactTarget {
    type Err = UnknownArtifactTarget;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local" => Ok(Self::Local),
            "remote" => Ok(Self::Remote),
            "remote_main" => Ok(Self::RemoteMain),
            "pr" => Ok(Self::Pr),
            "branch" => Ok(Self::Branch),
            "note" => Ok(Self::Note),
            "approval" => Ok(Self::Approval),
            "live_deployment" => Ok(Self::LiveDeployment),
            _ => Err(UnknownArtifactTarget(s.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnknownArtifactTarget(pub String);

impl fmt::Display for UnknownArtifactTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown artifact target: {:?}", self.0)
    }
}

impl std::error::Error for UnknownArtifactTarget {}

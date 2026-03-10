use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GateOwnerKind {
    Human,
    #[default]
    Agent,
}

impl GateOwnerKind {
    pub const ALL: [GateOwnerKind; 2] = [GateOwnerKind::Human, GateOwnerKind::Agent];

    pub fn as_str(self) -> &'static str {
        match self {
            GateOwnerKind::Human => "human",
            GateOwnerKind::Agent => "agent",
        }
    }
}

impl fmt::Display for GateOwnerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for GateOwnerKind {
    type Err = ParseGateOwnerKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "human" => Ok(GateOwnerKind::Human),
            "agent" | "" => Ok(GateOwnerKind::Agent),
            _ => Err(ParseGateOwnerKindError {
                value: value.to_string(),
            }),
        }
    }
}

impl Serialize for GateOwnerKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GateOwnerKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        GateOwnerKind::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateData {
    #[serde(default)]
    pub owner_kind: GateOwnerKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub failure_modes: BTreeMap<String, Vec<String>>,
}

impl GateData {
    pub fn find_reopen_targets(&self, invariant: &str) -> Option<&Vec<String>> {
        let target = normalize_invariant_key(invariant)?;
        self.failure_modes.iter().find_map(|(key, ids)| {
            (normalize_invariant_key(key).as_deref() == Some(target.as_str())).then_some(ids)
        })
    }
}

pub fn normalize_invariant_key(raw: &str) -> Option<String> {
    let normalized = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub fn parse_failure_mode_spec(
    raw: &str,
) -> Result<(String, Vec<String>), ParseGateFailureModeError> {
    let Some((raw_invariant, raw_targets)) = raw.split_once('=') else {
        return Err(ParseGateFailureModeError::MissingSeparator);
    };
    let invariant = raw_invariant.trim();
    if invariant.is_empty() {
        return Err(ParseGateFailureModeError::EmptyInvariant);
    }
    let targets = raw_targets
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Err(ParseGateFailureModeError::EmptyTargets);
    }
    Ok((invariant.to_string(), targets))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseGateOwnerKindError {
    value: String,
}

impl fmt::Display for ParseGateOwnerKindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid gate owner kind '{}': expected one of {}",
            self.value,
            GateOwnerKind::ALL
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Error for ParseGateOwnerKindError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseGateFailureModeError {
    MissingSeparator,
    EmptyInvariant,
    EmptyTargets,
}

impl fmt::Display for ParseGateFailureModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseGateFailureModeError::MissingSeparator => {
                write!(
                    f,
                    "invalid gate failure mode: expected '<invariant>=<knot-id[,knot-id...]>'"
                )
            }
            ParseGateFailureModeError::EmptyInvariant => {
                write!(f, "invalid gate failure mode: invariant cannot be empty")
            }
            ParseGateFailureModeError::EmptyTargets => {
                write!(
                    f,
                    "invalid gate failure mode: at least one knot id is required"
                )
            }
        }
    }
}

impl Error for ParseGateFailureModeError {}

#[cfg(test)]
mod tests {
    use super::{
        normalize_invariant_key, parse_failure_mode_spec, GateData, GateOwnerKind,
        ParseGateFailureModeError, ParseGateOwnerKindError,
    };
    use std::collections::BTreeMap;
    use std::str::FromStr;

    #[test]
    fn owner_kind_defaults_to_agent() {
        assert_eq!(GateOwnerKind::default(), GateOwnerKind::Agent);
    }

    #[test]
    fn owner_kind_parses_human_and_agent() {
        assert_eq!(
            GateOwnerKind::from_str("human").unwrap(),
            GateOwnerKind::Human
        );
        assert_eq!(
            GateOwnerKind::from_str("agent").unwrap(),
            GateOwnerKind::Agent
        );
    }

    #[test]
    fn owner_kind_rejects_unknown_values() {
        let err = GateOwnerKind::from_str("robot").unwrap_err();
        assert_eq!(
            err,
            ParseGateOwnerKindError {
                value: "robot".to_string()
            }
        );
        assert!(err.to_string().contains("expected one of human, agent"));
    }

    #[test]
    fn owner_kind_serializes_and_deserializes_as_string() {
        let raw = serde_json::to_string(&GateOwnerKind::Human).unwrap();
        assert_eq!(raw, "\"human\"");
        let parsed: GateOwnerKind = serde_json::from_str("\"agent\"").unwrap();
        assert_eq!(parsed, GateOwnerKind::Agent);
        let err = serde_json::from_str::<GateOwnerKind>("\"robot\"").unwrap_err();
        assert!(err.to_string().contains("invalid gate owner kind"));
    }

    #[test]
    fn normalize_invariant_key_collapses_whitespace() {
        assert_eq!(
            normalize_invariant_key("  Must   stay green  "),
            Some("must stay green".to_string())
        );
    }

    #[test]
    fn normalize_invariant_key_rejects_blank_values() {
        assert_eq!(normalize_invariant_key(" \n\t "), None);
    }

    #[test]
    fn parse_failure_mode_spec_requires_separator_and_invariant() {
        assert_eq!(
            parse_failure_mode_spec("must stay green").unwrap_err(),
            ParseGateFailureModeError::MissingSeparator
        );
        assert_eq!(
            parse_failure_mode_spec(" = knots-a").unwrap_err(),
            ParseGateFailureModeError::EmptyInvariant
        );
    }

    #[test]
    fn parse_failure_mode_spec_requires_targets() {
        let err = parse_failure_mode_spec("must stay green=").unwrap_err();
        assert_eq!(err, ParseGateFailureModeError::EmptyTargets);
    }

    #[test]
    fn parse_failure_mode_spec_parses_multiple_targets() {
        let (invariant, targets) =
            parse_failure_mode_spec("must stay green = knots-a, knots-b").unwrap();
        assert_eq!(invariant, "must stay green");
        assert_eq!(targets, vec!["knots-a".to_string(), "knots-b".to_string()]);
    }

    #[test]
    fn find_reopen_targets_matches_normalized_invariant() {
        let mut failure_modes = BTreeMap::new();
        failure_modes.insert(
            "must stay green".to_string(),
            vec!["knots-a".to_string(), "knots-b".to_string()],
        );
        let data = GateData {
            owner_kind: GateOwnerKind::Agent,
            failure_modes,
        };
        assert_eq!(
            data.find_reopen_targets("Must   Stay Green")
                .cloned()
                .unwrap(),
            vec!["knots-a".to_string(), "knots-b".to_string()]
        );
    }

    #[test]
    fn find_reopen_targets_returns_none_for_blank_or_missing_invariant() {
        let data = GateData::default();
        assert!(data.find_reopen_targets(" ").is_none());
        assert!(data.find_reopen_targets("missing").is_none());
    }
}

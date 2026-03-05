use std::error::Error;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvariantType {
    Scope,
    State,
}

impl InvariantType {
    pub const ALL: [InvariantType; 2] = [InvariantType::Scope, InvariantType::State];

    pub fn as_str(self) -> &'static str {
        match self {
            InvariantType::Scope => "Scope",
            InvariantType::State => "State",
        }
    }
}

impl fmt::Display for InvariantType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InvariantType {
    type Err = ParseInvariantTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "scope" => Ok(InvariantType::Scope),
            "state" => Ok(InvariantType::State),
            _ => Err(ParseInvariantTypeError {
                value: value.to_string(),
            }),
        }
    }
}

impl Serialize for InvariantType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InvariantType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        InvariantType::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Invariant {
    #[serde(rename = "type", alias = "invariant_type")]
    pub invariant_type: InvariantType,
    pub condition: String,
}

impl Invariant {
    pub fn new(
        invariant_type: InvariantType,
        condition: impl Into<String>,
    ) -> Result<Self, ParseInvariantSpecError> {
        let condition = condition.into();
        let condition = condition.trim();
        if condition.is_empty() {
            return Err(ParseInvariantSpecError::EmptyCondition);
        }
        Ok(Self {
            invariant_type,
            condition: condition.to_string(),
        })
    }
}

impl fmt::Display for Invariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.invariant_type, self.condition)
    }
}

pub fn parse_invariant_spec(raw: &str) -> Result<Invariant, ParseInvariantSpecError> {
    let Some((raw_type, raw_condition)) = raw.split_once(':') else {
        return Err(ParseInvariantSpecError::MissingSeparator);
    };
    let invariant_type =
        InvariantType::from_str(raw_type).map_err(ParseInvariantSpecError::InvalidType)?;
    Invariant::new(invariant_type, raw_condition)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseInvariantTypeError {
    value: String,
}

impl fmt::Display for ParseInvariantTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid invariant type '{}': expected one of {}",
            self.value,
            InvariantType::ALL
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Error for ParseInvariantTypeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseInvariantSpecError {
    MissingSeparator,
    InvalidType(ParseInvariantTypeError),
    EmptyCondition,
}

impl fmt::Display for ParseInvariantSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseInvariantSpecError::MissingSeparator => write!(
                f,
                "invalid invariant format: expected '<Scope|State>:<condition>'"
            ),
            ParseInvariantSpecError::InvalidType(err) => err.fmt(f),
            ParseInvariantSpecError::EmptyCondition => {
                write!(f, "invalid invariant format: condition cannot be empty")
            }
        }
    }
}

impl Error for ParseInvariantSpecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ParseInvariantSpecError::InvalidType(err) => Some(err),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_invariant_spec, Invariant, InvariantType};
    use std::str::FromStr;

    #[test]
    fn invariant_type_parses_case_insensitively() {
        assert_eq!(
            InvariantType::from_str("scope").unwrap(),
            InvariantType::Scope
        );
        assert_eq!(
            InvariantType::from_str("STATE").unwrap(),
            InvariantType::State
        );
    }

    #[test]
    fn invariant_type_error_mentions_allowed_values() {
        let err = InvariantType::from_str("other").expect_err("unknown type should fail");
        let message = err.to_string();
        assert!(message.contains("invalid invariant type"));
        assert!(message.contains("Scope"));
        assert!(message.contains("State"));
    }

    #[test]
    fn parse_invariant_spec_requires_separator() {
        let err = parse_invariant_spec("scope no colon").expect_err("format should fail");
        assert!(err
            .to_string()
            .contains("expected '<Scope|State>:<condition>'"));
    }

    #[test]
    fn parse_invariant_spec_requires_condition() {
        let err = parse_invariant_spec("scope:  ").expect_err("empty condition should fail");
        assert!(err.to_string().contains("condition cannot be empty"));
    }

    #[test]
    fn parse_invariant_spec_parses_valid_input() {
        let invariant = parse_invariant_spec("Scope:keep every step idempotent")
            .expect("valid spec should parse");
        assert_eq!(invariant.invariant_type, InvariantType::Scope);
        assert_eq!(invariant.condition, "keep every step idempotent");
    }

    #[test]
    fn serde_round_trip_uses_type_key() {
        let invariant = Invariant::new(InvariantType::State, "must remain in queue").unwrap();
        let json = serde_json::to_value(&invariant).expect("serialize should work");
        assert_eq!(json["type"], "State");
        assert_eq!(json["condition"], "must remain in queue");
        assert!(json.get("invariant_type").is_none());

        let parsed: Invariant = serde_json::from_value(json).expect("deserialize should work");
        assert_eq!(parsed, invariant);
    }

    #[test]
    fn invariant_display_shows_type_and_condition() {
        let inv = Invariant::new(InvariantType::Scope, "keep idempotent").unwrap();
        assert_eq!(inv.to_string(), "Scope: keep idempotent");
    }

    #[test]
    fn parse_invariant_spec_invalid_type_delegates_display() {
        let err = parse_invariant_spec("bogus:cond").expect_err("bad type");
        assert!(err.to_string().contains("invalid invariant type"));
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn parse_invariant_spec_missing_separator_has_no_source() {
        let err = parse_invariant_spec("nocolon").expect_err("no colon");
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn deserialize_rejects_unknown_invariant_type() {
        let json = serde_json::json!({"type": "bogus", "condition": "x"});
        let result = serde_json::from_value::<Invariant>(json);
        assert!(result.is_err());
    }

    #[test]
    fn invariant_type_all_contains_both_variants() {
        assert_eq!(super::InvariantType::ALL.len(), 2);
        assert_eq!(super::InvariantType::ALL[0], InvariantType::Scope);
        assert_eq!(super::InvariantType::ALL[1], InvariantType::State);
    }

    #[test]
    fn deserialize_accepts_invariant_type_alias() {
        let json = serde_json::json!({
            "invariant_type": "Scope",
            "condition": "only db module"
        });
        let inv: Invariant = serde_json::from_value(json).expect("alias should parse");
        assert_eq!(inv.invariant_type, InvariantType::Scope);
        assert_eq!(inv.condition, "only db module");
    }

    #[test]
    fn serde_round_trip_vec_of_invariants() {
        let invariants = vec![
            Invariant::new(InvariantType::Scope, "src/ only").unwrap(),
            Invariant::new(InvariantType::State, "no regressions").unwrap(),
        ];
        let json = serde_json::to_string(&invariants).expect("serialize vec should work");
        let parsed: Vec<Invariant> = serde_json::from_str(&json).expect("deserialize vec");
        assert_eq!(parsed, invariants);
    }

    #[test]
    fn invariant_type_display_matches_as_str() {
        for kind in InvariantType::ALL {
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }
}

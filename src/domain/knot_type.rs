use std::error::Error;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum KnotType {
    #[default]
    Work,
}

impl KnotType {
    pub const ALL: [KnotType; 1] = [KnotType::Work];

    pub fn as_str(self) -> &'static str {
        match self {
            KnotType::Work => "work",
        }
    }
}

impl fmt::Display for KnotType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KnotType {
    type Err = ParseKnotTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "work" | "task" | "" => Ok(KnotType::Work),
            _ => Err(ParseKnotTypeError {
                value: value.to_string(),
            }),
        }
    }
}

impl Serialize for KnotType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for KnotType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        KnotType::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKnotTypeError {
    value: String,
}

impl fmt::Display for ParseKnotTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid knot type '{}': expected one of {}",
            self.value,
            KnotType::ALL
                .iter()
                .map(|kt| kt.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Error for ParseKnotTypeError {}

/// Parse an `Option<String>` from the DB layer into a `KnotType`.
///
/// Backward compat: `None`, empty, and `"task"` all map to `Work`.
pub fn parse_knot_type(raw: Option<&str>) -> KnotType {
    match raw {
        None => KnotType::default(),
        Some(value) => KnotType::from_str(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_knot_type, KnotType, ParseKnotTypeError};
    use std::str::FromStr;

    #[test]
    fn round_trip_work() {
        let kt = KnotType::from_str("work").unwrap();
        assert_eq!(kt, KnotType::Work);
        assert_eq!(kt.as_str(), "work");
        let parsed: KnotType = KnotType::from_str(kt.as_str()).unwrap();
        assert_eq!(parsed, kt);
    }

    #[test]
    fn legacy_alias_task_maps_to_work() {
        let kt = KnotType::from_str("task").unwrap();
        assert_eq!(kt, KnotType::Work);
    }

    #[test]
    fn empty_string_maps_to_work() {
        let kt = KnotType::from_str("").unwrap();
        assert_eq!(kt, KnotType::Work);
    }

    #[test]
    fn whitespace_only_maps_to_work() {
        let kt = KnotType::from_str("   ").unwrap();
        assert_eq!(kt, KnotType::Work);
    }

    #[test]
    fn invalid_value_returns_error() {
        let err = KnotType::from_str("epic").expect_err("unknown type should fail");
        assert!(err.to_string().contains("invalid knot type"));
        assert!(err.to_string().contains("epic"));
    }

    #[test]
    fn default_is_work() {
        assert_eq!(KnotType::default(), KnotType::Work);
    }

    #[test]
    fn display_uses_as_str() {
        assert_eq!(format!("{}", KnotType::Work), "work");
    }

    #[test]
    fn serde_round_trip() {
        let serialized = serde_json::to_string(&KnotType::Work).expect("serialize should succeed");
        assert_eq!(serialized, "\"work\"");

        let deserialized: KnotType =
            serde_json::from_str(&serialized).expect("deserialize should succeed");
        assert_eq!(deserialized, KnotType::Work);
    }

    #[test]
    fn serde_deserialize_legacy_alias() {
        let kt: KnotType = serde_json::from_str("\"task\"").expect("task alias should deserialize");
        assert_eq!(kt, KnotType::Work);
    }

    #[test]
    fn parse_knot_type_backward_compat() {
        assert_eq!(parse_knot_type(None), KnotType::Work);
        assert_eq!(parse_knot_type(Some("")), KnotType::Work);
        assert_eq!(parse_knot_type(Some("task")), KnotType::Work);
        assert_eq!(parse_knot_type(Some("work")), KnotType::Work);
        assert_eq!(parse_knot_type(Some("unknown")), KnotType::Work);
    }

    #[test]
    fn parse_knot_type_error_display() {
        let err = ParseKnotTypeError {
            value: "bad".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("invalid knot type 'bad'"));
        assert!(msg.contains("work"));
    }
}

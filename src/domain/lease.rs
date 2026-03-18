use std::error::Error;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LeaseType {
    #[default]
    Agent,
    Manual,
}

impl LeaseType {
    pub const ALL: [LeaseType; 2] = [LeaseType::Agent, LeaseType::Manual];

    pub fn as_str(self) -> &'static str {
        match self {
            LeaseType::Agent => "agent",
            LeaseType::Manual => "manual",
        }
    }
}

impl fmt::Display for LeaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LeaseType {
    type Err = ParseLeaseTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "agent" | "" => Ok(LeaseType::Agent),
            "manual" => Ok(LeaseType::Manual),
            _ => Err(ParseLeaseTypeError {
                value: value.to_string(),
            }),
        }
    }
}

impl Serialize for LeaseType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LeaseType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        LeaseType::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLeaseTypeError {
    value: String,
}

impl fmt::Display for ParseLeaseTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid lease type '{}': expected one of {}",
            self.value,
            LeaseType::ALL
                .iter()
                .map(|lt| lt.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl Error for ParseLeaseTypeError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentInfo {
    pub agent_type: String,
    pub provider: String,
    pub agent_name: String,
    pub model: String,
    pub model_version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseData {
    #[serde(default)]
    pub lease_type: LeaseType,
    #[serde(default)]
    pub nickname: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_info: Option<AgentInfo>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseValidationError {
    EmptyNickname,
    MissingAgentInfo,
}

impl fmt::Display for LeaseValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeaseValidationError::EmptyNickname => {
                write!(f, "lease nickname must not be empty")
            }
            LeaseValidationError::MissingAgentInfo => {
                write!(f, "agent_info is required for agent lease_type")
            }
        }
    }
}

impl Error for LeaseValidationError {}

#[allow(dead_code)]
pub fn validate_lease_data(data: &LeaseData) -> Result<(), LeaseValidationError> {
    if data.nickname.trim().is_empty() {
        return Err(LeaseValidationError::EmptyNickname);
    }
    if data.lease_type == LeaseType::Agent && data.agent_info.is_none() {
        return Err(LeaseValidationError::MissingAgentInfo);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn lease_type_defaults_to_agent() {
        assert_eq!(LeaseType::default(), LeaseType::Agent);
    }

    #[test]
    fn lease_type_round_trip() {
        for lt in LeaseType::ALL {
            let parsed = LeaseType::from_str(lt.as_str()).unwrap();
            assert_eq!(parsed, lt);
        }
    }

    #[test]
    fn lease_type_rejects_unknown() {
        let err = LeaseType::from_str("robot").unwrap_err();
        assert!(err.to_string().contains("invalid lease type"));
        assert!(err.to_string().contains("expected one of agent, manual"));
    }

    #[test]
    fn lease_type_serde_round_trip() {
        let raw = serde_json::to_string(&LeaseType::Manual).unwrap();
        assert_eq!(raw, "\"manual\"");
        let parsed: LeaseType = serde_json::from_str("\"agent\"").unwrap();
        assert_eq!(parsed, LeaseType::Agent);
        let err = serde_json::from_str::<LeaseType>("\"robot\"").unwrap_err();
        assert!(err.to_string().contains("invalid lease type"));
    }

    #[test]
    fn lease_data_serde_round_trip() {
        let data = LeaseData {
            lease_type: LeaseType::Agent,
            nickname: "my-agent".to_string(),
            agent_info: Some(AgentInfo {
                agent_type: "cli".to_string(),
                provider: "Anthropic".to_string(),
                agent_name: "claude".to_string(),
                model: "opus".to_string(),
                model_version: "4".to_string(),
            }),
        };
        let json = serde_json::to_string(&data).unwrap();
        let parsed: LeaseData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn lease_data_default_has_empty_nickname() {
        let data = LeaseData::default();
        assert_eq!(data.lease_type, LeaseType::Agent);
        assert_eq!(data.nickname, "");
        assert!(data.agent_info.is_none());
    }

    #[test]
    fn lease_data_deserializes_legacy_empty_payload() {
        let parsed: LeaseData = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, LeaseData::default());
    }

    #[test]
    fn validate_rejects_empty_nickname() {
        let data = LeaseData {
            nickname: "  ".to_string(),
            agent_info: Some(AgentInfo {
                agent_type: "cli".to_string(),
                provider: "Anthropic".to_string(),
                agent_name: "claude".to_string(),
                model: "opus".to_string(),
                model_version: "4".to_string(),
            }),
            ..Default::default()
        };
        let err = validate_lease_data(&data).unwrap_err();
        assert_eq!(err, LeaseValidationError::EmptyNickname);
        assert!(err.to_string().contains("nickname must not be empty"));
    }

    #[test]
    fn validate_rejects_agent_without_info() {
        let data = LeaseData {
            lease_type: LeaseType::Agent,
            nickname: "my-agent".to_string(),
            agent_info: None,
        };
        let err = validate_lease_data(&data).unwrap_err();
        assert_eq!(err, LeaseValidationError::MissingAgentInfo);
        assert!(err.to_string().contains("agent_info is required"));
    }

    #[test]
    fn lease_type_display() {
        assert_eq!(format!("{}", LeaseType::Agent), "agent");
        assert_eq!(format!("{}", LeaseType::Manual), "manual");
    }

    #[test]
    fn validate_accepts_valid_agent_lease() {
        let data = LeaseData {
            lease_type: LeaseType::Agent,
            nickname: "valid".to_string(),
            agent_info: Some(AgentInfo {
                agent_type: "cli".to_string(),
                provider: "test".to_string(),
                agent_name: "agent".to_string(),
                model: "m".to_string(),
                model_version: "1".to_string(),
            }),
        };
        assert!(validate_lease_data(&data).is_ok());
    }

    #[test]
    fn validate_accepts_manual_lease_without_agent_info() {
        let data = LeaseData {
            lease_type: LeaseType::Manual,
            nickname: "manual".to_string(),
            agent_info: None,
        };
        assert!(validate_lease_data(&data).is_ok());
    }
}

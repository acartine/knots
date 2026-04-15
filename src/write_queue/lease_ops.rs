use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseCreateOperation {
    pub nickname: String,
    pub lease_type: String,
    pub agent_type: Option<String>,
    pub provider: Option<String>,
    pub agent_name: Option<String>,
    pub model: Option<String>,
    pub model_version: Option<String>,
    pub json: bool,
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseTerminateOperation {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeaseExtendOperation {
    pub lease_id: String,
    pub timeout_seconds: Option<u64>,
    pub json: bool,
}

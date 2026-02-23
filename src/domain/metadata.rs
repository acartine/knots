use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetadataEntry {
    pub entry_id: String,
    pub content: String,
    pub username: String,
    pub datetime: String,
    pub agentname: String,
    pub model: String,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct MetadataEntryInput {
    pub content: String,
    pub username: Option<String>,
    pub datetime: Option<String>,
    pub agentname: Option<String>,
    pub model: Option<String>,
    pub version: Option<String>,
}

impl MetadataEntry {
    pub fn from_input(input: MetadataEntryInput, fallback_datetime: &str) -> Self {
        let datetime = normalize_datetime(input.datetime.as_deref())
            .unwrap_or_else(|| fallback_datetime.to_string());
        Self {
            entry_id: Uuid::now_v7().to_string(),
            content: input.content.trim().to_string(),
            username: normalize_text(input.username.as_deref(), "unknown"),
            datetime,
            agentname: normalize_text(input.agentname.as_deref(), "unknown"),
            model: normalize_text(input.model.as_deref(), "unknown"),
            version: normalize_text(input.version.as_deref(), "unknown"),
        }
    }
}

pub fn normalize_text(value: Option<&str>, fallback: &str) -> String {
    match value.map(str::trim) {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => fallback.to_string(),
    }
}

pub fn normalize_datetime(value: Option<&str>) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    OffsetDateTime::parse(raw, &Rfc3339)
        .ok()
        .and_then(|ts| ts.format(&Rfc3339).ok())
}

use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum EventWriteError {
    InvalidTimestamp {
        value: String,
        source: time::error::Parse,
    },
    InvalidFileComponent {
        field: &'static str,
        value: String,
    },
    Io(std::io::Error),
    Serialize(serde_json::Error),
}

impl fmt::Display for EventWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventWriteError::InvalidTimestamp { value, source } => {
                write!(f, "invalid RFC3339 timestamp '{}': {}", value, source)
            }
            EventWriteError::InvalidFileComponent { field, value } => {
                write!(
                    f,
                    "invalid {} '{}': \
                     use only ASCII letters, numbers, '.', '-', '_'",
                    field, value
                )
            }
            EventWriteError::Io(err) => {
                write!(f, "I/O error while writing event: {}", err)
            }
            EventWriteError::Serialize(err) => {
                write!(f, "failed to serialize event as JSON: {}", err)
            }
        }
    }
}

impl Error for EventWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EventWriteError::InvalidTimestamp { source, .. } => Some(source),
            EventWriteError::Io(err) => Some(err),
            EventWriteError::Serialize(err) => Some(err),
            EventWriteError::InvalidFileComponent { .. } => None,
        }
    }
}

impl From<std::io::Error> for EventWriteError {
    fn from(value: std::io::Error) -> Self {
        EventWriteError::Io(value)
    }
}

impl From<serde_json::Error> for EventWriteError {
    fn from(value: serde_json::Error) -> Self {
        EventWriteError::Serialize(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_invalid_timestamp() {
        let bad = "not-a-date";
        let err = time::OffsetDateTime::parse(bad, &time::format_description::well_known::Rfc3339)
            .unwrap_err();
        let e = EventWriteError::InvalidTimestamp {
            value: bad.to_string(),
            source: err,
        };
        let msg = e.to_string();
        assert!(msg.contains("invalid RFC3339 timestamp"));
        assert!(msg.contains(bad));
        assert!(e.source().is_some());
    }

    #[test]
    fn display_invalid_file_component() {
        let e = EventWriteError::InvalidFileComponent {
            field: "knot_id",
            value: "has spaces".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("invalid knot_id"));
        assert!(msg.contains("has spaces"));
        assert!(e.source().is_none());
    }
}

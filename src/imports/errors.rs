use std::error::Error;
use std::fmt;

use crate::domain::state::ParseKnotStateError;
use crate::events::EventWriteError;

#[derive(Debug)]
pub enum ImportError {
    Io(std::io::Error),
    Db(rusqlite::Error),
    Json(serde_json::Error),
    Event(EventWriteError),
    ParseState(ParseKnotStateError),
    InvalidRecord(String),
    InvalidTimestamp(String),
    MissingDolt,
    CommandFailed(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(err) => write!(f, "I/O error: {}", err),
            ImportError::Db(err) => write!(f, "database error: {}", err),
            ImportError::Json(err) => write!(f, "JSON parse error: {}", err),
            ImportError::Event(err) => write!(f, "event write error: {}", err),
            ImportError::ParseState(err) => write!(f, "state parse error: {}", err),
            ImportError::InvalidRecord(message) => write!(f, "invalid source record: {}", message),
            ImportError::InvalidTimestamp(value) => {
                write!(f, "invalid --since timestamp '{}', expected RFC3339", value)
            }
            ImportError::MissingDolt => {
                write!(
                    f,
                    "dolt CLI is not installed; use `knots import jsonl` instead"
                )
            }
            ImportError::CommandFailed(message) => write!(f, "{}", message),
        }
    }
}

impl Error for ImportError {}

impl From<std::io::Error> for ImportError {
    fn from(value: std::io::Error) -> Self {
        ImportError::Io(value)
    }
}

impl From<rusqlite::Error> for ImportError {
    fn from(value: rusqlite::Error) -> Self {
        ImportError::Db(value)
    }
}

impl From<serde_json::Error> for ImportError {
    fn from(value: serde_json::Error) -> Self {
        ImportError::Json(value)
    }
}

impl From<EventWriteError> for ImportError {
    fn from(value: EventWriteError) -> Self {
        ImportError::Event(value)
    }
}

impl From<ParseKnotStateError> for ImportError {
    fn from(value: ParseKnotStateError) -> Self {
        ImportError::ParseState(value)
    }
}

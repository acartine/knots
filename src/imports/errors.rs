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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::domain::state::KnotState;
    use crate::events::EventWriteError;

    use super::ImportError;

    #[test]
    fn display_messages_cover_all_variants() {
        let io = ImportError::Io(std::io::Error::other("disk"));
        assert!(io.to_string().contains("I/O error"));

        let db = ImportError::Db(rusqlite::Error::InvalidQuery);
        assert!(db.to_string().contains("database error"));

        let json = ImportError::Json(
            serde_json::from_str::<serde_json::Value>("not-json").expect_err("json should fail"),
        );
        assert!(json.to_string().contains("JSON parse error"));

        let event = ImportError::Event(EventWriteError::Io(std::io::Error::other("event")));
        assert!(event.to_string().contains("event write error"));

        let parse = ImportError::ParseState(
            KnotState::from_str("not-a-state").expect_err("parse should fail"),
        );
        assert!(parse.to_string().contains("state parse error"));

        let invalid = ImportError::InvalidRecord("bad record".to_string());
        assert!(invalid.to_string().contains("invalid source record"));

        let timestamp = ImportError::InvalidTimestamp("bad-ts".to_string());
        assert!(timestamp.to_string().contains("expected RFC3339"));
    }

    #[test]
    fn from_conversions_map_to_expected_variants() {
        let io: ImportError = std::io::Error::other("io").into();
        assert!(matches!(io, ImportError::Io(_)));

        let db: ImportError = rusqlite::Error::InvalidQuery.into();
        assert!(matches!(db, ImportError::Db(_)));

        let json: ImportError = serde_json::from_str::<serde_json::Value>("bad")
            .expect_err("json parse should fail")
            .into();
        assert!(matches!(json, ImportError::Json(_)));

        let event: ImportError = EventWriteError::Io(std::io::Error::other("event")).into();
        assert!(matches!(event, ImportError::Event(_)));

        let parse_state: ImportError = KnotState::from_str("bad-state")
            .expect_err("state parse should fail")
            .into();
        assert!(matches!(parse_state, ImportError::ParseState(_)));
    }
}

use std::collections::BTreeMap;

use serde_json::Value;

use super::{ActiveCompactionError, ExactObjectReader, ObjectKind, RawEvent};

pub(super) struct SourceGeneration {
    pub events: Vec<RawEvent>,
    pub retained: BTreeMap<String, Vec<u8>>,
}

pub(super) fn read_source_generation(
    provider: &impl ExactObjectReader,
    source_commit: &str,
) -> Result<SourceGeneration, ActiveCompactionError> {
    let mut events = Vec::new();
    let mut retained = BTreeMap::new();
    for object in provider.read_commit_tree(source_commit)? {
        let event_path =
            object.path.starts_with(".knots/events/") || object.path.starts_with(".knots/index/");
        if event_path {
            if object.kind != ObjectKind::Blob || !object.path.ends_with(".json") {
                return Err(ActiveCompactionError::Invalid(format!(
                    "event source is not a JSON blob: {}",
                    object.path
                )));
            }
            let value: Value = serde_json::from_slice(&object.bytes)?;
            let event_id = value
                .get("event_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ActiveCompactionError::Invalid(format!(
                        "event has no event_id: {}",
                        object.path
                    ))
                })?;
            events.push(RawEvent {
                path: object.path,
                event_id: event_id.to_string(),
                bytes: object.bytes,
            });
        } else if object.path.starts_with(".knots/") {
            if object.kind != ObjectKind::Blob {
                return Err(ActiveCompactionError::Invalid(format!(
                    "sync source contains a non-blob object: {}",
                    object.path
                )));
            }
            retained.insert(object.path, object.bytes);
        }
    }
    events.sort();
    Ok(SourceGeneration { events, retained })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compaction::{RuntimeError, TreeObject};

    struct Reader(Vec<TreeObject>);

    impl ExactObjectReader for Reader {
        fn resolve_ref(&self, _remote_ref: &str) -> Result<Option<String>, RuntimeError> {
            Ok(None)
        }

        fn read_commit_tree(&self, _commit_oid: &str) -> Result<Vec<TreeObject>, RuntimeError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn exact_source_rejects_non_json_missing_ids_and_non_blobs() {
        for object in [
            object(".knots/events/bad.txt", ObjectKind::Blob, b"{}"),
            object(".knots/events/bad.json", ObjectKind::Blob, b"{}"),
            object(".knots/v2/link", ObjectKind::Symlink, b"target"),
        ] {
            assert!(read_source_generation(&Reader(vec![object]), "a").is_err());
        }
    }

    fn object(path: &str, kind: ObjectKind, bytes: &[u8]) -> TreeObject {
        TreeObject {
            path: path.to_string(),
            kind,
            oid: "a".repeat(40),
            bytes: bytes.to_vec(),
        }
    }
}

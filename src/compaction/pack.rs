use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::io::Cursor;

use sha2::{Digest, Sha256};

use super::{EventIndexEntry, EventPack, CANONICAL_ROOT};

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RawEvent {
    pub path: String,
    pub event_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct BuiltPack {
    pub descriptor: EventPack,
    pub compressed: Vec<u8>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum PackError {
    DuplicateEvent,
    InvalidLength,
    Compression,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol-v2 pack error: {self:?}")
    }
}

impl Error for PackError {}

pub(crate) fn build_pack(events: &[RawEvent]) -> Result<BuiltPack, PackError> {
    let mut ordered = events.to_vec();
    ordered
        .sort_by(|left, right| (&left.event_id, &left.path).cmp(&(&right.event_id, &right.path)));
    let mut event_ids = HashSet::new();
    for event in &ordered {
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(PackError::DuplicateEvent);
        }
    }
    let mut decoded = Vec::new();
    let mut index = Vec::with_capacity(ordered.len());
    for event in ordered {
        append_len(&mut decoded, event.path.len())?;
        decoded.extend_from_slice(event.path.as_bytes());
        append_len(&mut decoded, event.event_id.len())?;
        decoded.extend_from_slice(event.event_id.as_bytes());
        append_len(&mut decoded, event.bytes.len())?;
        let offset = decoded.len() as u64;
        decoded.extend_from_slice(&event.bytes);
        index.push(EventIndexEntry {
            event_id: event.event_id,
            content_sha256: digest(&event.bytes),
            offset,
            bytes: event.bytes.len() as u64,
        });
    }
    let compressed =
        zstd::stream::encode_all(Cursor::new(&decoded), 19).map_err(|_| PackError::Compression)?;
    let sha256 = digest(&compressed);
    let pack_id = format!("pack-{sha256}");
    Ok(BuiltPack {
        descriptor: EventPack {
            path: format!("{CANONICAL_ROOT}/packs/{pack_id}.pack.zst"),
            pack_id,
            sha256,
            bytes: compressed.len() as u64,
            decoded_bytes: decoded.len() as u64,
            events: index,
        },
        compressed,
    })
}

fn append_len(target: &mut Vec<u8>, value: usize) -> Result<(), PackError> {
    let value = u64::try_from(value).map_err(|_| PackError::InvalidLength)?;
    target.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
pub(crate) fn fixture_pack() -> BuiltPack {
    build_pack(&[RawEvent {
        path: ".knots/v2/inbox/events/event-1.json".to_string(),
        event_id: "event-1".to_string(),
        bytes: br#"{"event_id":"event-1","body":"raw bytes retained"}"#.to_vec(),
    }])
    .expect("pack builds")
}

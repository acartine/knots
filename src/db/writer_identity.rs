use std::error::Error;
use std::fmt;

use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use super::now_utc_rfc3339;
use crate::compaction::V2RefLayout;

const ALGORITHM: &str = "ed25519";

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct WriterIdentity {
    pub writer_id: String,
    pub public_key: [u8; 32],
    pub public_key_sha256: String,
    pub generation: u64,
    pub parent_writer_id: Option<String>,
}

impl fmt::Debug for WriterIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriterIdentity")
            .field("writer_id", &self.writer_id)
            .field("public_key_sha256", &self.public_key_sha256)
            .field("generation", &self.generation)
            .field("parent_writer_id", &self.parent_writer_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(crate) enum WriterIdentityError {
    Database(rusqlite::Error),
    MetadataMismatch,
    GenerationOverflow,
}

impl fmt::Display for WriterIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protocol-v2 writer identity error: {self:?}")
    }
}

impl Error for WriterIdentityError {}

impl From<rusqlite::Error> for WriterIdentityError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

pub(crate) fn ensure_writer_identity(
    conn: &Connection,
) -> Result<WriterIdentity, WriterIdentityError> {
    ensure_writer_identity_for_credential(conn, None)
}

pub(crate) fn ensure_writer_identity_for_credential(
    conn: &Connection,
    credential_id: Option<&str>,
) -> Result<WriterIdentity, WriterIdentityError> {
    if let Some(identity) = active_identity(conn)? {
        validate_identity(conn, &identity)?;
        let tx = conn.unchecked_transaction()?;
        migrate_writer_epoch(&tx, &identity.writer_id, credential_id, false)?;
        tx.commit()?;
        return Ok(identity);
    }
    let signing = SigningKey::generate(&mut OsRng);
    let tx = conn.unchecked_transaction()?;
    let inserted = insert_identity(&tx, &signing, 1, None, true)?;
    let identity = if inserted {
        identity_from_signing(&signing, 1, None)
    } else {
        active_identity(&tx)?.ok_or(WriterIdentityError::MetadataMismatch)?
    };
    migrate_writer_epoch(&tx, &identity.writer_id, credential_id, false)?;
    tx.commit()?;
    Ok(identity)
}

pub(crate) fn rotate_writer_identity(
    conn: &Connection,
) -> Result<WriterIdentity, WriterIdentityError> {
    rotate_writer_identity_for_credential(conn, None)
}

pub(crate) fn rotate_writer_identity_for_credential(
    conn: &Connection,
    credential_id: Option<&str>,
) -> Result<WriterIdentity, WriterIdentityError> {
    let prior = active_identity(conn)?.ok_or(WriterIdentityError::MetadataMismatch)?;
    validate_identity(conn, &prior)?;
    let generation = prior
        .generation
        .checked_add(1)
        .ok_or(WriterIdentityError::GenerationOverflow)?;
    let signing = SigningKey::generate(&mut OsRng);
    let next = identity_from_signing(&signing, generation, Some(prior.writer_id.clone()));
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE v2_writer_identity SET active = 0, retired_at = ?1 WHERE active = 1",
        params![now_utc_rfc3339()],
    )?;
    if !insert_identity(&tx, &signing, generation, Some(&prior.writer_id), false)? {
        return Err(WriterIdentityError::MetadataMismatch);
    }
    migrate_writer_epoch(&tx, &next.writer_id, credential_id, true)?;
    tx.commit()?;
    Ok(next)
}

pub(crate) fn load_writer_signing_key(
    conn: &Connection,
    writer_id: &str,
) -> Result<SigningKey, WriterIdentityError> {
    let seed = conn
        .query_row(
            "SELECT private_seed FROM v2_writer_identity WHERE writer_id = ?1",
            [writer_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .ok_or(WriterIdentityError::MetadataMismatch)?;
    let bytes = seed
        .try_into()
        .map_err(|_| WriterIdentityError::MetadataMismatch)?;
    let signing = SigningKey::from_bytes(&bytes);
    let identity = identity_by_id(conn, writer_id)?;
    verify_signing_key(&identity, &signing)?;
    Ok(signing)
}

fn insert_identity(
    conn: &Connection,
    signing: &SigningKey,
    generation: u64,
    parent: Option<&str>,
    ignore_conflict: bool,
) -> Result<bool, WriterIdentityError> {
    let identity = identity_from_signing(signing, generation, parent.map(str::to_string));
    let verb = if ignore_conflict { "OR IGNORE" } else { "" };
    let sql = format!(
        "INSERT {verb} INTO v2_writer_identity \
         (writer_id, algorithm, public_key, public_key_sha256, private_seed, generation, \
          parent_writer_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
    );
    let changed = conn.execute(
        &sql,
        params![
            identity.writer_id,
            ALGORITHM,
            identity.public_key.as_slice(),
            identity.public_key_sha256,
            signing.to_bytes().as_slice(),
            sqlite_generation(generation)?,
            parent,
            now_utc_rfc3339(),
        ],
    )?;
    Ok(changed == 1)
}

fn migrate_writer_epoch(
    conn: &Connection,
    writer_id: &str,
    credential_override: Option<&str>,
    replace_credential: bool,
) -> Result<(), WriterIdentityError> {
    let legacy = conn
        .query_row(
            "SELECT writer_id, credential_id FROM v2_writer_epoch WHERE active = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((legacy_id, stored_credential_id)) = legacy else {
        if let Some(credential_id) = credential_override {
            insert_writer_epoch(conn, writer_id, credential_id)?;
        }
        return Ok(());
    };
    let credential_id = if replace_credential {
        credential_override.unwrap_or(&stored_credential_id)
    } else {
        &stored_credential_id
    };
    if legacy_id == writer_id && credential_id == stored_credential_id {
        return Ok(());
    }
    conn.execute(
        "UPDATE v2_writer_epoch SET active = 0 WHERE writer_id = ?1",
        [&legacy_id],
    )?;
    insert_writer_epoch(conn, writer_id, credential_id)?;
    conn.execute(
        "UPDATE v2_outbox SET writer_id = NULL, sequence = NULL, proposed_inbox_oid = NULL \
         WHERE writer_id = ?1 AND acknowledged_at IS NULL",
        [&legacy_id],
    )?;
    Ok(())
}

fn insert_writer_epoch(
    conn: &Connection,
    writer_id: &str,
    credential_id: &str,
) -> Result<(), WriterIdentityError> {
    conn.execute(
        "INSERT INTO v2_writer_epoch \
         (writer_id, credential_id, inbox_ref, created_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(writer_id) DO UPDATE SET \
         credential_id = excluded.credential_id, inbox_ref = excluded.inbox_ref, active = 1",
        params![
            writer_id,
            credential_id,
            V2RefLayout::default().inbox(writer_id),
            now_utc_rfc3339(),
        ],
    )?;
    Ok(())
}

fn active_identity(conn: &Connection) -> Result<Option<WriterIdentity>, WriterIdentityError> {
    query_identity(conn, "WHERE active = 1", [])
}

fn identity_by_id(
    conn: &Connection,
    writer_id: &str,
) -> Result<WriterIdentity, WriterIdentityError> {
    query_identity(conn, "WHERE writer_id = ?1", [writer_id])?
        .ok_or(WriterIdentityError::MetadataMismatch)
}

fn query_identity<const N: usize>(
    conn: &Connection,
    predicate: &str,
    values: [&str; N],
) -> Result<Option<WriterIdentity>, WriterIdentityError> {
    let sql = format!(
        "SELECT writer_id, algorithm, public_key, public_key_sha256, generation, parent_writer_id \
         FROM v2_writer_identity {predicate}"
    );
    let row = conn
        .query_row(&sql, rusqlite::params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })
        .optional()?;
    row.map(
        |(writer_id, algorithm, public, fingerprint, generation, parent_writer_id)| {
            if algorithm != ALGORITHM {
                return Err(WriterIdentityError::MetadataMismatch);
            }
            Ok(WriterIdentity {
                writer_id,
                public_key: public
                    .try_into()
                    .map_err(|_| WriterIdentityError::MetadataMismatch)?,
                public_key_sha256: fingerprint,
                generation: u64::try_from(generation)
                    .map_err(|_| WriterIdentityError::MetadataMismatch)?,
                parent_writer_id,
            })
        },
    )
    .transpose()
}

fn validate_identity(
    conn: &Connection,
    identity: &WriterIdentity,
) -> Result<(), WriterIdentityError> {
    let signing = load_writer_signing_key(conn, &identity.writer_id)?;
    verify_signing_key(identity, &signing)
}

fn verify_signing_key(
    identity: &WriterIdentity,
    signing: &SigningKey,
) -> Result<(), WriterIdentityError> {
    let public = signing.verifying_key().to_bytes();
    let fingerprint = format!("{:x}", Sha256::digest(public));
    if public != identity.public_key
        || fingerprint != identity.public_key_sha256
        || identity.writer_id != fingerprint
    {
        return Err(WriterIdentityError::MetadataMismatch);
    }
    Ok(())
}

fn identity_from_signing(
    signing: &SigningKey,
    generation: u64,
    parent_writer_id: Option<String>,
) -> WriterIdentity {
    let public_key = signing.verifying_key().to_bytes();
    let fingerprint = format!("{:x}", Sha256::digest(public_key));
    WriterIdentity {
        writer_id: fingerprint.clone(),
        public_key,
        public_key_sha256: fingerprint,
        generation,
        parent_writer_id,
    }
}

fn sqlite_generation(generation: u64) -> Result<i64, rusqlite::Error> {
    i64::try_from(generation)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;

    #[test]
    fn identity_reopens_rotates_and_never_formats_secret() {
        let ws = knots_test_support::workspace("writer-identity");
        let path = ws.path().join("state.sqlite");
        let first = {
            let conn = crate::db::open_connection(path.to_str().unwrap()).unwrap();
            ensure_writer_identity(&conn).unwrap()
        };
        let conn = crate::db::open_connection(path.to_str().unwrap()).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
            0
        );
        assert_eq!(ensure_writer_identity(&conn).unwrap(), first);
        let seed: Vec<u8> = conn
            .query_row(
                "SELECT private_seed FROM v2_writer_identity WHERE writer_id = ?1",
                [&first.writer_id],
                |row| row.get(0),
            )
            .unwrap();
        let seed_hex = seed
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(!format!("{first:?}").contains(&seed_hex));
        let signing = load_writer_signing_key(&conn, &first.writer_id).unwrap();
        assert!(!format!("{signing:?}").contains(&seed_hex));
        assert!(!WriterIdentityError::MetadataMismatch
            .to_string()
            .contains(&seed_hex));
        let second = rotate_writer_identity(&conn).unwrap();
        assert_eq!(
            second.parent_writer_id.as_deref(),
            Some(first.writer_id.as_str())
        );
        assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM v2_writer_identity"), 2);
        conn.execute(
            "UPDATE v2_writer_identity SET algorithm = 'not-ed25519' WHERE active = 1",
            [],
        )
        .unwrap();
        assert!(ensure_writer_identity(&conn).is_err());
    }

    #[test]
    fn rotation_failure_rolls_back_after_reopen() {
        let ws = knots_test_support::workspace("writer-identity-rollback");
        let path = ws.path().join("state.sqlite");
        let conn = crate::db::open_connection(path.to_str().unwrap()).unwrap();
        let first = ensure_writer_identity(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_rotation BEFORE INSERT ON v2_writer_identity \
             WHEN NEW.generation > 1 BEGIN SELECT RAISE(ABORT, 'crash'); END;",
        )
        .unwrap();
        assert!(rotate_writer_identity(&conn).is_err());
        drop(conn);
        let reopened = crate::db::open_connection(path.to_str().unwrap()).unwrap();
        assert_eq!(ensure_writer_identity(&reopened).unwrap(), first);
        assert_eq!(
            scalar(&reopened, "SELECT COUNT(*) FROM v2_writer_identity"),
            1
        );
    }

    #[test]
    fn concurrent_creation_converges_and_uuid_pending_rows_survive() {
        let ws = knots_test_support::workspace("writer-identity-concurrent");
        let path = ws.path().join("state.sqlite");
        crate::db::open_connection(path.to_str().unwrap()).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let conn = crate::db::open_connection(path.to_str().unwrap()).unwrap();
                    barrier.wait();
                    ensure_writer_identity(&conn).unwrap()
                })
            })
            .collect::<Vec<_>>();
        let identities = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(identities[0], identities[1]);

        let conn = crate::db::open_connection(path.to_str().unwrap()).unwrap();
        conn.execute("DELETE FROM v2_writer_identity", []).unwrap();
        seed_uuid_pending(&conn);
        let identity = ensure_writer_identity(&conn).unwrap();
        assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM v2_outbox"), 1);
        let pending_writer: Option<String> = conn
            .query_row("SELECT writer_id FROM v2_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(pending_writer, None);
        assert_eq!(active_epoch_id(&conn), identity.writer_id);
        let rotated = rotate_writer_identity(&conn).unwrap();
        assert_eq!(
            rotated.parent_writer_id.as_deref(),
            Some(identity.writer_id.as_str())
        );

        conn.execute(
            "UPDATE v2_writer_epoch SET active = 0 WHERE writer_id = ?1",
            [&rotated.writer_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO v2_writer_epoch \
             (writer_id,credential_id,inbox_ref,created_at) \
             VALUES ('uuid-new','cred-new','new','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO v2_outbox \
             (event_id,stream,relative_path,content_sha256,payload,writer_id,sequence,created_at) \
             VALUES ('event-new','full','events/new.json','hash-new',X'03','uuid-new',1,'now')",
            [],
        )
        .unwrap();
        drop(conn);
        let reopened = crate::db::open_connection(path.to_str().unwrap()).unwrap();
        assert_eq!(ensure_writer_identity(&reopened).unwrap(), rotated);
        assert_eq!(scalar(&reopened, "SELECT COUNT(*) FROM v2_outbox"), 2);
        assert_eq!(
            scalar(
                &reopened,
                "SELECT COUNT(*) FROM v2_outbox WHERE writer_id IS NOT NULL"
            ),
            0
        );
        assert_eq!(active_epoch_id(&reopened), rotated.writer_id);
    }

    fn seed_uuid_pending(conn: &Connection) {
        conn.execute(
            "INSERT INTO v2_writer_epoch \
             (writer_id, credential_id, inbox_ref, created_at) \
             VALUES ('uuid-old','cred','old','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO v2_outbox \
             (event_id,stream,relative_path,content_sha256,payload,writer_id,sequence,created_at) \
             VALUES ('event','full','events/event.json','hash',X'0102','uuid-old',1,'now')",
            [],
        )
        .unwrap();
    }

    fn scalar(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn active_epoch_id(conn: &Connection) -> String {
        conn.query_row(
            "SELECT writer_id FROM v2_writer_epoch WHERE active = 1",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }
}

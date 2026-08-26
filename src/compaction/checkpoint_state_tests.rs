use std::collections::HashSet;

use super::checkpoint_tests::fixture;
use super::{install_checkpoint, prepare_checkpoint, CheckpointError};
use crate::db;

#[test]
fn stale_install_fails_and_existing_quarantine_baseline_survives_replacement() {
    let conn = db::open_connection(":memory:").unwrap();
    let fixture = fixture();
    let prepared = prepare_checkpoint(fixture.input()).unwrap();
    conn.execute(
        "INSERT INTO v2_generation_quarantine VALUES ('held', 'older', '[]', 'then')",
        [],
    )
    .unwrap();
    install_checkpoint(&conn, &prepared, &HashSet::new(), |_| Ok(())).unwrap();
    assert!(matches!(
        install_checkpoint(&conn, &prepared, &HashSet::new(), |_| Ok(())),
        Err(CheckpointError::Control(_))
    ));
    let original = baseline(&conn);
    conn.execute("DELETE FROM v2_checkpoint", []).unwrap();
    install_checkpoint(&conn, &prepared, &HashSet::new(), |_| Ok(())).unwrap();
    assert_eq!(baseline(&conn), original);
}

fn baseline(conn: &rusqlite::Connection) -> (String, String, String) {
    conn.query_row(
        "SELECT base_generation_id, writer_vector_json, quarantined_at \
         FROM v2_generation_quarantine WHERE knot_id = 'held'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .unwrap()
}

use sha2::{Digest, Sha256};

use super::*;

const COMMIT: &str = "1111111111111111111111111111111111111111";
const CONTROL_HEAD: &str = "4444444444444444444444444444444444444444";
const NEXT_CONTROL_HEAD: &str = "5555555555555555555555555555555555555555";
const GENERATION: &str = "v2-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn writer() -> WriterHead {
    WriterHead {
        writer_id: "writer-a".to_string(),
        inbox_ref: V2RefLayout::default().inbox("writer-a"),
        commit: COMMIT.to_string(),
        sequence: 5,
    }
}

fn control(action: ControlKind) -> ControlRecord {
    ControlRecord {
        schema_version: 1,
        epoch: 2,
        previous_control_head: Some(CONTROL_HEAD.to_string()),
        active_generation_id: GENERATION.to_string(),
        active_generation_commit: COMMIT.to_string(),
        archive_ref: V2RefLayout::default().archive(GENERATION),
        acknowledged_writer_heads: vec![writer()],
        protection_policy_sha256: format!("{:x}", Sha256::digest(b"provider policy")),
        action,
    }
}

fn model() -> ProtocolModel {
    let mut initial = control(ControlKind::Activation);
    initial.epoch = 1;
    initial.previous_control_head = None;
    ProtocolModel::with_active(CONTROL_HEAD, initial)
}

#[test]
fn recovery_rejects_wrong_action_target_and_writer_vector() {
    let mut model = model();
    assert_eq!(
        model.recover(
            CONTROL_HEAD,
            NEXT_CONTROL_HEAD,
            control(ControlKind::Activation),
        ),
        Err(ProtocolError::InvalidRecovery)
    );

    let wrong_target = control(ControlKind::Recovery {
        rollback_of_epoch: 0,
    });
    assert_eq!(
        model.recover(CONTROL_HEAD, NEXT_CONTROL_HEAD, wrong_target),
        Err(ProtocolError::InvalidRecovery)
    );

    let mut missing_writer = control(ControlKind::Recovery {
        rollback_of_epoch: 1,
    });
    missing_writer.acknowledged_writer_heads.clear();
    assert_eq!(
        model.recover(CONTROL_HEAD, NEXT_CONTROL_HEAD, missing_writer),
        Err(ProtocolError::WriterHeadRegressed)
    );

    let mut invalid_writer = control(ControlKind::Recovery {
        rollback_of_epoch: 1,
    });
    invalid_writer.acknowledged_writer_heads[0]
        .writer_id
        .clear();
    assert_eq!(
        model.recover(CONTROL_HEAD, NEXT_CONTROL_HEAD, invalid_writer),
        Err(ProtocolError::InvalidControlRecord)
    );
}

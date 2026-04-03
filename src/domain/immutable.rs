use super::lease::LeaseReference;

pub trait ImmutableRecord {
    fn record_id(&self) -> &str;
    fn created_at(&self) -> &str;
    fn lease_ref(&self) -> Option<&LeaseReference>;
}

pub fn ensure_append_only_records<T>(existing: &[T], next: &[T], kind: &str) -> Result<(), String>
where
    T: ImmutableRecord + PartialEq,
{
    if next.len() < existing.len() {
        return Err(append_only_error(kind));
    }

    if let Some(record) = existing
        .iter()
        .zip(next.iter())
        .find_map(|(left, right)| (left != right).then_some(left))
    {
        return Err(append_only_error_with_record(kind, record));
    }

    Ok(())
}

fn append_only_error(kind: &str) -> String {
    format!(
        "cannot modify or delete existing {kind} records; metadata is append-only. \
         Add a new record instead."
    )
}

fn append_only_error_with_record(kind: &str, record: &dyn ImmutableRecord) -> String {
    let lease_context = record
        .lease_ref()
        .map(|lease| format!(" lease={}", lease.knot_id))
        .unwrap_or_default();
    format!(
        "{} Conflicting record id={} created_at={}{}.",
        append_only_error(kind),
        record.record_id(),
        record.created_at(),
        lease_context
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Eq)]
    struct DummyRecord {
        id: &'static str,
        created_at: &'static str,
        lease_ref: Option<LeaseReference>,
    }

    impl ImmutableRecord for DummyRecord {
        fn record_id(&self) -> &str {
            self.id
        }

        fn created_at(&self) -> &str {
            self.created_at
        }

        fn lease_ref(&self) -> Option<&LeaseReference> {
            self.lease_ref.as_ref()
        }
    }

    #[test]
    fn append_only_accepts_identical_prefix_with_new_records() {
        let existing = vec![DummyRecord {
            id: "r1",
            created_at: "2026-04-03T12:00:00Z",
            lease_ref: None,
        }];
        let next = vec![
            existing[0].clone(),
            DummyRecord {
                id: "r2",
                created_at: "2026-04-03T12:01:00Z",
                lease_ref: LeaseReference::new("knots-lease").ok(),
            },
        ];
        assert!(ensure_append_only_records(&existing, &next, "note").is_ok());
    }

    #[test]
    fn append_only_rejects_mutation() {
        let existing = vec![DummyRecord {
            id: "r1",
            created_at: "2026-04-03T12:00:00Z",
            lease_ref: None,
        }];
        let next = vec![DummyRecord {
            id: "r1",
            created_at: "2026-04-03T12:00:00Z",
            lease_ref: LeaseReference::new("knots-lease").ok(),
        }];
        let err = ensure_append_only_records(&existing, &next, "step history").unwrap_err();
        assert!(err.contains("append-only"));
    }
}

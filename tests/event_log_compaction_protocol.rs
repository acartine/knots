//! Executable safety oracle for the event-log compaction design.
//!
//! This is deliberately a small protocol model, not production code. It makes
//! the proposed invariants executable before the implementation is split into
//! independently shippable knots.

use std::collections::{BTreeMap, BTreeSet};

const COMPACTION_AWARE_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    id: u64,
    knot: &'static str,
    value: &'static str,
    generation: u64,
}

#[derive(Clone, Debug)]
struct Generation {
    cutoff: u64,
    snapshot: BTreeMap<&'static str, &'static str>,
    parent: Option<u64>,
    rollback_events: Option<Vec<Event>>,
}

#[derive(Debug, PartialEq, Eq)]
enum PushError {
    MixedVersion,
    StaleGeneration,
    CoveredByCheckpoint,
}

#[derive(Default)]
struct Remote {
    events: Vec<Event>,
    generations: BTreeMap<u64, Generation>,
    candidate: Option<u64>,
    active: Option<u64>,
}

impl Remote {
    fn append(&mut self, event: Event) {
        self.events.push(event);
        self.events.sort_by_key(|item| item.id);
    }

    fn state(&self) -> BTreeMap<&'static str, &'static str> {
        let mut state = self
            .active
            .and_then(|id| self.generations.get(&id))
            .map(|generation| generation.snapshot.clone())
            .unwrap_or_default();
        for event in &self.events {
            state.insert(event.knot, event.value);
        }
        state
    }

    fn prepare(&mut self, id: u64, cutoff: u64) {
        let mut snapshot = self
            .active
            .and_then(|active| self.generations.get(&active))
            .map(|generation| generation.snapshot.clone())
            .unwrap_or_default();
        for event in self.events.iter().filter(|event| event.id <= cutoff) {
            snapshot.insert(event.knot, event.value);
        }
        self.generations.insert(
            id,
            Generation {
                cutoff,
                snapshot,
                parent: self.active,
                rollback_events: None,
            },
        );
        self.candidate = Some(id);
    }

    fn activate(&mut self, id: u64) {
        assert_eq!(
            self.candidate,
            Some(id),
            "only a prepared generation activates"
        );
        let cutoff = self.generations[&id].cutoff;
        let backup = self.events.clone();
        self.generations
            .get_mut(&id)
            .expect("prepared generation exists")
            .rollback_events = Some(backup);
        self.events.retain(|event| event.id > cutoff);
        self.active = Some(id);
        self.candidate = None;
    }

    fn rollback(&mut self) {
        let active = self.active.expect("an active generation is required");
        let generation = self.generations.get(&active).expect("generation exists");
        self.events = generation
            .rollback_events
            .clone()
            .expect("the prior tree remains available during the rollback window");
        self.active = generation.parent;
    }

    fn push(&mut self, version: u32, events: &[Event]) -> Result<(), PushError> {
        let Some(active) = self.active else {
            self.events.extend_from_slice(events);
            return Ok(());
        };
        if version < COMPACTION_AWARE_VERSION {
            return Err(PushError::MixedVersion);
        }
        let cutoff = self.generations[&active].cutoff;
        for event in events {
            if event.id <= cutoff {
                return Err(PushError::CoveredByCheckpoint);
            }
            if event.generation != active {
                return Err(PushError::StaleGeneration);
            }
        }
        self.events.extend_from_slice(events);
        self.events.sort_by_key(|event| event.id);
        Ok(())
    }

    fn revision(&self) -> (Option<u64>, Option<u64>) {
        (self.active, self.events.iter().map(|event| event.id).max())
    }
}

#[derive(Default)]
struct Client {
    cache: BTreeMap<&'static str, &'static str>,
    revision: Option<(Option<u64>, Option<u64>)>,
    leased: BTreeSet<&'static str>,
    quarantined: BTreeMap<&'static str, &'static str>,
    local_events: Vec<Event>,
}

impl Client {
    fn sync(&mut self, remote: &Remote, prune_local: bool) -> usize {
        if prune_local {
            self.prune_local(remote);
        }
        if self.revision == Some(remote.revision()) && self.quarantined.is_empty() {
            return 0;
        }
        let target = remote.state();
        let mut applied = 0;
        for (knot, value) in target {
            if self.leased.contains(knot) {
                self.quarantined.insert(knot, value);
            } else if self.cache.insert(knot, value) != Some(value) {
                applied += 1;
            }
        }
        self.revision = Some(remote.revision());
        applied
    }

    fn release(&mut self, knot: &'static str) {
        self.leased.remove(knot);
        if let Some(value) = self.quarantined.remove(knot) {
            self.cache.insert(knot, value);
        }
    }

    fn prune_local(&mut self, remote: &Remote) {
        if let Some(active) = remote.active {
            let cutoff = remote.generations[&active].cutoff;
            self.local_events.retain(|event| event.id > cutoff);
        }
    }
}

fn event(id: u64, knot: &'static str, value: &'static str, generation: u64) -> Event {
    Event {
        id,
        knot,
        value,
        generation,
    }
}

fn activated_remote() -> Remote {
    let mut remote = Remote::default();
    remote.append(event(1, "K-1", "created", 0));
    remote.append(event(2, "K-1", "planned", 0));
    remote.append(event(3, "K-2", "ready", 0));
    remote.prepare(1, 3);
    remote.activate(1);
    remote
}

#[test]
fn fresh_bootstrap_uses_checkpoint_then_retained_events() {
    let mut remote = activated_remote();
    remote.append(event(4, "K-1", "implemented", 1));
    let mut client = Client::default();

    assert_eq!(client.sync(&remote, true), 2);
    assert_eq!(client.cache, remote.state());
    assert_eq!(client.cache["K-1"], "implemented");
}

#[test]
fn pre_cutoff_watermark_rebases_through_checkpoint() {
    let remote = activated_remote();
    let mut client = Client::default();
    client.cache.insert("K-1", "created");
    client.revision = Some((None, Some(1)));

    assert_eq!(client.sync(&remote, true), 2);
    assert_eq!(client.cache, remote.state());
}

#[test]
fn lease_quarantine_preserves_local_projection_until_release() {
    let remote = activated_remote();
    let mut client = Client::default();
    client.cache.insert("K-1", "local in-flight work");
    client.leased.insert("K-1");

    client.sync(&remote, true);
    assert_eq!(client.cache["K-1"], "local in-flight work");
    assert_eq!(client.quarantined["K-1"], "planned");
    client.release("K-1");
    assert_eq!(client.cache["K-1"], "planned");
}

#[test]
fn stale_writer_cannot_resurrect_covered_events() {
    let mut remote = activated_remote();
    let stale = event(2, "K-1", "planned", 1);

    assert_eq!(
        remote.push(COMPACTION_AWARE_VERSION, &[stale]),
        Err(PushError::CoveredByCheckpoint)
    );
    assert!(remote.events.is_empty());
}

#[test]
fn concurrent_writer_after_cutoff_survives_activation() {
    let mut remote = Remote::default();
    remote.append(event(1, "K-1", "created", 0));
    remote.prepare(1, 1);
    remote.append(event(2, "K-1", "concurrent", 0));

    remote.activate(1);
    assert_eq!(remote.events, vec![event(2, "K-1", "concurrent", 0)]);
    assert_eq!(remote.state()["K-1"], "concurrent");
}

#[test]
fn interruption_before_activation_keeps_previous_protocol_live() {
    let mut remote = Remote::default();
    remote.append(event(1, "K-1", "created", 0));
    remote.prepare(1, 1);

    assert_eq!(remote.active, None);
    assert_eq!(remote.state()["K-1"], "created");
    assert_eq!(remote.events.len(), 1);
}

#[test]
fn interruption_after_activation_recovers_local_pruning_on_next_sync() {
    let remote = activated_remote();
    let mut client = Client {
        local_events: vec![event(1, "K-1", "created", 0)],
        ..Client::default()
    };

    client.sync(&remote, false);
    assert_eq!(client.local_events.len(), 1, "local prune was interrupted");
    assert_eq!(client.sync(&remote, true), 0);
    assert!(client.local_events.is_empty());
}

#[test]
fn mixed_version_writer_is_rejected_after_activation() {
    let mut remote = activated_remote();
    let event = event(4, "K-3", "legacy write", 0);

    assert_eq!(remote.push(1, &[event]), Err(PushError::MixedVersion));
}

#[test]
fn rollback_restores_the_previous_complete_generation() {
    let mut remote = Remote::default();
    remote.append(event(1, "K-1", "created", 0));
    remote.prepare(1, 1);
    remote.activate(1);
    remote.append(event(2, "K-1", "planned", 1));
    remote.prepare(2, 2);
    remote.activate(2);

    remote.rollback();
    assert_eq!(remote.active, Some(1));
    assert_eq!(remote.state()["K-1"], "planned");
}

#[test]
fn second_sync_is_a_no_op() {
    let remote = activated_remote();
    let mut client = Client::default();

    assert!(client.sync(&remote, true) > 0);
    assert_eq!(client.sync(&remote, true), 0);
}

//! Virtual time is ordered by (time, sequence); never consults the host clock.
use raft_protocol::{DurableState, Snapshot, adopt_snapshot};
use std::collections::BTreeMap;

mod cluster;
pub use cluster::{Campaign, Cluster, Oracle, SLOTS, TraceRow, run_campaign};

pub struct Random(u64);
impl Random {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(0x9e3779b97f4a7c15))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}
pub struct Scheduler<T> {
    pub now: u64,
    sequence: u64,
    queue: BTreeMap<(u64, u64), T>,
}
impl<T> Default for Scheduler<T> {
    fn default() -> Self {
        Self {
            now: 0,
            sequence: 0,
            queue: BTreeMap::new(),
        }
    }
}
impl<T> Scheduler<T> {
    pub fn drop_next(&mut self) -> bool {
        self.queue.pop_first().is_some()
    }
    pub fn delay_next(&mut self, delay: u64) -> bool {
        if let Some(((time, _), value)) = self.queue.pop_first() {
            self.sequence += 1;
            self.queue.insert(
                (
                    time.checked_add(delay).expect("virtual time overflow"),
                    self.sequence,
                ),
                value,
            );
            true
        } else {
            false
        }
    }
    pub fn duplicate_next(&mut self) -> bool
    where
        T: Clone,
    {
        if let Some((&(time, _), value)) = self.queue.first_key_value() {
            let value = value.clone();
            self.sequence += 1;
            self.queue.insert((time, self.sequence), value);
            true
        } else {
            false
        }
    }
    pub fn schedule(&mut self, delay: u64, value: T) {
        self.sequence += 1;
        self.queue.insert(
            (
                self.now.checked_add(delay).expect("virtual time overflow"),
                self.sequence,
            ),
            value,
        );
    }
    pub fn pop(&mut self) -> Option<((u64, u64), T)> {
        let (key, value) = self.queue.pop_first()?;
        assert!(key.0 >= self.now);
        self.now = key.0;
        Some((key, value))
    }
}
#[derive(Clone, Debug, Default)]
pub struct MockDisk {
    pub volatile: DurableState,
    pub durable: DurableState,
    pub snapshot: Option<Snapshot>,
    pub fail_sync: bool,
}
impl MockDisk {
    pub fn write(&mut self, state: DurableState) {
        self.volatile = state;
    }
    pub fn sync(&mut self) -> Result<(), &'static str> {
        if self.fail_sync {
            Err("injected sync failure")
        } else {
            self.durable = self.volatile.clone();
            Ok(())
        }
    }
    /// docs/SNAPSHOT_COMPACTION.md §7.4: the snapshot becomes durable first, then
    /// the rewritten WAL. `stop_between` models a crash between the two steps.
    pub fn install(&mut self, state: DurableState, snapshot: Snapshot, stop_between: bool) {
        self.snapshot = Some(snapshot);
        if !stop_between {
            self.durable = state.clone();
            self.volatile = state;
        }
    }
    /// What storage recovery would hand to Core (§7.3), finishing an interrupted install.
    pub fn recover(&mut self) -> Result<DurableState, String> {
        let base = self.durable.base();
        match &self.snapshot {
            None if base.index > 0 => Err("disk: WAL base without a snapshot".into()),
            None => Ok(self.durable.clone()),
            Some(s) if base.index > s.base.index => Err("disk: WAL base after snapshot".into()),
            Some(s) if base.index == s.base.index && base.term != s.base.term => {
                Err("disk: WAL base term differs from snapshot".into())
            }
            Some(s) => {
                if base.index < s.base.index {
                    let mut next = adopt_snapshot(&self.durable, s.base);
                    // As raft-storage does: an install interrupted after it raised
                    // the term leaves a snapshot newer than the WAL's term.
                    if next.term < s.base.term {
                        next.term = s.base.term;
                        next.voted_for = None;
                    }
                    self.durable = next;
                    self.volatile = self.durable.clone();
                }
                Ok(self.durable.clone())
            }
        }
    }
    pub fn crash(&mut self, unsynced_survives: bool) {
        if unsynced_survives {
            self.durable = self.volatile.clone();
        }
        self.volatile = self.durable.clone();
    }
}

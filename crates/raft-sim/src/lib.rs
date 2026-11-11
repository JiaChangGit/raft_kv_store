//! Virtual time is ordered by (time, sequence); never consults the host clock.
use raft_protocol::DurableState;
use std::collections::BTreeMap;

mod cluster;
pub use cluster::{Campaign, Cluster, Oracle, TraceRow, run_campaign};

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
    pub fn crash(&mut self, unsynced_survives: bool) {
        if unsynced_survives {
            self.durable = self.volatile.clone();
        }
        self.volatile = self.durable.clone();
    }
}

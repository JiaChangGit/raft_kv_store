//! Independent observers check the production Core; no second election/replication implementation.
use crate::{MockDisk, Random, Scheduler};
use raft_core::{Core, Effect, Event, Role};
use raft_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Tiny chunks force every snapshot transfer to span several InstallSnapshot RPCs.
const SIM_CHUNK: usize = 7;

/// Stand-in state machine: the applied index and a CRC chained over every
/// applied entry. Two nodes agree on it exactly when they applied the same
/// prefix, so a snapshot of it can be checked against the observer's record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub index: u64,
    pub digest: u32,
}
impl Model {
    fn fold(digest: u32, entry: &LogEntry) -> u32 {
        let mut bytes = digest.to_le_bytes().to_vec();
        bytes.extend(serde_json::to_vec(entry).expect("entry serializes"));
        crc32c(&bytes)
    }
    fn apply(&mut self, entry: &LogEntry) -> Result<(), String> {
        if entry.index != self.index + 1 {
            return Err(format!("model apply gap at index {}", entry.index));
        }
        self.index = entry.index;
        self.digest = Self::fold(self.digest, entry);
        Ok(())
    }
    fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("model serializes")
    }
    fn decode(snapshot: &Snapshot) -> Result<Self, String> {
        let model: Self = serde_json::from_slice(&snapshot.data)
            .map_err(|e| format!("I6 snapshot payload undecodable: {e}"))?;
        if model.index != snapshot.base.index {
            return Err("I6 snapshot payload index differs from its base".into());
        }
        Ok(model)
    }
}

#[derive(Clone)]
enum Scheduled {
    Timer {
        node: usize,
        incarnation: u64,
        event: Event,
    },
    Deliver(Envelope),
    Restart(usize),
    Heal,
}
#[derive(Clone, Debug, Serialize)]
pub struct TraceRow {
    pub time: u64,
    pub node: u8,
    pub kind: String,
    pub term: u64,
    pub role: String,
    pub commit: u64,
    pub applied: u64,
    pub durable: u64,
}

/// Both logs hold indexes up to `end` and agree on every index both still hold.
fn agree_through(a: &DurableState, b: &DurableState, end: u64) -> bool {
    let start = a.base().index.max(b.base().index) + 1;
    a.last_index() >= end
        && b.last_index() >= end
        && (start..=end).all(|i| a.entry(i) == b.entry(i))
}

/// `new` keeps every entry of `old` that it still holds (it may have compacted some).
fn extends(new: &DurableState, old: &DurableState) -> bool {
    agree_through(new, old, old.last_index())
}

#[derive(Default)]
pub struct Oracle {
    votes: BTreeMap<(u8, u64), u8>,
    leaders: BTreeMap<u64, u8>,
    committed: BTreeMap<u64, (LogEntry, u64)>,
    applied: BTreeMap<u64, LogEntry>,
    actual_applied: [u64; 3],
    observed_commit: [u64; 3],
    pub apply_count: u64,
    pub election_count: u64,
    pub snapshots_taken: u64,
    pub snapshots_installed: u64,
    /// Read id -> highest index known committed anywhere when the read began.
    reads: BTreeMap<u64, u64>,
    pub reads_served: u64,
    pub reads_failed: u64,
}
impl Oracle {
    fn read_started(&mut self, id: u64) {
        let floor = self.committed.keys().next_back().copied().unwrap_or(0);
        self.reads.insert(id, floor);
    }
    /// I8 Read Freshness (docs/READ_INDEX.md §8): the read reflects every entry
    /// any node knew was committed when it began, and the node has applied it.
    fn read_served(
        &mut self,
        node: usize,
        id: u64,
        read_index: u64,
        applied: u64,
    ) -> Result<(), String> {
        let Some(floor) = self.reads.remove(&id) else {
            return Err(format!("I8 read {id} answered twice or never issued"));
        };
        if read_index < floor {
            return Err(format!(
                "I8 Read Freshness node={} read={id} readIndex={read_index} < committed {floor} at start",
                node + 1
            ));
        }
        if applied < read_index {
            return Err(format!(
                "I8 read {id} answered before applying readIndex {read_index}"
            ));
        }
        self.reads_served += 1;
        Ok(())
    }
    fn read_failed(&mut self, id: u64) -> Result<(), String> {
        self.reads
            .remove(&id)
            .ok_or_else(|| format!("I8 read {id} failed twice or never issued"))?;
        self.reads_failed += 1;
        Ok(())
    }
    /// I6 Snapshot Safety: a snapshot at S must equal the committed prefix 1..=S.
    fn snapshot(&self, node: usize, snapshot: &Snapshot) -> Result<Model, String> {
        let model = Model::decode(snapshot)?;
        let mut digest = 0;
        for index in 1..=snapshot.base.index {
            let Some((entry, _)) = self.committed.get(&index) else {
                return Err(format!(
                    "I6 snapshot node={} covers index {index} never observed committed",
                    node + 1
                ));
            };
            digest = Model::fold(digest, entry);
            if index == snapshot.base.index && entry.term != snapshot.base.term {
                return Err(format!("I6 snapshot node={} base term mismatch", node + 1));
            }
        }
        if digest != model.digest {
            return Err(format!(
                "I6 snapshot node={} index={} differs from the committed prefix",
                node + 1,
                snapshot.base.index
            ));
        }
        Ok(model)
    }
    fn leader_transition(&mut self, core: &Core) -> Result<(), String> {
        if core.role != Role::Leader {
            return Ok(());
        }
        let votes = self
            .votes
            .iter()
            .filter(|((_, term), candidate)| *term == core.state.term && **candidate == core.id)
            .count();
        if votes < 2 {
            return Err("leader transition lacks durable vote quorum".into());
        }
        if let Some(previous) = self.leaders.insert(core.state.term, core.id) {
            if previous != core.id {
                return Err(format!("I1 Election Safety term={}", core.state.term));
            }
        } else {
            self.election_count += 1;
        }
        for (index, (entry, commit_term)) in &self.committed {
            // Indexes the leader compacted are checked through its snapshot (I6).
            if *commit_term < core.state.term
                && *index > core.state.base().index
                && core.state.entry(*index) != Some(entry)
            {
                return Err(format!(
                    "I4 Leader Completeness transition node={} index={index}",
                    core.id
                ));
            }
        }
        Ok(())
    }
    fn durable(&mut self, id: u8, state: &DurableState) -> Result<(), String> {
        if let Some(candidate) = state.voted_for
            && let Some(old) = self.votes.insert((id, state.term), candidate)
            && old != candidate
        {
            return Err(format!("durable double vote node={id} term={}", state.term));
        }
        Ok(())
    }
    fn apply(&mut self, node: usize, core: &Core, entry: &LogEntry) -> Result<(), String> {
        if entry.index != self.actual_applied[node] + 1 || entry.index > core.commit_index {
            return Err(format!(
                "apply order/commit bound node={} index={}",
                node + 1,
                entry.index
            ));
        }
        self.actual_applied[node] = entry.index;
        self.apply_count += 1;
        if let Some((old, _)) = self.committed.get(&entry.index) {
            if old != entry {
                return Err(format!("committed apply conflict index={}", entry.index));
            }
        } else {
            self.committed
                .insert(entry.index, (entry.clone(), core.state.term));
        }
        if let Some(old) = self.applied.insert(entry.index, entry.clone())
            && old != *entry
        {
            return Err(format!("I5 State Machine Safety index={}", entry.index));
        }
        Ok(())
    }
    fn checkpoint(
        &mut self,
        nodes: &[Core; 3],
        disks: &[MockDisk; 3],
        online: &[bool; 3],
    ) -> Result<(), String> {
        for (i, n) in nodes.iter().enumerate() {
            // I7: only applied entries may be compacted.
            if online[i] && n.state.base().index > n.last_applied {
                return Err(format!("I7 compaction beyond applied node={}", i + 1));
            }
        }
        // I3 is checked against independently stored durable logs, even for stopped nodes.
        for a in 0..3 {
            for b in a + 1..3 {
                // Compare the indexes both disks still hold; compacted prefixes are
                // covered by the snapshot check (I6).
                let (left_log, right_log) = (&disks[a].durable, &disks[b].durable);
                let start = left_log.base().index.max(right_log.base().index) + 1;
                let end = left_log.last_index().min(right_log.last_index());
                let mut prefix_equal = true;
                for index in start..=end {
                    let (left, right) = (
                        left_log.entry(index).expect("held"),
                        right_log.entry(index).expect("held"),
                    );
                    prefix_equal &= left == right;
                    if left.term == right.term && !prefix_equal {
                        return Err(format!(
                            "I3 Log Matching nodes={},{} index={}",
                            a + 1,
                            b + 1,
                            left.index
                        ));
                    }
                }
            }
        }
        for (i, n) in nodes.iter().enumerate() {
            if online[i] {
                if n.last_applied > n.commit_index
                    || n.commit_index > n.state.last_index()
                    || n.last_applied != self.actual_applied[i]
                    || n.commit_index < self.observed_commit[i]
                    || !n.state.valid()
                {
                    return Err(format!("index/state invariant node={}", i + 1));
                }
                self.observed_commit[i] = n.commit_index;
                if n.role == Role::Leader {
                    let votes = self
                        .votes
                        .iter()
                        .filter(|((_, term), candidate)| {
                            *term == n.state.term && **candidate == n.id
                        })
                        .count();
                    if votes < 2 {
                        return Err(format!(
                            "I1 leader without durable vote quorum node={} term={}",
                            n.id, n.state.term
                        ));
                    }
                    if let Some(old) = self.leaders.insert(n.state.term, n.id) {
                        if old != n.id {
                            return Err(format!("I1 Election Safety term={}", n.state.term));
                        }
                    } else {
                        self.election_count += 1;
                    }
                    for (index, (entry, commit_term)) in &self.committed {
                        if *commit_term < n.state.term
                            && *index > n.state.base().index
                            && n.state.entry(*index) != Some(entry)
                        {
                            return Err(format!(
                                "I4 Leader Completeness node={} term={} index={index}",
                                n.id, n.state.term
                            ));
                        }
                    }
                }
                for entry in n
                    .state
                    .entries()
                    .iter()
                    .take_while(|entry| entry.index <= n.commit_index)
                {
                    if let Some((old, _)) = self.committed.get(&entry.index) {
                        if old != entry {
                            return Err(format!("committed conflict index={}", entry.index));
                        }
                    } else {
                        self.committed
                            .insert(entry.index, (entry.clone(), n.state.term));
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct Cluster {
    pub nodes: [Core; 3],
    pub disks: [MockDisk; 3],
    pub online: [bool; 3],
    queue: Scheduler<Scheduled>,
    random: Random,
    incarnation: [u64; 3],
    blocked: [[bool; 3]; 3],
    pub oracle: Oracle,
    pub trace: Vec<TraceRow>,
    next_client: u64,
    pub seed: u64,
    pub processed: u64,
    pub models: [Model; 3],
    /// Compact after this many applied entries; `None` never compacts (control group).
    pub snapshot_every: Option<u64>,
}
impl Cluster {
    pub fn new(seed: u64) -> Self {
        let mut s = Self {
            nodes: std::array::from_fn(|i| {
                let mut core = Core::new(i as u8 + 1, DurableState::default()).unwrap();
                core.set_chunk_size(SIM_CHUNK);
                core
            }),
            disks: std::array::from_fn(|_| MockDisk::default()),
            online: [true; 3],
            queue: Scheduler::default(),
            random: Random::new(seed),
            incarnation: [0; 3],
            blocked: [[false; 3]; 3],
            oracle: Oracle::default(),
            trace: vec![],
            next_client: 0,
            seed,
            processed: 0,
            models: [Model::default(); 3],
            // Dissertation 5.1.3 recommends snapshotting after every entry while
            // developing; include that, a few small intervals and a control group.
            snapshot_every: [None, Some(1), Some(2), Some(3), Some(5), Some(8)]
                [(seed % 6) as usize],
        };
        for i in 0..3 {
            let duration = 400 + s.random.next_u64() % 401;
            s.timer(i, duration, Event::ElectionTimeout { generation: 0 });
        }
        s
    }
    fn timer(&mut self, node: usize, delay: u64, event: Event) {
        self.queue.schedule(
            delay,
            Scheduled::Timer {
                node,
                incarnation: self.incarnation[node],
                event,
            },
        );
    }
    fn record(&mut self, node: usize, kind: &str) {
        let n = &self.nodes[node];
        self.trace.push(TraceRow {
            time: self.queue.now,
            node: node as u8 + 1,
            kind: kind.into(),
            term: n.state.term,
            role: format!("{:?}", n.role),
            commit: n.commit_index,
            applied: n.last_applied,
            durable: n.durable_index,
        });
    }
    pub fn partition(&mut self, node: usize) {
        for peer in 0..3 {
            if peer != node {
                self.blocked[node][peer] = true;
                self.blocked[peer][node] = true;
            }
        }
    }
    pub fn heal(&mut self) {
        self.blocked = [[false; 3]; 3];
    }
    fn crash(&mut self, node: usize, survives: bool) -> Result<(), String> {
        self.disks[node].crash(survives);
        self.oracle
            .durable(node as u8 + 1, &self.disks[node].durable)?;
        self.online[node] = false;
        self.incarnation[node] += 1;
        self.oracle.actual_applied[node] = 0;
        self.oracle.observed_commit[node] = 0;
        self.models[node] = Model::default();
        self.record(
            node,
            if survives {
                "crash-unsynced-survives"
            } else {
                "crash"
            },
        );
        let delay = 50 + self.random.next_u64() % 300;
        self.queue.schedule(delay, Scheduled::Restart(node));
        Ok(())
    }
    fn execute(&mut self, node: usize, effects: Vec<Effect>, faults: bool) -> Result<(), String> {
        for effect in effects {
            if !self.online[node] {
                break;
            }
            match effect {
                Effect::Persist { barrier_id, state } => {
                    let old_term = self.disks[node].durable.term;
                    self.disks[node].write(state);
                    // Crash between write and sync permits either complete survival or loss.
                    if faults && self.random.next_u64().is_multiple_of(500) {
                        let survives = self.random.next_u64().is_multiple_of(2);
                        self.crash(node, survives)?;
                        break;
                    }
                    self.disks[node].sync().map_err(str::to_owned)?;
                    if self.disks[node].durable.term < old_term {
                        return Err("durable term regression".into());
                    }
                    self.oracle
                        .durable(node as u8 + 1, &self.disks[node].durable)?;
                    let completion = self.nodes[node].persisted(barrier_id);
                    self.execute(node, completion, faults)?;
                }
                Effect::Send(message) => {
                    match &message.body {
                        Rpc::VoteResponse {
                            term,
                            granted: true,
                        } => {
                            let durable = &self.disks[node].durable;
                            if durable.term != *term || durable.voted_for != Some(message.to) {
                                return Err("vote reply before durable vote".into());
                            }
                        }
                        Rpc::AppendResponse {
                            success: true,
                            matched_through,
                            ..
                        } => {
                            let durable = &self.disks[node].durable;
                            if *matched_through > durable.last_index()
                                || durable.term != self.nodes[node].state.term
                                || !agree_through(
                                    durable,
                                    &self.nodes[node].state,
                                    *matched_through,
                                )
                            {
                                return Err("append ACK before durability".into());
                            }
                        }
                        Rpc::InstallSnapshotResponse {
                            success: true,
                            done: true,
                            last_included_index,
                            ..
                        } => {
                            let durable = &self.disks[node].durable;
                            if durable.last_index() < *last_included_index
                                || durable.term != self.nodes[node].state.term
                            {
                                return Err("snapshot ACK before durability".into());
                            }
                        }
                        Rpc::AppendEntries { .. } | Rpc::InstallSnapshot { .. } => self.timer(
                            node,
                            350,
                            Event::RpcTimeout {
                                peer: message.to,
                                rpc_id: message.rpc_id,
                            },
                        ),
                        _ => {}
                    }
                    if !faults || !self.random.next_u64().is_multiple_of(20) {
                        let delay = 1 + self.random.next_u64() % 80;
                        let delay = if faults && self.random.next_u64().is_multiple_of(20) {
                            delay + 200
                        } else {
                            delay
                        };
                        self.queue
                            .schedule(delay, Scheduled::Deliver(message.clone()));
                        if faults && self.random.next_u64().is_multiple_of(25) {
                            self.queue.schedule(delay + 1, Scheduled::Deliver(message));
                        }
                    }
                }
                Effect::PersistSnapshot {
                    barrier_id,
                    state,
                    snapshot,
                    restore,
                } => {
                    let model = self.oracle.snapshot(node, &snapshot)?;
                    let stop_between = faults && self.random.next_u64().is_multiple_of(100);
                    self.disks[node].install(state, snapshot, stop_between);
                    if stop_between {
                        self.crash(node, false)?;
                        break;
                    }
                    self.oracle
                        .durable(node as u8 + 1, &self.disks[node].durable)?;
                    if restore {
                        self.models[node] = model;
                        self.oracle.actual_applied[node] = model.index;
                        self.oracle.snapshots_installed += 1;
                    } else {
                        self.oracle.snapshots_taken += 1;
                    }
                    let completion = self.nodes[node].persisted(barrier_id);
                    self.execute(node, completion, faults)?;
                }
                Effect::Apply(entry) => {
                    self.oracle.apply(node, &self.nodes[node], &entry)?;
                    self.models[node].apply(&entry)?;
                }
                Effect::ResetElection { generation } => {
                    let duration = 400 + self.random.next_u64() % 401;
                    self.timer(node, duration, Event::ElectionTimeout { generation });
                }
                Effect::ResetHeartbeat { generation } => {
                    self.timer(node, 100, Event::Heartbeat { generation })
                }
                Effect::Reject(_, _) => {}
                Effect::ReadReady { id, read_index } => {
                    let applied = self.models[node].index;
                    self.oracle.read_served(node, id, read_index, applied)?;
                }
                Effect::ReadFailed { id, .. } => self.oracle.read_failed(id)?,
            }
        }
        Ok(())
    }
    fn event(&mut self, node: usize, event: Event, faults: bool) -> Result<(), String> {
        if !self.online[node] {
            return Ok(());
        }
        let was_leader = self.nodes[node].role == Role::Leader;
        let old_term = self.nodes[node].state.term;
        let old_log = self.nodes[node].state.clone();
        let kind = match &event {
            Event::ElectionTimeout { .. } => "election-timer",
            Event::Heartbeat { .. } => "heartbeat",
            Event::Message(_) => "message",
            Event::Propose(_) => "propose",
            Event::RpcTimeout { .. } => "rpc-timeout",
            Event::Compact { .. } => "compact",
            Event::Read { .. } => "read",
            Event::CancelRead { .. } => "cancel-read",
        };
        let effects = self.nodes[node].step(event);
        // Observe election before a crash-prone NoOp persist can hide the transition.
        if !was_leader && self.nodes[node].role == Role::Leader {
            self.oracle.leader_transition(&self.nodes[node])?;
        }
        if was_leader
            && self.nodes[node].role == Role::Leader
            && self.nodes[node].state.term == old_term
            && !extends(&self.nodes[node].state, &old_log)
        {
            return Err(format!("I2 Leader Append-Only node={}", node + 1));
        }
        self.execute(node, effects, faults)?;
        if was_leader && self.online[node] && self.nodes[node].role != Role::Leader {
            let generation = self.nodes[node].election_generation;
            let delay = 400 + self.random.next_u64() % 401;
            self.timer(node, delay, Event::ElectionTimeout { generation });
        }
        self.record(node, kind);
        self.maybe_compact(node, faults)
    }
    fn maybe_compact(&mut self, node: usize, faults: bool) -> Result<(), String> {
        let Some(every) = self.snapshot_every else {
            return Ok(());
        };
        let n = &self.nodes[node];
        if !self.online[node] || n.last_applied < n.state.base().index + every {
            return Ok(());
        }
        if self.models[node].index != n.last_applied {
            return Err(format!(
                "model and Raft disagree on applied node={}",
                node + 1
            ));
        }
        let index = n.last_applied;
        let data = self.models[node].bytes();
        let effects = self.nodes[node].step(Event::Compact { index, data });
        self.execute(node, effects, faults)
    }
    pub fn advance(&mut self, faults: bool) -> Result<bool, String> {
        let Some((_, scheduled)) = self.queue.pop() else {
            return Ok(false);
        };
        self.processed += 1;
        match scheduled {
            Scheduled::Timer {
                node,
                incarnation,
                event,
            } if incarnation == self.incarnation[node] => self.event(node, event, faults)?,
            Scheduled::Deliver(message) => {
                let a = (message.from - 1) as usize;
                let b = (message.to - 1) as usize;
                if self.online[b] && !self.blocked[a][b] {
                    self.event(b, Event::Message(message), faults)?;
                }
            }
            Scheduled::Restart(node) if !self.online[node] => {
                let state = self.disks[node].recover()?;
                let snapshot = self.disks[node].snapshot.clone();
                self.models[node] = match &snapshot {
                    Some(snapshot) => self.oracle.snapshot(node, snapshot)?,
                    None => Model::default(),
                };
                self.oracle.actual_applied[node] = state.base().index;
                self.nodes[node] = Core::restore(node as u8 + 1, state, snapshot)?;
                self.nodes[node].set_chunk_size(SIM_CHUNK);
                self.online[node] = true;
                let duration = 400 + self.random.next_u64() % 401;
                self.timer(node, duration, Event::ElectionTimeout { generation: 0 });
                self.record(node, "restart");
            }
            Scheduled::Heal => self.heal(),
            _ => {}
        }
        if faults {
            let roll = self.random.next_u64() % 1000;
            if roll == 0 {
                let node = (self.random.next_u64() % 3) as usize;
                if self.online[node] {
                    self.crash(node, false)?;
                }
            }
            if roll == 1 {
                let node = (self.random.next_u64() % 3) as usize;
                self.partition(node);
                self.queue.schedule(700, Scheduled::Heal);
            }
        }
        if self.next_client < 64
            && self.random.next_u64().is_multiple_of(40)
            && let Some(node) =
                (0..3).find(|i| self.online[*i] && self.nodes[*i].role == Role::Leader)
        {
            self.next_client += 1;
            let key = format!("k{}", self.next_client % 3);
            let operation = match self.next_client % 3 {
                0 => Some(Operation::Put {
                    key,
                    value: format!("v{}", self.next_client),
                }),
                // S3: a GET is a ReadIndex read, never a log entry.
                1 => None,
                _ => Some(Operation::Delete { key }),
            };
            match operation {
                Some(operation) => self.event(
                    node,
                    Event::Propose(ClientRequest {
                        client_id: self.next_client as u128,
                        sequence: 1,
                        operation,
                    }),
                    faults,
                )?,
                None => {
                    let id = self.next_client;
                    self.oracle.read_started(id);
                    self.event(node, Event::Read { id }, faults)?;
                }
            }
        }
        self.oracle
            .checkpoint(&self.nodes, &self.disks, &self.online)?;
        Ok(true)
    }
    pub fn run(&mut self, events: u64, faults: bool) -> Result<(), String> {
        for _ in 0..events {
            if !self.advance(faults)? {
                return Err("scheduler unexpectedly empty".into());
            }
        }
        if self.oracle.election_count == 0 || self.oracle.apply_count == 0 {
            return Err("campaign exercised no election/apply".into());
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct Failure {
    pub seed: u64,
    pub event: u64,
    pub invariant: String,
    pub trace: Vec<TraceRow>,
}
#[derive(Serialize)]
pub struct Campaign {
    pub command: String,
    pub version: String,
    pub seed_count: u64,
    pub event_count: u64,
    pub pass_count: u64,
    pub failures: Vec<Failure>,
    pub invariants: Vec<&'static str>,
    pub elections: u64,
    pub applies: u64,
    pub snapshots_taken: u64,
    pub snapshots_installed: u64,
    pub reads_served: u64,
    pub reads_failed: u64,
}
pub fn run_campaign(seeds: u64, events: u64) -> Campaign {
    let mut report = Campaign {
        command: std::env::args().collect::<Vec<_>>().join(" "),
        version: env!("CARGO_PKG_VERSION").into(),
        seed_count: seeds,
        event_count: 0,
        pass_count: 0,
        failures: vec![],
        invariants: vec![
            "I1 Election Safety",
            "I2 Leader Append-Only",
            "I3 Log Matching",
            "I4 Leader Completeness",
            "I5 State Machine Safety",
            "I6 Snapshot Safety",
            "I7 Compaction Only Covers Applied Entries",
            "I8 Read Freshness",
        ],
        elections: 0,
        applies: 0,
        snapshots_taken: 0,
        snapshots_installed: 0,
        reads_served: 0,
        reads_failed: 0,
    };
    for seed in 0..seeds {
        let mut cluster = Cluster::new(seed);
        match cluster.run(events, true) {
            Ok(()) => report.pass_count += 1,
            Err(invariant) => report.failures.push(Failure {
                seed,
                event: cluster.processed,
                invariant,
                trace: std::mem::take(&mut cluster.trace),
            }),
        }
        report.event_count += cluster.processed;
        report.elections += cluster.oracle.election_count;
        report.applies += cluster.oracle.apply_count;
        report.snapshots_taken += cluster.oracle.snapshots_taken;
        report.snapshots_installed += cluster.oracle.snapshots_installed;
        report.reads_served += cluster.oracle.reads_served;
        report.reads_failed += cluster.oracle.reads_failed;
        if !report.failures.is_empty() {
            break;
        }
    }
    report
}

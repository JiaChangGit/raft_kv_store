//! Independent observers check the production Core; no second election/replication implementation.
use crate::{MockDisk, Random, Scheduler};
use raft_core::{Change, Core, Effect, Event, Role, bootstrap_state};
use raft_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Snapshot chunk sizes in bytes; the seed picks one. The model's snapshot is
/// about 30 bytes, so 7 and 13 make every transfer span several InstallSnapshot
/// RPCs and 4096 delivers it in a single message. Until the independent review
/// of 2026-11-14 the size was always 7, and the one-message path (where a
/// follower may raise its term and install in one barrier) was never taken
/// (docs/lessons/11-install-from-a-newer-term/).
const SIM_CHUNKS: [usize; 3] = [7, 4096, 13];
/// Node slots (IDs 1..=SLOTS). Five allow configurations of every size 1..=5,
/// including the even sizes the 2015 counterexamples need (docs/MEMBERSHIP.md §12).
pub const SLOTS: usize = 5;
/// The minimum election timeout, as in the node program (§9.4).
const MIN_ELECTION: u64 = 400;
/// Virtual milliseconds the simulated administrator waits for an answer.
const ADMIN_PATIENCE: u64 = 15_000;

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
    /// The administrator stops waiting for request `id` (§10).
    AdminTimeout {
        id: u64,
    },
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
    /// The node's configuration, e.g. "1,2,3"; empty if it has none.
    pub voters: String,
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

/// The configuration at `index` of `core`'s log (the snapshot's if compacted).
fn voters_at(core: &Core, index: LogIndex) -> Option<Voters> {
    core.state
        .config_at(index)
        .map(|(_, voters)| voters)
        .or_else(|| core.snapshot().and_then(|s| Voters::from_bits(s.voters)))
}

#[derive(Default)]
pub struct Oracle {
    votes: BTreeMap<(u8, u64), u8>,
    leaders: BTreeMap<u64, u8>,
    committed: BTreeMap<u64, (LogEntry, u64)>,
    applied: BTreeMap<u64, LogEntry>,
    actual_applied: [u64; SLOTS],
    observed_commit: [u64; SLOTS],
    pub apply_count: u64,
    pub election_count: u64,
    pub snapshots_taken: u64,
    pub snapshots_installed: u64,
    /// Read id -> highest index known committed anywhere when the read began.
    reads: BTreeMap<u64, u64>,
    pub reads_served: u64,
    pub reads_failed: u64,
    /// Committed configurations seen, by size (index 0 unused).
    pub configuration_sizes: [u64; SLOTS + 1],
    pub membership_changes: u64,
    pub membership_failures: BTreeMap<String, u64>,
    pub leader_removals: u64,
    /// Client or administrator requests given to a node that believed it was
    /// leader while another online node led a higher term. Zero would mean the
    /// workload no longer tests deposed leaders (lesson 12).
    pub stale_leader_requests: u64,
    /// Whole snapshots delivered in one InstallSnapshot message (lesson 11).
    pub one_message_snapshots: u64,
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
    /// I6 Snapshot Safety: a snapshot at S must equal the committed prefix 1..=S,
    /// and record the configuration in effect at S (docs/MEMBERSHIP.md §5).
    fn snapshot(&self, node: usize, snapshot: &Snapshot) -> Result<Model, String> {
        let model = Model::decode(snapshot)?;
        let mut digest = 0;
        let mut voters = None;
        for index in 1..=snapshot.base.index {
            let Some((entry, _)) = self.committed.get(&index) else {
                return Err(format!(
                    "I6 snapshot node={} covers index {index} never observed committed",
                    node + 1
                ));
            };
            digest = Model::fold(digest, entry);
            voters = entry.command.config().or(voters);
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
        if voters.map(Voters::bits) != Some(snapshot.voters) {
            return Err(format!(
                "I6 snapshot node={} index={} records the wrong configuration",
                node + 1,
                snapshot.base.index
            ));
        }
        Ok(model)
    }
    /// A new leader: elected by durable votes of a majority of its configuration
    /// (its own vote counts only if it is a member), unique in its term (I1),
    /// holding every entry committed in earlier terms (I4).
    fn leader_transition(&mut self, core: &Core) -> Result<(), String> {
        if core.role != Role::Leader {
            return Ok(());
        }
        let term = core.state.term;
        let Some((_, voters)) = core.configuration() else {
            return Err(format!("leader without configuration node={}", core.id));
        };
        let granted = self
            .votes
            .iter()
            .filter(|((voter, at), candidate)| {
                *at == term && **candidate == core.id && voters.contains(*voter)
            })
            .fold(Voters::default(), |set, ((voter, _), _)| set.with(*voter));
        if !voters.is_quorum(granted) {
            return Err(format!(
                "leader node={} term={term} lacks a durable vote majority of {voters}",
                core.id
            ));
        }
        if let Some(previous) = self.leaders.insert(term, core.id) {
            if previous != core.id {
                return Err(format!("I1 Election Safety term={term}"));
            }
        } else {
            self.election_count += 1;
        }
        self.completeness(core)
    }
    fn completeness(&self, core: &Core) -> Result<(), String> {
        for (index, (entry, commit_term)) in &self.committed {
            // Indexes the leader compacted are checked through its snapshot (I6).
            if *commit_term < core.state.term
                && *index > core.state.base().index
                && core.state.entry(*index) != Some(entry)
            {
                return Err(format!(
                    "I4 Leader Completeness node={} term={} index={index}",
                    core.id, core.state.term
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
    fn commit_observed(&mut self, entry: &LogEntry, term: u64) -> Result<(), String> {
        if let Some((old, _)) = self.committed.get(&entry.index) {
            if old != entry {
                return Err(format!("committed conflict index={}", entry.index));
            }
        } else {
            if let Some(voters) = entry.command.config() {
                self.configuration_sizes[voters.len()] += 1;
            }
            self.committed.insert(entry.index, (entry.clone(), term));
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
        self.commit_observed(entry, core.state.term)?;
        if let Some(old) = self.applied.insert(entry.index, entry.clone())
            && old != *entry
        {
            return Err(format!("I5 State Machine Safety index={}", entry.index));
        }
        Ok(())
    }
    /// I9 (docs/MEMBERSHIP.md §12): every configuration entry a leader appends
    /// (a) follows a committed entry of its term, (b) follows only committed
    /// configurations, and (c) differs from the previous one by exactly one node.
    /// `new` is the state the leader is about to persist; nothing else has
    /// changed its commit index since it appended (see the barrier rule).
    fn change_rule(
        &self,
        core: &Core,
        old_last: LogIndex,
        new: &DurableState,
    ) -> Result<(), String> {
        if core.role != Role::Leader {
            return Ok(());
        }
        let first_new = old_last + 1;
        for entry in new
            .entries_from(first_new.max(new.base().index + 1))
            .unwrap_or(&[])
        {
            let Some(voters) = entry.command.config() else {
                continue;
            };
            let before = new
                .config_at(entry.index - 1)
                .or_else(|| voters_at(core, new.base().index).map(|v| (new.base().index, v)));
            let committed_in_term = core.state.term_at(core.commit_index) == Some(core.state.term);
            let previous_committed = before.is_some_and(|(index, _)| index <= core.commit_index);
            let one_apart =
                before.is_some_and(|(_, old)| (old.bits() ^ voters.bits()).count_ones() == 1);
            if !(committed_in_term && previous_committed && one_apart) {
                return Err(format!(
                    "I9 change rule node={} index={} term-commit={committed_in_term} previous-committed={previous_committed} one-apart={one_apart}",
                    core.id, entry.index
                ));
            }
        }
        Ok(())
    }
    fn checkpoint(
        &mut self,
        nodes: &[Core; SLOTS],
        disks: &[MockDisk; SLOTS],
        online: &[bool; SLOTS],
    ) -> Result<(), String> {
        for (i, n) in nodes.iter().enumerate() {
            // I7: only applied entries may be compacted.
            if online[i] && n.state.base().index > n.last_applied {
                return Err(format!("I7 compaction beyond applied node={}", i + 1));
            }
        }
        // I3 is checked against independently stored durable logs, even for stopped nodes.
        for a in 0..SLOTS {
            for b in a + 1..SLOTS {
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
                    match self.leaders.get(&n.state.term) {
                        // A leader elected inside a durability barrier (a single-voter
                        // configuration) is first seen here.
                        None => self.leader_transition(n)?,
                        Some(id) if *id != n.id => {
                            return Err(format!("I1 Election Safety term={}", n.state.term));
                        }
                        Some(_) => self.completeness(n)?,
                    }
                }
                let committed: Vec<LogEntry> = n
                    .state
                    .entries()
                    .iter()
                    .take_while(|entry| entry.index <= n.commit_index)
                    .cloned()
                    .collect();
                for entry in committed {
                    self.commit_observed(&entry, n.state.term)?;
                }
            }
        }
        Ok(())
    }
}

/// The simulated administrator's outstanding request.
#[derive(Clone, Copy)]
struct AdminRequest {
    id: u64,
    node: usize,
    incarnation: u64,
}

pub struct Cluster {
    pub nodes: [Core; SLOTS],
    pub disks: [MockDisk; SLOTS],
    pub online: [bool; SLOTS],
    queue: Scheduler<Scheduled>,
    random: Random,
    incarnation: [u64; SLOTS],
    blocked: [[bool; SLOTS]; SLOTS],
    pub oracle: Oracle,
    pub trace: Vec<TraceRow>,
    next_client: u64,
    next_stale_read: u64,
    next_admin: u64,
    admin: Option<AdminRequest>,
    pub seed: u64,
    pub processed: u64,
    pub models: [Model; SLOTS],
    /// Compact after this many applied entries; `None` never compacts (control group).
    pub snapshot_every: Option<u64>,
    /// This seed's snapshot chunk size, one of [`SIM_CHUNKS`].
    pub chunk: usize,
}
impl Cluster {
    pub fn new(seed: u64) -> Self {
        // Independent of `snapshot_every` (seed % 6): every pair occurs.
        let chunk = SIM_CHUNKS[((seed / 6) % SIM_CHUNKS.len() as u64) as usize];
        let mut s = Self {
            nodes: std::array::from_fn(|i| {
                let id = i as u8 + 1;
                // Dissertation §4.4: node 1 bootstraps alone; the rest start empty
                // and are added by the simulated administrator.
                let state = if id == 1 {
                    bootstrap_state(1)
                } else {
                    DurableState::default()
                };
                let mut core = Core::new(id, state).unwrap();
                core.set_chunk_size(chunk);
                core
            }),
            disks: std::array::from_fn(|_| MockDisk::default()),
            online: [true; SLOTS],
            queue: Scheduler::default(),
            random: Random::new(seed),
            incarnation: [0; SLOTS],
            blocked: [[false; SLOTS]; SLOTS],
            oracle: Oracle::default(),
            trace: vec![],
            next_client: 0,
            next_stale_read: 0,
            next_admin: 0,
            admin: None,
            seed,
            processed: 0,
            models: [Model::default(); SLOTS],
            // Dissertation 5.1.3 recommends snapshotting after every entry while
            // developing; include that, a few small intervals and a control group.
            snapshot_every: [None, Some(1), Some(2), Some(3), Some(5), Some(8)]
                [(seed % 6) as usize],
            chunk,
        };
        // The bootstrap entry is durable before the node starts (§6).
        s.disks[0].write(s.nodes[0].state.clone());
        s.disks[0].sync().expect("no fault injected yet");
        for i in 0..SLOTS {
            let duration = MIN_ELECTION + s.random.next_u64() % 401;
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
            voters: n
                .configuration()
                .map(|(_, voters)| voters.to_string())
                .unwrap_or_default(),
        });
    }
    /// Cut every link between the nodes in `side` and the rest.
    pub fn partition(&mut self, side: [bool; SLOTS]) {
        for a in 0..SLOTS {
            for b in 0..SLOTS {
                if side[a] != side[b] {
                    self.blocked[a][b] = true;
                }
            }
        }
    }
    pub fn heal(&mut self) {
        self.blocked = [[false; SLOTS]; SLOTS];
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
        // One crash in eight is a long outage, longer than a catch-up may take,
        // so adding a node that is down fails (docs/MEMBERSHIP.md §8).
        let delay = if self.random.next_u64().is_multiple_of(8) {
            11_000 + self.random.next_u64() % 2_000
        } else {
            50 + self.random.next_u64() % 300
        };
        self.queue.schedule(delay, Scheduled::Restart(node));
        Ok(())
    }
    fn execute(&mut self, node: usize, effects: Vec<Effect>, faults: bool) -> Result<(), String> {
        for effect in effects {
            if !self.online[node] {
                break;
            }
            match effect {
                Effect::Persist { barrier_id, delta } => {
                    // The barrier is pending, so the core's state is exactly
                    // what this delta leads to.
                    self.oracle.change_rule(
                        &self.nodes[node],
                        self.disks[node].volatile.last_index(),
                        &self.nodes[node].state,
                    )?;
                    let old_term = self.disks[node].durable.term;
                    self.disks[node].write_delta(&delta);
                    if self.disks[node].volatile != self.nodes[node].state {
                        return Err(format!(
                            "delta does not reproduce the state node={}",
                            node + 1
                        ));
                    }
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
                    // A follower's install is interrupted ten times as often as a
                    // local compaction: it is rarer, and it is the one that can
                    // carry a new term in the same barrier (lesson 11).
                    let odds = if restore { 10 } else { 100 };
                    let stop_between = faults && self.random.next_u64().is_multiple_of(odds);
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
                    let duration = MIN_ELECTION + self.random.next_u64() % 401;
                    self.timer(node, duration, Event::ElectionTimeout { generation });
                    self.timer(
                        node,
                        MIN_ELECTION,
                        Event::LeaderContactExpired { generation },
                    );
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
                Effect::MembershipDone { id, .. } => self.admin_answered(id, None),
                Effect::MembershipFailed { id, status } => self.admin_answered(id, Some(status)),
            }
        }
        Ok(())
    }
    fn admin_answered(&mut self, id: u64, failure: Option<&str>) {
        if self.admin.is_some_and(|request| request.id == id) {
            self.admin = None;
        }
        match failure {
            None => self.oracle.membership_changes += 1,
            Some(status) => {
                *self
                    .oracle
                    .membership_failures
                    .entry(status.to_string())
                    .or_default() += 1
            }
        }
    }
    fn event(&mut self, node: usize, event: Event, faults: bool) -> Result<(), String> {
        if !self.online[node] {
            return Ok(());
        }
        let was_leader = self.nodes[node].role == Role::Leader;
        let was_candidate = self.nodes[node].role == Role::Candidate;
        let old_term = self.nodes[node].state.term;
        let old_log = self.nodes[node].state.clone();
        // I10 (docs/MEMBERSHIP.md §12): a node without configuration, or one
        // that knows it was removed, never becomes a candidate.
        let must_not_campaign = match self.nodes[node].configuration() {
            None => true,
            Some((index, voters)) => {
                !voters.contains(node as u8 + 1) && self.nodes[node].commit_index >= index
            }
        };
        let kind = match &event {
            Event::ElectionTimeout { .. } => "election-timer",
            Event::Heartbeat { .. } => "heartbeat",
            Event::Message(_) => "message",
            Event::Propose(_) => "propose",
            Event::ProposeBatch(_) => "propose-batch",
            Event::RpcTimeout { .. } => "rpc-timeout",
            Event::Compact { .. } => "compact",
            Event::Read { .. } => "read",
            Event::CancelRead { .. } => "cancel-read",
            Event::ChangeMembership { .. } => "change-membership",
            Event::CancelMembership { .. } => "cancel-membership",
            Event::LeaderContactExpired { .. } => "leader-contact-expired",
        };
        let effects = self.nodes[node].step(event);
        if must_not_campaign && !was_candidate && self.nodes[node].role == Role::Candidate {
            return Err(format!("I10 campaign without the right node={}", node + 1));
        }
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
            if self.nodes[node].state.term == old_term {
                // Stepped down without a higher term: removed from its committed
                // configuration (§9.3).
                self.oracle.leader_removals += 1;
            }
            let generation = self.nodes[node].election_generation;
            let delay = MIN_ELECTION + self.random.next_u64() % 401;
            self.timer(node, delay, Event::ElectionTimeout { generation });
            self.timer(
                node,
                MIN_ELECTION,
                Event::LeaderContactExpired { generation },
            );
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
    /// Any online node that believes it is leader, chosen at random: possibly
    /// one that was deposed and has not noticed. Real clients cannot tell the
    /// difference either, and a deposed leader is exactly whom ReadIndex must
    /// stop from answering. S4 always chose the highest term here, which made
    /// I8 blind to a missing confirmation round (lesson 12).
    fn some_leader(&mut self) -> Option<usize> {
        let leaders: Vec<usize> = (0..SLOTS)
            .filter(|i| self.online[*i] && self.nodes[*i].role == Role::Leader)
            .collect();
        if leaders.is_empty() {
            return None;
        }
        let chosen = leaders[(self.random.next_u64() % leaders.len() as u64) as usize];
        let term = |i: usize| self.nodes[i].state.term;
        if leaders.iter().any(|other| term(*other) > term(chosen)) {
            self.oracle.stale_leader_requests += 1;
        }
        Some(chosen)
    }
    /// An online node that believes it is leader although another online node
    /// leads a higher term: the one with the lowest term, if there is any.
    fn deposed_leader(&self) -> Option<usize> {
        let leaders =
            || (0..SLOTS).filter(|i| self.online[*i] && self.nodes[*i].role == Role::Leader);
        let newest = leaders().map(|i| self.nodes[i].state.term).max()?;
        leaders()
            .filter(|i| self.nodes[*i].state.term < newest)
            .min_by_key(|i| self.nodes[*i].state.term)
    }
    /// The simulated administrator (docs/MEMBERSHIP.md §12): add nodes 2 and 3
    /// first, then add or remove a random node, one request at a time.
    fn administer(&mut self, faults: bool) -> Result<(), String> {
        if self.admin.is_some() {
            return Ok(());
        }
        let Some(node) = self.some_leader() else {
            return Ok(());
        };
        let Some((_, voters)) = self.nodes[node].configuration() else {
            return Ok(());
        };
        let pick = |random: &mut Random, from: Vec<NodeId>| {
            from[(random.next_u64() % from.len() as u64) as usize]
        };
        let absent: Vec<NodeId> = (1..=SLOTS as u8)
            .filter(|id| !voters.contains(*id))
            .collect();
        let change = if !voters.contains(2) {
            Change::Add(2)
        } else if !voters.contains(3) {
            Change::Add(3)
        } else if voters.len() == SLOTS
            || (voters.len() > 1 && self.random.next_u64().is_multiple_of(2))
        {
            Change::Remove(pick(&mut self.random, voters.to_vec()))
        } else {
            Change::Add(pick(&mut self.random, absent))
        };
        self.next_admin += 1;
        let id = self.next_admin;
        self.admin = Some(AdminRequest {
            id,
            node,
            incarnation: self.incarnation[node],
        });
        self.queue
            .schedule(ADMIN_PATIENCE, Scheduled::AdminTimeout { id });
        self.event(node, Event::ChangeMembership { id, change }, faults)
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
                    if matches!(
                        message.body,
                        Rpc::InstallSnapshot {
                            offset: 0,
                            done: true,
                            ..
                        }
                    ) {
                        self.oracle.one_message_snapshots += 1;
                    }
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
                self.nodes[node].set_chunk_size(self.chunk);
                self.online[node] = true;
                let duration = MIN_ELECTION + self.random.next_u64() % 401;
                self.timer(node, duration, Event::ElectionTimeout { generation: 0 });
                self.record(node, "restart");
            }
            Scheduled::Heal => self.heal(),
            Scheduled::AdminTimeout { id } => {
                if let Some(request) = self.admin.filter(|request| request.id == id) {
                    self.admin = None;
                    if request.incarnation == self.incarnation[request.node] {
                        self.event(request.node, Event::CancelMembership { id }, faults)?;
                    }
                    *self
                        .oracle
                        .membership_failures
                        .entry("ADMIN_TIMEOUT".into())
                        .or_default() += 1;
                }
            }
            _ => {}
        }
        if faults {
            let roll = self.random.next_u64() % 1000;
            if roll == 0 {
                let node = (self.random.next_u64() % SLOTS as u64) as usize;
                if self.online[node] {
                    self.crash(node, false)?;
                }
            }
            if roll == 1 {
                let side = std::array::from_fn(|_| self.random.next_u64().is_multiple_of(2));
                self.partition(side);
                self.queue.schedule(700, Scheduled::Heal);
            }
        }
        if self.random.next_u64().is_multiple_of(150) {
            self.administer(faults)?;
        }
        if self.next_client < 64
            && self.random.next_u64().is_multiple_of(40)
            && let Some(node) = self.some_leader()
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
        // Two leaders at once are rare and brief, so the ordinary workload
        // seldom meets a deposed one. Ask it for a read on purpose: it must
        // never answer with less than what was committed when the read began
        // (I8). These reads are extra and have their own id range.
        if let Some(node) = self.deposed_leader()
            && self.random.next_u64().is_multiple_of(4)
        {
            self.next_stale_read += 1;
            let id = 1_000_000 + self.next_stale_read;
            self.oracle.stale_leader_requests += 1;
            self.oracle.read_started(id);
            self.event(node, Event::Read { id }, faults)?;
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
    /// Completed membership changes (including ones already in effect).
    pub membership_changes: u64,
    /// Failed or abandoned membership requests, by status.
    pub membership_failures: BTreeMap<String, u64>,
    /// Requests that failed with CATCH_UP_FAILED (also counted above).
    pub catch_up_failures: u64,
    /// Leaders that stepped down because their committed configuration excluded them.
    pub leader_removals: u64,
    /// Committed configurations by size: `configuration_sizes[n]` is the count of size n.
    pub configuration_sizes: Vec<u64>,
    /// Committed configurations of even size (2 or 4).
    pub even_configurations: u64,
    /// Requests given to a self-believed leader of an older term (lesson 12).
    pub stale_leader_requests: u64,
    /// Whole snapshots delivered in one message (lesson 11).
    pub one_message_snapshots: u64,
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
            "I9 Membership Change Rule",
            "I10 No Campaign Without Configuration Or After Known Removal",
        ],
        elections: 0,
        applies: 0,
        snapshots_taken: 0,
        snapshots_installed: 0,
        reads_served: 0,
        reads_failed: 0,
        membership_changes: 0,
        membership_failures: BTreeMap::new(),
        catch_up_failures: 0,
        leader_removals: 0,
        configuration_sizes: vec![0; SLOTS + 1],
        even_configurations: 0,
        stale_leader_requests: 0,
        one_message_snapshots: 0,
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
        let oracle = &cluster.oracle;
        report.event_count += cluster.processed;
        report.elections += oracle.election_count;
        report.applies += oracle.apply_count;
        report.snapshots_taken += oracle.snapshots_taken;
        report.snapshots_installed += oracle.snapshots_installed;
        report.reads_served += oracle.reads_served;
        report.reads_failed += oracle.reads_failed;
        report.membership_changes += oracle.membership_changes;
        for (status, count) in &oracle.membership_failures {
            *report
                .membership_failures
                .entry(status.clone())
                .or_default() += count;
        }
        report.leader_removals += oracle.leader_removals;
        report.stale_leader_requests += oracle.stale_leader_requests;
        report.one_message_snapshots += oracle.one_message_snapshots;
        for (size, count) in oracle.configuration_sizes.iter().enumerate() {
            report.configuration_sizes[size] += count;
        }
        if !report.failures.is_empty() {
            break;
        }
    }
    report.catch_up_failures = report
        .membership_failures
        .get("CATCH_UP_FAILED")
        .copied()
        .unwrap_or(0);
    report.even_configurations = report.configuration_sizes[2] + report.configuration_sizes[4];
    report
}

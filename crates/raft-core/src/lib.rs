//! Pure event-driven Raft. Complete every ordered durability barrier before the next event.
use raft_protocol::*;
use std::collections::BTreeMap;

#[cfg(test)]
mod membership_tests;
#[cfg(test)]
mod perf_tests;
#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
}
/// Per-node arrays are indexed by node ID (1..=MAX_NODE).
const SLOTS: usize = MAX_NODE as usize + 1;
/// docs/MEMBERSHIP.md §8: catch-up runs this many rounds ...
pub const CATCH_UP_ROUNDS: u32 = 10;
/// ... and the last one must see fewer heartbeats than this (the minimum
/// election timeout, 400 ms, is four 100 ms heartbeats) ...
pub const CATCH_UP_SHORT_ROUND: u32 = 4;
/// ... and the whole catch-up may not see more heartbeats than this.
pub const CATCH_UP_MAX_HEARTBEATS: u32 = 100;

/// One membership change (docs/MEMBERSHIP.md §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Add(NodeId),
    Remove(NodeId),
}
/// The first configuration of a new cluster (dissertation §4.4): term 1 and a
/// configuration entry listing only `id`. The host writes it to an empty data
/// directory when asked to bootstrap.
pub fn bootstrap_state(id: NodeId) -> DurableState {
    DurableState::new(
        1,
        None,
        vec![LogEntry {
            index: 1,
            term: 1,
            command: Command::Config { voters: vec![id] },
        }],
    )
}
#[derive(Clone, Debug)]
pub enum Event {
    ElectionTimeout {
        generation: u64,
    },
    Heartbeat {
        generation: u64,
    },
    Message(Envelope),
    Propose(ClientRequest),
    /// Several writes proposed together (docs/PERFORMANCE.md §6.1): one log
    /// entry each, in this order, behind a single durability barrier and one
    /// round of replication. `Propose` is a batch of one.
    ProposeBatch(Vec<ClientRequest>),
    RpcTimeout {
        peer: NodeId,
        rpc_id: u64,
    },
    /// The host serialized its state machine at `index` (which must equal
    /// `last_applied`); replace the log up to there with this snapshot.
    Compact {
        index: LogIndex,
        data: Vec<u8>,
    },
    /// A linearizable read (docs/READ_INDEX.md). `id` is chosen by the host and
    /// comes back in exactly one `ReadReady` or `ReadFailed`, unless cancelled.
    Read {
        id: u64,
    },
    /// The host gave up on a read (its client timed out); forget it.
    CancelRead {
        id: u64,
    },
    /// docs/MEMBERSHIP.md §7. Answered by exactly one `MembershipDone` or
    /// `MembershipFailed`, unless cancelled.
    ChangeMembership {
        id: u64,
        change: Change,
    },
    /// The host gave up waiting; forget the request (a catch-up stops, an
    /// appended configuration entry stays in the log).
    CancelMembership {
        id: u64,
    },
    /// The minimum election timeout passed since the `ResetElection` with this
    /// generation (docs/MEMBERSHIP.md §9.4).
    LeaderContactExpired {
        generation: u64,
    },
}
#[derive(Clone, Debug)]
pub enum Effect {
    /// Durably record `delta`, this step's change to the persistent state.
    Persist {
        barrier_id: u64,
        delta: Delta,
    },
    /// Durably store `snapshot` and replace the WAL with `state`, which starts
    /// right after it (docs/SNAPSHOT_COMPACTION.md §7.4). When `restore` is set
    /// the host must also replace its state machine with the snapshot's content.
    /// Like `Persist`, it is a barrier: call `persisted` once it is durable.
    PersistSnapshot {
        barrier_id: u64,
        state: DurableState,
        snapshot: Snapshot,
        restore: bool,
    },
    Send(Envelope),
    Apply(LogEntry),
    ResetElection {
        generation: u64,
    },
    ResetHeartbeat {
        generation: u64,
    },
    Reject(ClientRequest, &'static str),
    /// The host may now answer read `id` from its state machine, which has
    /// applied at least through `read_index`.
    ReadReady {
        id: u64,
        read_index: LogIndex,
    },
    /// Read `id` was not served and had no effect.
    ReadFailed {
        id: u64,
        status: &'static str,
    },
    /// The configuration entry for request `id` is committed at `index`.
    MembershipDone {
        id: u64,
        voters: Voters,
        index: LogIndex,
    },
    /// Request `id` failed. `TIMEOUT_UNKNOWN` means its configuration entry was
    /// appended and may still be committed; every other status means no change.
    MembershipFailed {
        id: u64,
        status: &'static str,
    },
}

#[derive(Clone, Debug)]
struct Inflight {
    rpc_id: u64,
    term: u64,
    epoch: u64,
    prev: u64,
    count: u64,
    commit: u64,
    /// Set when this request is an InstallSnapshot chunk: which snapshot.
    chunk: Option<LogBase>,
    /// `read_round` when this request was sent; a reply confirms leadership
    /// only for reads registered no later than that (docs/READ_INDEX.md §6.2).
    round: u64,
}

/// A read waiting for its confirmation round (docs/READ_INDEX.md §6.1).
#[derive(Clone, Debug)]
struct PendingRead {
    id: u64,
    /// `None` until the leader has committed an entry of its own term.
    read_index: Option<LogIndex>,
    round: u64,
}

/// The membership request a leader is working on (docs/MEMBERSHIP.md §7).
#[derive(Clone, Debug)]
struct PendingChange {
    id: u64,
    change: Change,
    phase: Phase,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waiting until an entry of this term is committed (the 2015 fix).
    WaitForTermCommit,
    /// docs/MEMBERSHIP.md §8. `target` is the leader's last index when the
    /// round started; `round_beats` counts heartbeats within it.
    CatchingUp {
        node: NodeId,
        round: u32,
        target: LogIndex,
        round_beats: u32,
        total_beats: u32,
    },
    /// The configuration entry is in the log at `index`.
    Appended { index: LogIndex },
}

/// A snapshot a follower is receiving chunk by chunk (Figure 13 steps 2-5).
#[derive(Clone, Debug)]
struct Receiving {
    term: Term,
    base: LogBase,
    total_len: u64,
    total_crc: u32,
    data: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Core {
    pub id: NodeId,
    pub state: DurableState,
    pub role: Role,
    pub commit_index: u64,
    pub last_applied: u64,
    pub election_generation: u64,
    pub heartbeat_generation: u64,
    pub leader_hint: Option<NodeId>,
    pub durable_index: u64,
    votes: Voters,
    vote_requests: BTreeMap<NodeId, u64>,
    next: [u64; SLOTS],
    matches: [u64; SLOTS],
    inflight: [Option<Inflight>; SLOTS],
    epoch: u64,
    serial: u64,
    barrier_serial: u64,
    pending_barrier: Option<u64>,
    /// The latest durable snapshot; leaders send it to followers that need
    /// entries already compacted away.
    snapshot: Option<Snapshot>,
    /// The configuration at the log base, from the snapshot there (None
    /// without a snapshot). It applies when the log holds no configuration.
    base_voters: Option<Voters>,
    pending_snapshot: Option<Snapshot>,
    /// Requested during this step; becomes a PersistSnapshot barrier at its end.
    requested_snapshot: Option<(Snapshot, bool)>,
    /// Leader: snapshot being sent to each follower, and the next offset.
    transfer: [Option<(LogBase, u64)>; SLOTS],
    receiving: Option<Receiving>,
    chunk_size: usize,
    read_round: u64,
    acked_round: [u64; SLOTS],
    pending_reads: Vec<PendingRead>,
    /// Heard from the leader of this term within the minimum election timeout
    /// (docs/MEMBERSHIP.md §9.4); a leader always has.
    leader_contact: bool,
    pending_change: Option<PendingChange>,
    /// Term and vote as of the last barrier: a change needs a new one.
    sealed: (Term, Option<NodeId>),
    /// Entries copied into `Persist` effects so far (docs/PERFORMANCE.md §6.4).
    pub entries_copied: u64,
    /// AppendEntries requests sent, and the entries they carried.
    pub append_rpcs: u64,
    pub append_entries_sent: u64,
    failed: bool,
}
impl Core {
    /// A node whose log starts at index 1 (no snapshot).
    pub fn new(id: NodeId, state: DurableState) -> Result<Self, String> {
        Self::restore(id, state, None)
    }
    /// A node recovered from storage: the log base must be the snapshot's position.
    pub fn restore(
        id: NodeId,
        state: DurableState,
        snapshot: Option<Snapshot>,
    ) -> Result<Self, String> {
        if !valid_node(id) || !state.valid() {
            return Err("invalid identity or durable state".into());
        }
        let snapshot_base = snapshot.as_ref().map(|s| s.base).unwrap_or_default();
        if state.base() != snapshot_base || snapshot.as_ref().is_some_and(|s| !s.valid()) {
            return Err("log base must equal the durable snapshot's position".into());
        }
        let base_voters = snapshot.as_ref().and_then(|s| Voters::from_bits(s.voters));
        let durable_index = state.last_index();
        let mut state = state;
        state.mark_clean();
        let sealed = (state.term, state.voted_for);
        // Entries up to the base are inside a durable snapshot, hence committed and applied.
        let snapshot_index = state.base().index;
        Ok(Self {
            id,
            state,
            role: Role::Follower,
            commit_index: snapshot_index,
            last_applied: snapshot_index,
            election_generation: 0,
            heartbeat_generation: 0,
            leader_hint: None,
            durable_index,
            votes: Voters::default(),
            vote_requests: BTreeMap::new(),
            next: [1; SLOTS],
            matches: [0; SLOTS],
            inflight: std::array::from_fn(|_| None),
            epoch: 0,
            serial: 0,
            barrier_serial: 0,
            pending_barrier: None,
            snapshot,
            base_voters,
            pending_snapshot: None,
            requested_snapshot: None,
            transfer: [None; SLOTS],
            receiving: None,
            chunk_size: MAX_CHUNK,
            read_round: 0,
            acked_round: [0; SLOTS],
            pending_reads: vec![],
            leader_contact: false,
            pending_change: None,
            sealed,
            entries_copied: 0,
            append_rpcs: 0,
            append_entries_sent: 0,
            failed: false,
        })
    }
    /// The configuration in effect: the latest one in the log, else the
    /// snapshot's (at the base index); `None` for a node never configured.
    pub fn configuration(&self) -> Option<(LogIndex, Voters)> {
        self.state.latest_config().or_else(|| {
            self.base_voters
                .map(|voters| (self.state.base().index, voters))
        })
    }
    fn voters(&self) -> Voters {
        self.configuration()
            .map(|(_, voters)| voters)
            .unwrap_or_default()
    }
    /// The configuration at `index` (which must not be compacted).
    fn voters_at(&self, index: LogIndex) -> Option<Voters> {
        self.state
            .config_at(index)
            .map(|(_, voters)| voters)
            .or(self.base_voters)
    }
    /// The node being caught up before it is added (docs/MEMBERSHIP.md §8).
    pub fn catching_up(&self) -> Option<NodeId> {
        match self.pending_change.as_ref()?.phase {
            Phase::CatchingUp { node, .. } => Some(node),
            _ => None,
        }
    }
    pub fn leader_contact(&self) -> bool {
        self.leader_contact
    }
    /// docs/MEMBERSHIP.md §9.2: no configuration, no election; a node outside
    /// its configuration campaigns only until it knows that one is committed.
    fn may_campaign(&self) -> bool {
        match self.configuration() {
            None => false,
            Some((index, voters)) => voters.contains(self.id) || self.commit_index < index,
        }
    }
    /// Whom the leader replicates to: the other voters and a catching-up node.
    fn targets(&self) -> Voters {
        let voters = self.voters().without(self.id);
        match self.catching_up() {
            Some(node) => voters.with(node),
            None => voters,
        }
    }
    /// Forget what the leader tracked for `peer` and start over (§8, §9.5).
    fn reset_peer(&mut self, peer: NodeId) {
        let p = peer as usize;
        self.next[p] = self.state.last_index() + 1;
        self.matches[p] = 0;
        self.inflight[p] = None;
        self.transfer[p] = None;
        self.acked_round[p] = 0;
    }
    /// The leader has committed an entry of its own term (dissertation §6.4
    /// step 1 and the 2015 membership fix).
    fn committed_in_term(&self) -> bool {
        self.state.term_at(self.commit_index) == Some(self.state.term)
    }
    /// Snapshot chunk size in bytes, at most [`MAX_CHUNK`]. The simulator uses
    /// tiny chunks so that multi-chunk transfers are exercised.
    pub fn set_chunk_size(&mut self, bytes: usize) {
        self.chunk_size = bytes.clamp(1, MAX_CHUNK);
    }
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn matched(&self, peer: NodeId) -> u64 {
        if valid_node(peer) {
            self.matches[peer as usize]
        } else {
            0
        }
    }
    pub fn storage_failed(&mut self) {
        self.failed = true;
    }
    pub fn persisted(&mut self, barrier_id: u64) -> Vec<Effect> {
        if self.failed || self.pending_barrier != Some(barrier_id) {
            return vec![];
        }
        self.pending_barrier = None;
        if let Some(snapshot) = self.pending_snapshot.take() {
            self.snapshot = Some(snapshot);
        }
        self.durable_index = self.state.last_index();

        let mut effects = vec![];
        if self.role == Role::Candidate && self.state.voted_for == Some(self.id) {
            // Our own vote counts once it is durable, and only if we vote; a
            // single-voter configuration is won right here.
            self.votes = self.votes.with(self.id);
            if self.voters().is_quorum(self.votes) {
                self.become_leader(&mut effects);
            }
        }
        if self.role == Role::Leader {
            self.matches[self.id as usize] = self.durable_index;
            self.advance_commit(&mut effects);
            self.start_read_round(&mut effects);
            self.finish_reads(&mut effects);
        }
        self.seal(effects)
    }
    pub fn step(&mut self, event: Event) -> Vec<Effect> {
        if self.failed || self.pending_barrier.is_some() {
            return vec![];
        }

        let mut effects = vec![];
        match event {
            Event::ElectionTimeout { generation }
                if generation == self.election_generation && self.role != Role::Leader =>
            {
                // An ineligible node stays quiet until a leader contacts it,
                // which is the only way its eligibility can change.
                if self.may_campaign() {
                    self.begin_election(&mut effects)
                }
            }
            Event::LeaderContactExpired { generation }
                if generation == self.election_generation && self.role != Role::Leader =>
            {
                self.leader_contact = false;
            }
            Event::Heartbeat { generation }
                if generation == self.heartbeat_generation && self.role == Role::Leader =>
            {
                self.replicate_all(&mut effects);
                self.heartbeat_generation += 1;
                effects.push(Effect::ResetHeartbeat {
                    generation: self.heartbeat_generation,
                });
                self.catch_up_heartbeat(&mut effects);
            }
            Event::ChangeMembership { id, change } => {
                self.change_membership(id, change, &mut effects)
            }
            Event::CancelMembership { id } => {
                if self.pending_change.as_ref().is_some_and(|c| c.id == id) {
                    let node = self.catching_up();
                    self.pending_change = None;
                    if let Some(node) = node {
                        self.reset_peer(node);
                    }
                }
            }
            Event::Message(message) if message.valid_for(self.id) => {
                self.message(message, &mut effects)
            }
            Event::Compact { index, data } => self.compact(index, data),
            Event::Read { id } => self.read(id, &mut effects),
            Event::CancelRead { id } => self.pending_reads.retain(|read| read.id != id),
            Event::RpcTimeout { peer, rpc_id } if valid_node(peer) && self.role == Role::Leader => {
                if self.inflight[peer as usize]
                    .as_ref()
                    .is_some_and(|r| r.rpc_id == rpc_id)
                {
                    self.inflight[peer as usize] = None;
                    self.replicate(peer, &mut effects);
                }
            }
            Event::Propose(request) => self.propose(vec![request], &mut effects),
            Event::ProposeBatch(requests) => self.propose(requests, &mut effects),
            _ => {}
        }
        self.seal(effects)
    }
    /// Group commit: every valid request becomes an entry; the step's one
    /// barrier then covers them all. An invalid request is rejected alone.
    fn propose(&mut self, requests: Vec<ClientRequest>, effects: &mut Vec<Effect>) {
        let mut appended = false;
        for request in requests {
            if !request.operation.valid() {
                effects.push(Effect::Reject(request, "INVALID_ARGUMENT"));
            } else if self.role != Role::Leader {
                effects.push(Effect::Reject(request, "NOT_LEADER"));
            } else {
                self.state.push(LogEntry {
                    index: self.state.last_index() + 1,
                    term: self.state.term,
                    command: Command::Client(request),
                });
                appended = true;
            }
        }
        if appended {
            self.replicate_all(effects);
        }
    }
    /// End of a step: one durability barrier for whatever changed (or none).
    ///
    /// The barrier precedes everything that may depend on it (messages and
    /// answers), but not the `Apply`s: those cover entries committed before
    /// this barrier, and the barrier's completion may commit and apply more.
    /// Hosts run that completion before the rest of this list, so an `Apply`
    /// left behind the barrier would be applied after a later index (lesson 09).
    fn seal(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        let barrier = if let Some((snapshot, restore)) = self.requested_snapshot.take() {
            // One barrier writes the snapshot and the new WAL, which also carries
            // any term or vote change made in this step.
            self.barrier_serial += 1;
            self.pending_barrier = Some(self.barrier_serial);
            self.pending_snapshot = Some(snapshot.clone());
            self.state.mark_clean();
            self.sealed = (self.state.term, self.state.voted_for);
            Effect::PersistSnapshot {
                barrier_id: self.barrier_serial,
                state: self.state.clone(),
                snapshot,
                restore,
            }
        } else if self.state.log_changed() || self.sealed != (self.state.term, self.state.voted_for)
        {
            // Only what this step changed (docs/PERFORMANCE.md §6.3): the log
            // itself records where it was first touched, so nothing is copied
            // or compared beyond the entries that go to disk.
            self.barrier_serial += 1;
            self.pending_barrier = Some(self.barrier_serial);
            let delta = self.state.delta();
            self.entries_copied += delta.append.len() as u64;
            self.state.mark_clean();
            self.sealed = (self.state.term, self.state.voted_for);
            Effect::Persist {
                barrier_id: self.barrier_serial,
                delta,
            }
        } else {
            return effects;
        };
        let (mut sealed, rest): (Vec<_>, Vec<_>) = effects
            .into_iter()
            .partition(|effect| matches!(effect, Effect::Apply(_)));
        sealed.push(barrier);
        sealed.extend(rest);
        sealed
    }
    /// Local compaction (docs/SNAPSHOT_COMPACTION.md §8). Only applied entries
    /// may be covered; anything else is ignored and the host simply keeps its log.
    fn compact(&mut self, index: LogIndex, data: Vec<u8>) {
        let Some(term) = self.state.term_at(index) else {
            return;
        };
        if index != self.last_applied || index <= self.state.base().index {
            return;
        }
        // The snapshot records the configuration at its position (§5); it is
        // committed, since only applied entries are covered.
        let Some(voters) = self.voters_at(index) else {
            return;
        };
        let snapshot = Snapshot::new(LogBase { index, term }, voters, data);
        if !snapshot.valid() {
            return;
        }
        self.state = adopt_snapshot(&self.state, snapshot.base);
        self.base_voters = Some(voters);
        self.requested_snapshot = Some((snapshot, false));
    }
    /// docs/READ_INDEX.md §6.2 step 1-3.
    fn read(&mut self, id: u64, effects: &mut Vec<Effect>) {
        if self.role != Role::Leader {
            effects.push(Effect::ReadFailed {
                id,
                status: "NOT_LEADER",
            });
            return;
        }
        self.pending_reads.push(PendingRead {
            id,
            read_index: None,
            round: 0,
        });
        self.start_read_round(effects);
        // A leader that is the only voter is its own majority: no reply will
        // ever arrive to finish this read, so it is finished here (lesson 20).
        self.finish_reads(effects);
    }
    /// Give every read still without a readIndex one, and a fresh round.
    fn start_read_round(&mut self, effects: &mut Vec<Effect>) {
        // Step 1: a new leader does not know what is committed until an entry
        // of its own term is (the no-op appended on election).
        if self.role != Role::Leader || !self.committed_in_term() {
            return;
        }
        if self
            .pending_reads
            .iter()
            .all(|read| read.read_index.is_some())
        {
            return;
        }
        // Step 4's key point: only requests sent after this increment can
        // confirm these reads, because they carry the new round number.
        self.read_round += 1;
        for read in self
            .pending_reads
            .iter_mut()
            .filter(|r| r.read_index.is_none())
        {
            read.read_index = Some(self.commit_index);
            read.round = self.read_round;
        }
        self.replicate_all(effects);
    }
    /// Steps 6-7: answer every read whose round a majority has acknowledged.
    fn finish_reads(&mut self, effects: &mut Vec<Effect>) {
        if self.role != Role::Leader || self.pending_reads.is_empty() {
            return;
        }
        // Only voters of the current configuration confirm (§9.1); the leader
        // counts itself only if it votes.
        let voters = self.voters();
        let mut acks: Vec<u64> = voters
            .ids()
            .filter(|peer| *peer != self.id)
            .map(|peer| self.acked_round[peer as usize])
            .collect();
        acks.sort_unstable_by(|a, b| b.cmp(a));
        // The highest round that `needed` followers have acknowledged is
        // confirmed. Too few followers means nothing is confirmed (0), never everything.
        let needed = voters
            .majority()
            .saturating_sub(usize::from(voters.contains(self.id)));
        let confirmed = if voters.is_empty() {
            0
        } else {
            match needed {
                0 => u64::MAX,
                needed => acks.get(needed - 1).copied().unwrap_or(0),
            }
        };
        let last_applied = self.last_applied;
        self.pending_reads.retain(|read| match read.read_index {
            Some(read_index) if read.round <= confirmed && last_applied >= read_index => {
                effects.push(Effect::ReadReady {
                    id: read.id,
                    read_index,
                });
                false
            }
            _ => true,
        });
    }
    /// Make sure every follower gets a request of the newest round.
    fn replicate_for_reads(&mut self, peer: NodeId, effects: &mut Vec<Effect>) {
        let wanted = self
            .pending_reads
            .iter()
            .map(|r| r.round)
            .max()
            .unwrap_or(0);
        if wanted > self.acked_round[peer as usize] {
            self.replicate(peer, effects);
        }
    }
    /// Step 8: a node that is no longer leader serves no read.
    fn fail_reads(&mut self, effects: &mut Vec<Effect>) {
        for read in self.pending_reads.drain(..) {
            effects.push(Effect::ReadFailed {
                id: read.id,
                status: "NOT_LEADER",
            });
        }
        self.acked_round = [0; SLOTS];
    }
    /// A former leader answers its membership request (§7 step 9).
    fn fail_change(&mut self, effects: &mut Vec<Effect>) {
        if let Some(change) = self.pending_change.take() {
            let status = match change.phase {
                Phase::Appended { .. } => "TIMEOUT_UNKNOWN",
                _ => "NOT_LEADER",
            };
            effects.push(Effect::MembershipFailed {
                id: change.id,
                status,
            });
        }
    }
    fn reset_election(&mut self, effects: &mut Vec<Effect>) {
        self.election_generation += 1;
        effects.push(Effect::ResetElection {
            generation: self.election_generation,
        });
    }
    fn send(&mut self, to: NodeId, body: Rpc, effects: &mut Vec<Effect>) -> u64 {
        self.serial += 1;
        let rpc_id = self.serial;
        self.reply(to, rpc_id, body, effects);
        rpc_id
    }
    fn reply(&self, to: NodeId, rpc_id: u64, body: Rpc, effects: &mut Vec<Effect>) {
        effects.push(Effect::Send(Envelope {
            version: 1,
            cluster_id: CLUSTER.into(),
            from: self.id,
            to,
            rpc_id,
            body,
        }));
    }
    fn step_down(&mut self, term: u64, effects: &mut Vec<Effect>) {
        let needs_election_timer = self.role == Role::Leader;
        if term > self.state.term {
            self.state.term = term;
            self.state.voted_for = None;
        }
        self.role = Role::Follower;
        self.leader_hint = None;
        self.votes = Voters::default();
        self.vote_requests.clear();
        self.inflight = std::array::from_fn(|_| None);
        self.transfer = [None; SLOTS];
        self.fail_reads(effects);
        self.fail_change(effects);
        self.heartbeat_generation += 1;
        // A former leader has no active election timer; this starts one without
        // resetting an already running follower timer on arbitrary higher terms.
        if needs_election_timer {
            effects.push(Effect::ResetElection {
                generation: self.election_generation,
            });
        }
    }
    fn begin_election(&mut self, effects: &mut Vec<Effect>) {
        let Some(term) = self.state.term.checked_add(1) else {
            self.failed = true;
            return;
        };
        self.state.term = term;
        self.state.voted_for = Some(self.id);
        self.role = Role::Candidate;
        self.leader_hint = None;
        self.leader_contact = false;
        self.votes = Voters::default();
        self.vote_requests.clear();
        self.inflight = std::array::from_fn(|_| None);
        self.transfer = [None; SLOTS];
        self.fail_reads(effects);
        self.reset_election(effects);
        // Ask the other voters of our configuration (§9.2); our own vote is
        // counted in `persisted`, once durable.
        for peer in self.voters().without(self.id).to_vec() {
            let id = self.send(
                peer,
                Rpc::RequestVote {
                    term: self.state.term,
                    candidate_id: self.id,
                    last_log_index: self.state.last_index(),
                    last_log_term: self.state.term_at(self.state.last_index()).unwrap(),
                },
                effects,
            );
            self.vote_requests.insert(peer, id);
        }
    }
    fn become_leader(&mut self, effects: &mut Vec<Effect>) {
        self.role = Role::Leader;
        self.leader_hint = Some(self.id);
        self.leader_contact = true;
        self.pending_change = None;
        self.election_generation += 1;
        self.heartbeat_generation += 1;
        self.epoch += 1;
        self.next = [self.state.last_index() + 1; SLOTS];
        self.matches = [0; SLOTS];
        self.matches[self.id as usize] = self.durable_index;
        self.inflight = std::array::from_fn(|_| None);
        self.transfer = [None; SLOTS];
        self.acked_round = [0; SLOTS];
        self.state.push(LogEntry {
            index: self.state.last_index() + 1,
            term: self.state.term,
            command: Command::NoOp,
        });
        self.replicate_all(effects);
        effects.push(Effect::ResetHeartbeat {
            generation: self.heartbeat_generation,
        });
    }
    fn replicate_all(&mut self, effects: &mut Vec<Effect>) {
        for peer in self.targets().to_vec() {
            self.replicate(peer, effects);
        }
    }
    fn replicate(&mut self, peer: NodeId, effects: &mut Vec<Effect>) {
        let p = peer as usize;
        if self.role != Role::Leader || self.inflight[p].is_some() || !self.targets().contains(peer)
        {
            return;
        }
        let next = self.next[p].clamp(1, self.state.last_index() + 1);
        let prev = next - 1;
        let Some(prev_log_term) = self.state.term_at(prev) else {
            // The entry before `next` was compacted: only the snapshot can help.
            return self.send_snapshot_chunk(peer, effects);
        };
        // Everything the follower lacks, up to one batch (§6.2): while a
        // request is in flight new entries accumulate, so batches grow with load.
        let available = self
            .state
            .entries_from(next)
            .expect("next lies after the base");
        let entries = available[..batch_len(available)].to_vec();
        let count = entries.len() as u64;
        self.append_rpcs += 1;
        self.append_entries_sent += count;
        let rpc_id = self.send(
            peer,
            Rpc::AppendEntries {
                term: self.state.term,
                leader_id: self.id,
                prev_log_index: prev,
                prev_log_term,
                entries,
                leader_commit: self.commit_index,
            },
            effects,
        );
        self.inflight[p] = Some(Inflight {
            rpc_id,
            term: self.state.term,
            epoch: self.epoch,
            prev,
            count,
            commit: self.commit_index,
            chunk: None,
            round: self.read_round,
        });
    }
    /// docs/SNAPSHOT_COMPACTION.md §9.2: send the next chunk of the current snapshot.
    fn send_snapshot_chunk(&mut self, peer: NodeId, effects: &mut Vec<Effect>) {
        let p = peer as usize;
        let snapshot = self
            .snapshot
            .clone()
            .expect("a leader with a compacted log holds the snapshot it came from");
        // A newer snapshot restarts the transfer from the beginning.
        let offset = match self.transfer[p] {
            Some((base, offset)) if base == snapshot.base => offset,
            _ => 0,
        };
        self.transfer[p] = Some((snapshot.base, offset));
        let total = snapshot.data.len() as u64;
        let start = offset.min(total) as usize;
        let end = (start + self.chunk_size).min(snapshot.data.len());
        let rpc_id = self.send(
            peer,
            Rpc::InstallSnapshot {
                term: self.state.term,
                leader_id: self.id,
                last_included_index: snapshot.base.index,
                last_included_term: snapshot.base.term,
                voters: snapshot.voters,
                total_len: total,
                total_crc: snapshot.crc(),
                offset: start as u64,
                data: to_hex(&snapshot.data[start..end]),
                done: end == snapshot.data.len(),
            },
            effects,
        );
        self.inflight[p] = Some(Inflight {
            rpc_id,
            term: self.state.term,
            epoch: self.epoch,
            prev: 0,
            count: 0,
            commit: self.commit_index,
            chunk: Some(snapshot.base),
            round: self.read_round,
        });
    }
    fn advance_commit(&mut self, effects: &mut Vec<Effect>) {
        // A majority of the current configuration (§9.1): the leader's own
        // durable index counts only if it is a voter.
        let voters = self.voters();
        for n in (self.commit_index + 1..=self.durable_index).rev() {
            let holders = voters
                .ids()
                .filter(|id| self.matches[*id as usize] >= n)
                .fold(Voters::default(), Voters::with);
            // Figure 2 / §5.4.2: previous-term majority alone cannot directly commit.
            if self.state.term_at(n) == Some(self.state.term) && voters.is_quorum(holders) {
                self.commit_index = n;
                break;
            }
        }
        self.apply_committed(effects);
        if self.role == Role::Leader {
            self.after_commit(effects);
        }
    }
    /// What a leader does once more is committed (docs/MEMBERSHIP.md §7, §9.3).
    fn after_commit(&mut self, effects: &mut Vec<Effect>) {
        match self.pending_change.as_ref().map(|c| c.phase) {
            Some(Phase::WaitForTermCommit) => self.proceed_change(effects),
            Some(Phase::Appended { index }) if self.commit_index >= index => {
                let change = self.pending_change.take().expect("present");
                effects.push(Effect::MembershipDone {
                    id: change.id,
                    voters: self.voters_at(index).unwrap_or_default(),
                    index,
                });
            }
            _ => {}
        }
        // A leader outside its own committed configuration steps down.
        if let Some((index, voters)) = self.configuration()
            && !voters.contains(self.id)
            && self.commit_index >= index
        {
            let term = self.state.term;
            self.step_down(term, effects);
        }
    }
    /// docs/MEMBERSHIP.md §7 steps 1-3; the rest waits for the term commit.
    fn change_membership(&mut self, id: u64, change: Change, effects: &mut Vec<Effect>) {
        let fail = |status| Effect::MembershipFailed { id, status };
        if self.role != Role::Leader {
            return effects.push(fail("NOT_LEADER"));
        }
        let voters = self.voters();
        let (node, wanted) = match change {
            Change::Add(node) => (node, voters.with(node)),
            Change::Remove(node) => (node, voters.without(node)),
        };
        if !valid_node(node) || wanted.is_empty() {
            return effects.push(fail("INVALID_ARGUMENT"));
        }
        if self.pending_change.is_some() {
            return effects.push(fail("CHANGE_IN_PROGRESS"));
        }
        self.pending_change = Some(PendingChange {
            id,
            change,
            phase: Phase::WaitForTermCommit,
        });
        self.proceed_change(effects);
    }
    /// Steps 4-8: once this term has a committed entry, check what is
    /// committed, then remove at once or start catching the new node up.
    fn proceed_change(&mut self, effects: &mut Vec<Effect>) {
        // The 2015 fix: a leader that has committed an entry of its own term
        // knows the latest committed configuration, and no uncommitted one
        // from an earlier term can be committed any more. Before that, its
        // commit index lags, so nothing below can be judged (lesson 08).
        if !self.committed_in_term() {
            return;
        }
        let Some(pending) = self.pending_change.as_ref() else {
            return;
        };
        let (id, change) = (pending.id, pending.change);
        let (index, voters) = self.configuration().expect("a leader has a configuration");
        // Only an entry this leader appended for an earlier (cancelled)
        // request can still be uncommitted now.
        if index > self.commit_index {
            self.pending_change = None;
            return effects.push(Effect::MembershipFailed {
                id,
                status: "CHANGE_IN_PROGRESS",
            });
        }
        let wanted = match change {
            Change::Add(node) => voters.with(node),
            Change::Remove(node) => voters.without(node),
        };
        if wanted == voters {
            self.pending_change = None;
            return effects.push(Effect::MembershipDone { id, voters, index });
        }
        let change = self.pending_change.as_mut().expect("present");
        match change.change {
            Change::Remove(node) => {
                let voters = self.voters().without(node);
                self.append_config(voters, effects);
            }
            Change::Add(node) => {
                change.phase = Phase::CatchingUp {
                    node,
                    round: 1,
                    target: self.state.last_index(),
                    round_beats: 0,
                    total_beats: 0,
                };
                self.reset_peer(node);
                self.catch_up_progress(effects);
                self.replicate(node, effects);
            }
        }
    }
    /// Append the configuration entry for the pending change (§7 steps 6-7).
    fn append_config(&mut self, voters: Voters, effects: &mut Vec<Effect>) {
        let before = self.targets();
        let index = self.state.last_index() + 1;
        self.state.push(LogEntry {
            index,
            term: self.state.term,
            command: Command::Config {
                voters: voters.to_vec(),
            },
        });
        if let Some(change) = self.pending_change.as_mut() {
            change.phase = Phase::Appended { index };
        }
        // A node that is no longer a target gets nothing more (§9.5).
        let after = self.targets();
        for peer in before.ids().filter(|peer| !after.contains(*peer)) {
            self.inflight[peer as usize] = None;
            self.transfer[peer as usize] = None;
        }
        for peer in after.ids().filter(|peer| !before.contains(*peer)) {
            self.reset_peer(peer);
        }
        self.replicate_all(effects);
    }
    /// §8: finish every round the new node has reached; after the last one,
    /// add it if that round was short, otherwise give up.
    fn catch_up_progress(&mut self, effects: &mut Vec<Effect>) {
        let last = self.state.last_index();
        let Some(PendingChange {
            phase:
                Phase::CatchingUp {
                    node,
                    round,
                    target,
                    round_beats,
                    ..
                },
            ..
        }) = self.pending_change.as_mut()
        else {
            return;
        };
        let node = *node;
        while self.matches[node as usize] >= *target {
            if *round == CATCH_UP_ROUNDS {
                if *round_beats < CATCH_UP_SHORT_ROUND {
                    let voters = self.voters().with(node);
                    self.append_config(voters, effects);
                } else {
                    self.abort_catch_up(effects);
                }
                return;
            }
            *round += 1;
            *target = last;
            *round_beats = 0;
        }
    }
    /// A heartbeat passed during catch-up (§8).
    fn catch_up_heartbeat(&mut self, effects: &mut Vec<Effect>) {
        let Some(PendingChange {
            phase:
                Phase::CatchingUp {
                    round_beats,
                    total_beats,
                    ..
                },
            ..
        }) = self.pending_change.as_mut()
        else {
            return;
        };
        *round_beats += 1;
        *total_beats += 1;
        if *total_beats > CATCH_UP_MAX_HEARTBEATS {
            self.abort_catch_up(effects);
        }
    }
    fn abort_catch_up(&mut self, effects: &mut Vec<Effect>) {
        let node = self.catching_up();
        if let Some(change) = self.pending_change.take() {
            effects.push(Effect::MembershipFailed {
                id: change.id,
                status: "CATCH_UP_FAILED",
            });
        }
        if let Some(node) = node {
            self.reset_peer(node);
        }
    }
    fn apply_committed(&mut self, effects: &mut Vec<Effect>) {
        while self.last_applied < self.commit_index {
            self.last_applied += 1;
            let entry = self
                .state
                .entry(self.last_applied)
                .expect("a committed entry after the base is held in the log");
            effects.push(Effect::Apply(entry.clone()));
        }
    }
    fn message(&mut self, message: Envelope, effects: &mut Vec<Effect>) {
        let term = message.body.term();
        // §9.4 (dissertation §4.2.3): within the minimum election timeout of
        // hearing from a leader, a RequestVote neither updates our term nor
        // gets our vote. A leader always counts as having heard one.
        if self.leader_contact && matches!(message.body, Rpc::RequestVote { .. }) {
            return self.reply(
                message.from,
                message.rpc_id,
                Rpc::VoteResponse {
                    term: self.state.term,
                    granted: false,
                },
                effects,
            );
        }
        if term > self.state.term {
            self.step_down(term, effects);
        }
        let Envelope {
            from, rpc_id, body, ..
        } = message;
        match body {
            Rpc::RequestVote {
                term,
                candidate_id,
                last_log_index,
                last_log_term,
            } => {
                let fresh = (last_log_term, last_log_index)
                    >= (
                        self.state.term_at(self.state.last_index()).unwrap(),
                        self.state.last_index(),
                    );
                let granted = term > 0
                    && term == self.state.term
                    && candidate_id == from
                    && fresh
                    && (self.state.voted_for.is_none()
                        || self.state.voted_for == Some(candidate_id));
                if granted {
                    self.state.voted_for = Some(candidate_id);
                    self.reset_election(effects);
                }
                self.reply(
                    from,
                    rpc_id,
                    Rpc::VoteResponse {
                        term: self.state.term,
                        granted,
                    },
                    effects,
                );
            }
            Rpc::VoteResponse { term, granted } => {
                if term == self.state.term
                    && self.role == Role::Candidate
                    && self.vote_requests.get(&from) == Some(&rpc_id)
                    && granted
                {
                    // Only voters of our configuration count (§9.2).
                    self.votes = self.votes.with(from);
                    if self.voters().is_quorum(self.votes) {
                        self.become_leader(effects);
                    }
                }
            }
            Rpc::AppendEntries {
                term,
                leader_id,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            } => {
                let valid_entries = entries.len() <= MAX_BATCH_ENTRIES
                    && prev_log_index.checked_add(entries.len() as u64).is_some()
                    && entries.iter().enumerate().all(|(i, e)| {
                        e.index == prev_log_index + i as u64 + 1
                            && e.term > 0
                            && e.term <= term
                            && e.command.valid()
                    });
                if term == 0 || term != self.state.term || leader_id != from || !valid_entries {
                    self.reply(
                        from,
                        rpc_id,
                        Rpc::AppendResponse {
                            term: self.state.term,
                            success: false,
                            matched_through: 0,
                            last_log_index: self.state.last_index(),
                        },
                        effects,
                    );
                    return;
                }
                self.step_down(term, effects);
                self.leader_hint = Some(from);
                self.leader_contact = true;
                self.reset_election(effects);
                // §9.4: entries up to our snapshot are committed, and a legitimate
                // leader holds the same ones, so they count as matching. Only the
                // entry at the base itself is checked, by term.
                let base = self.state.base();
                let (check_index, check_term, skip) = if prev_log_index < base.index {
                    let skip = (base.index - prev_log_index) as usize;
                    let at_base = entries.get(skip - 1).map_or(base.term, |e| e.term);
                    (base.index, at_base, skip.min(entries.len()))
                } else {
                    (prev_log_index, prev_log_term, 0)
                };
                let mut success = self.state.term_at(check_index) == Some(check_term);
                if success {
                    for entry in &entries[skip..] {
                        if let Some(existing) = self.state.entry(entry.index) {
                            if existing.term == entry.term {
                                if existing != entry {
                                    success = false;
                                    break;
                                }
                                continue;
                            }
                            if entry.index <= self.commit_index {
                                success = false;
                                break;
                            }
                            self.state.truncate_from(entry.index);
                        }
                        self.state.push(entry.clone());
                    }
                }
                let matched = if success {
                    prev_log_index + entries.len() as u64
                } else {
                    0
                };
                if success {
                    self.commit_index = self.commit_index.max(leader_commit.min(matched));
                    self.apply_committed(effects);
                }
                self.reply(
                    from,
                    rpc_id,
                    Rpc::AppendResponse {
                        term: self.state.term,
                        success,
                        matched_through: matched,
                        last_log_index: self.state.last_index(),
                    },
                    effects,
                );
            }
            Rpc::InstallSnapshot {
                term,
                last_included_index,
                last_included_term,
                voters,
                total_len,
                total_crc,
                offset,
                data,
                done,
                ..
            } => {
                let base = LogBase {
                    index: last_included_index,
                    term: last_included_term,
                };
                let reply = |core: &Core, effects: &mut Vec<Effect>, success, next, done| {
                    core.reply(
                        from,
                        rpc_id,
                        Rpc::InstallSnapshotResponse {
                            term: core.state.term,
                            success,
                            last_included_index,
                            next_offset: next,
                            done,
                        },
                        effects,
                    )
                };
                // Figure 13 step 1.
                if term != self.state.term {
                    return reply(self, effects, false, 0, false);
                }
                self.step_down(term, effects);
                self.leader_hint = Some(from);
                self.leader_contact = true;
                self.reset_election(effects);
                // §9.3 step 2: we already hold and have committed everything it covers.
                if last_included_index <= self.commit_index {
                    self.receiving = None;
                    return reply(self, effects, true, total_len, true);
                }
                let chunk = from_hex(&data).expect("validated by Envelope::valid_for");
                if offset == 0 {
                    self.receiving = Some(Receiving {
                        term,
                        base,
                        total_len,
                        total_crc,
                        data: Vec::with_capacity(total_len as usize),
                    });
                }
                let Some(receiving) = self.receiving.as_mut().filter(|r| {
                    r.term == term
                        && r.base == base
                        && r.total_len == total_len
                        && r.total_crc == total_crc
                }) else {
                    // Some other snapshot, or nothing started: begin again.
                    self.receiving = None;
                    return reply(self, effects, false, 0, false);
                };
                if receiving.data.len() as u64 != offset {
                    let have = receiving.data.len() as u64;
                    return reply(self, effects, false, have, false);
                }
                receiving.data.extend_from_slice(&chunk);
                let have = receiving.data.len() as u64;
                if !done {
                    return reply(self, effects, true, have, false);
                }
                let received = self.receiving.take().expect("present");
                if have != total_len || crc32c(&received.data) != total_crc {
                    return reply(self, effects, false, 0, false);
                }
                let snapshot = Snapshot {
                    base,
                    voters,
                    data: received.data.into(),
                };
                if !snapshot.valid() {
                    return reply(self, effects, false, 0, false);
                }
                // Figure 13 steps 6-8. Commit and apply jump to the snapshot; the
                // reply below is executed only after the PersistSnapshot barrier.
                self.state = adopt_snapshot(&self.state, base);
                self.base_voters = Voters::from_bits(voters);
                self.commit_index = last_included_index;
                self.last_applied = last_included_index;
                self.requested_snapshot = Some((snapshot, true));
                reply(self, effects, true, total_len, true);
            }
            Rpc::InstallSnapshotResponse {
                term,
                success,
                last_included_index,
                next_offset,
                done,
            } => {
                if term != self.state.term || self.role != Role::Leader {
                    return;
                }
                let p = from as usize;
                let Some(request) = self.inflight[p].clone() else {
                    return;
                };
                let Some(chunk_base) = request.chunk else {
                    return;
                };
                if request.rpc_id != rpc_id
                    || request.term != term
                    || request.epoch != self.epoch
                    || chunk_base.index != last_included_index
                {
                    return;
                }
                self.inflight[p] = None;
                // Same term: this follower still accepted us as leader (§6.2 step 5).
                self.acked_round[p] = self.acked_round[p].max(request.round);
                self.finish_reads(effects);
                if success && done {
                    self.transfer[p] = None;
                    self.matches[p] = self.matches[p].max(last_included_index);
                    self.next[p] = self.matches[p] + 1;
                    self.catch_up_progress(effects);
                } else if let Some((base, _)) = self.transfer[p] {
                    // Continue (or resend) from where the follower says it is.
                    self.transfer[p] = Some((base, next_offset));
                }
                self.replicate(from, effects);
                self.replicate_for_reads(from, effects);
            }
            Rpc::AppendResponse {
                term,
                success,
                matched_through,
                last_log_index,
            } => {
                if term != self.state.term || self.role != Role::Leader {
                    return;
                }
                let p = from as usize;
                let Some(request) = self.inflight[p].clone() else {
                    return;
                };
                if request.rpc_id != rpc_id
                    || request.term != term
                    || request.epoch != self.epoch
                    || request.chunk.is_some()
                {
                    return;
                }
                self.inflight[p] = None;
                // Same term, success or not: this follower still accepts us (§6.2 step 5).
                self.acked_round[p] = self.acked_round[p].max(request.round);
                self.finish_reads(effects);
                if success {
                    let confirmed = request.prev + request.count;
                    if matched_through != confirmed {
                        return;
                    }
                    self.matches[p] = self.matches[p].max(confirmed);
                    self.next[p] = self.matches[p] + 1;
                    self.catch_up_progress(effects);
                    let old_commit = self.commit_index;
                    self.advance_commit(effects);
                    if self.commit_index > old_commit {
                        self.start_read_round(effects);
                        self.finish_reads(effects);
                        self.replicate_all(effects);
                    }
                    if self.next[p] <= self.state.last_index() || request.commit < self.commit_index
                    {
                        self.replicate(from, effects);
                    }
                } else {
                    // §4.2.1: the follower's log length caps nextIndex, so an
                    // empty new node is found in one round trip.
                    self.next[p] = self.next[p]
                        .saturating_sub(1)
                        .min(last_log_index.saturating_add(1))
                        .max(self.matches[p] + 1)
                        .max(1);
                    self.replicate(from, effects);
                }
                self.replicate_for_reads(from, effects);
            }
        }
    }
}

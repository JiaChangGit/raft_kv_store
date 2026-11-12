//! Pure event-driven Raft. Complete every ordered durability barrier before the next event.
use raft_protocol::*;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
}
#[derive(Clone, Debug)]
pub enum Event {
    ElectionTimeout { generation: u64 },
    Heartbeat { generation: u64 },
    Message(Envelope),
    Propose(ClientRequest),
    RpcTimeout { peer: NodeId, rpc_id: u64 },
}
#[derive(Clone, Debug)]
pub enum Effect {
    Persist {
        barrier_id: u64,
        state: DurableState,
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
}

#[derive(Clone, Debug)]
struct Inflight {
    rpc_id: u64,
    term: u64,
    epoch: u64,
    prev: u64,
    count: u64,
    commit: u64,
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
    votes: BTreeSet<NodeId>,
    vote_requests: BTreeMap<NodeId, u64>,
    next: [u64; 4],
    matches: [u64; 4],
    inflight: [Option<Inflight>; 4],
    epoch: u64,
    serial: u64,
    barrier_serial: u64,
    pending_barrier: Option<u64>,
    failed: bool,
}
impl Core {
    pub fn new(id: NodeId, state: DurableState) -> Result<Self, String> {
        if !valid_node(id) || !state.valid() {
            return Err("invalid identity or durable state".into());
        }
        let durable_index = state.last_index();
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
            votes: BTreeSet::new(),
            vote_requests: BTreeMap::new(),
            next: [1; 4],
            matches: [0; 4],
            inflight: std::array::from_fn(|_| None),
            epoch: 0,
            serial: 0,
            barrier_serial: 0,
            pending_barrier: None,
            failed: false,
        })
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
        self.durable_index = self.state.last_index();
        if self.role == Role::Candidate && self.state.voted_for == Some(self.id) {
            self.votes.insert(self.id);
        }
        let mut effects = vec![];
        if self.role == Role::Leader {
            self.matches[self.id as usize] = self.durable_index;
            self.advance_commit(&mut effects);
        }
        effects
    }
    pub fn step(&mut self, event: Event) -> Vec<Effect> {
        if self.failed || self.pending_barrier.is_some() {
            return vec![];
        }
        let old = self.state.clone();
        let mut effects = vec![];
        match event {
            Event::ElectionTimeout { generation }
                if generation == self.election_generation && self.role != Role::Leader =>
            {
                self.begin_election(&mut effects)
            }
            Event::Heartbeat { generation }
                if generation == self.heartbeat_generation && self.role == Role::Leader =>
            {
                self.replicate_all(&mut effects);
                self.heartbeat_generation += 1;
                effects.push(Effect::ResetHeartbeat {
                    generation: self.heartbeat_generation,
                });
            }
            Event::Message(message) if message.valid_for(self.id) => {
                self.message(message, &mut effects)
            }
            Event::RpcTimeout { peer, rpc_id } if valid_node(peer) && self.role == Role::Leader => {
                if self.inflight[peer as usize]
                    .as_ref()
                    .is_some_and(|r| r.rpc_id == rpc_id)
                {
                    self.inflight[peer as usize] = None;
                    self.replicate(peer, &mut effects);
                }
            }
            Event::Propose(request) => {
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
                    self.replicate_all(&mut effects);
                }
            }
            _ => {}
        }
        if self.state != old {
            self.barrier_serial += 1;
            self.pending_barrier = Some(self.barrier_serial);
            effects.insert(
                0,
                Effect::Persist {
                    barrier_id: self.barrier_serial,
                    state: self.state.clone(),
                },
            );
        }
        effects
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
        self.votes.clear();
        self.vote_requests.clear();
        self.inflight = std::array::from_fn(|_| None);
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
        self.votes.clear();
        self.vote_requests.clear();
        self.inflight = std::array::from_fn(|_| None);
        self.reset_election(effects);
        for peer in NODES
            .into_iter()
            .filter(|n| *n != self.id)
            .collect::<Vec<_>>()
        {
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
        self.election_generation += 1;
        self.heartbeat_generation += 1;
        self.epoch += 1;
        self.next = [self.state.last_index() + 1; 4];
        self.matches = [0; 4];
        self.matches[self.id as usize] = self.durable_index;
        self.inflight = std::array::from_fn(|_| None);
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
        for peer in NODES {
            if peer != self.id {
                self.replicate(peer, effects);
            }
        }
    }
    fn replicate(&mut self, peer: NodeId, effects: &mut Vec<Effect>) {
        let p = peer as usize;
        if self.role != Role::Leader || self.inflight[p].is_some() {
            return;
        }
        let next = self.next[p].clamp(1, self.state.last_index() + 1);
        let prev = next - 1;
        let Some(prev_log_term) = self.state.term_at(prev) else {
            unreachable!("log compaction is not enabled yet; entry {prev} was discarded")
        };
        let entries: Vec<_> = self
            .state
            .entries_from(next)
            .expect("next lies after the base")
            .iter()
            .take(32)
            .cloned()
            .collect();
        let count = entries.len() as u64;
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
        });
    }
    fn advance_commit(&mut self, effects: &mut Vec<Effect>) {
        for n in (self.commit_index + 1..=self.durable_index).rev() {
            let count = NODES
                .into_iter()
                .filter(|id| self.matches[*id as usize] >= n)
                .count();
            // Figure 2 / §5.4.2: previous-term majority alone cannot directly commit.
            if self.state.term_at(n) == Some(self.state.term) && count >= QUORUM {
                self.commit_index = n;
                break;
            }
        }
        self.apply_committed(effects);
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
                    self.votes.insert(from);
                    if self.votes.len() >= QUORUM {
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
                let valid_entries = entries.len() <= 32
                    && prev_log_index.checked_add(entries.len() as u64).is_some()
                    && entries.iter().enumerate().all(|(i, e)| {
                        e.index == prev_log_index + i as u64 + 1
                            && e.term > 0
                            && e.term <= term
                            && match &e.command {
                                Command::NoOp => true,
                                Command::Client(r) => r.operation.valid(),
                            }
                    });
                if term == 0 || term != self.state.term || leader_id != from || !valid_entries {
                    self.reply(
                        from,
                        rpc_id,
                        Rpc::AppendResponse {
                            term: self.state.term,
                            success: false,
                            matched_through: 0,
                        },
                        effects,
                    );
                    return;
                }
                self.step_down(term, effects);
                self.leader_hint = Some(from);
                self.reset_election(effects);
                let mut success = self.state.term_at(prev_log_index) == Some(prev_log_term);
                if success {
                    for entry in &entries {
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
                    },
                    effects,
                );
            }
            Rpc::AppendResponse {
                term,
                success,
                matched_through,
            } => {
                if term != self.state.term || self.role != Role::Leader {
                    return;
                }
                let p = from as usize;
                let Some(request) = self.inflight[p].clone() else {
                    return;
                };
                if request.rpc_id != rpc_id || request.term != term || request.epoch != self.epoch {
                    return;
                }
                self.inflight[p] = None;
                if success {
                    let confirmed = request.prev + request.count;
                    if matched_through != confirmed {
                        return;
                    }
                    self.matches[p] = self.matches[p].max(confirmed);
                    self.next[p] = self.matches[p] + 1;
                    let old_commit = self.commit_index;
                    self.advance_commit(effects);
                    if self.commit_index > old_commit {
                        self.replicate_all(effects);
                    }
                    if self.next[p] <= self.state.last_index() || request.commit < self.commit_index
                    {
                        self.replicate(from, effects);
                    }
                } else {
                    self.next[p] = self.next[p]
                        .saturating_sub(1)
                        .max(self.matches[p] + 1)
                        .max(1);
                    self.replicate(from, effects);
                }
            }
        }
    }
}

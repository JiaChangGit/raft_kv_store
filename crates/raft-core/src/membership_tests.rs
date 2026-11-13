//! S4 core behaviour: membership changes. docs/MEMBERSHIP.md §6-§9 and §15.
use super::*;

fn voters(ids: &[NodeId]) -> Voters {
    Voters::from_ids(ids).unwrap()
}
fn config(index: u64, term: u64, ids: &[NodeId]) -> LogEntry {
    LogEntry {
        index,
        term,
        command: Command::Config {
            voters: ids.to_vec(),
        },
    }
}
fn noop(index: u64, term: u64) -> LogEntry {
    LogEntry {
        index,
        term,
        command: Command::NoOp,
    }
}
fn put(sequence: u64) -> ClientRequest {
    ClientRequest {
        client_id: 7,
        sequence,
        operation: Operation::Put {
            key: "k".into(),
            value: sequence.to_string(),
        },
    }
}

/// A tiny network of real cores. Barriers complete at once; messages wait in
/// `queue` until delivered or dropped. Every applied entry is checked against
/// what any node applied at that index before (State Machine Safety).
struct Net {
    cores: BTreeMap<NodeId, Core>,
    queue: Vec<Envelope>,
    applied: BTreeMap<LogIndex, LogEntry>,
    /// The last index each node applied: applies must be consecutive.
    last_applied: BTreeMap<NodeId, LogIndex>,
    answers: Vec<(u64, Result<Voters, &'static str>)>,
    reads: Vec<(u64, Result<LogIndex, &'static str>)>,
}
impl Net {
    fn new(nodes: &[(NodeId, DurableState)]) -> Self {
        Self {
            cores: nodes
                .iter()
                .map(|(id, state)| (*id, Core::new(*id, state.clone()).unwrap()))
                .collect(),
            queue: vec![],
            applied: BTreeMap::new(),
            last_applied: BTreeMap::new(),
            answers: vec![],
            reads: vec![],
        }
    }
    fn core(&self, id: NodeId) -> &Core {
        &self.cores[&id]
    }
    fn run(&mut self, id: NodeId, event: Event) {
        let effects = self.cores.get_mut(&id).unwrap().step(event);
        self.execute(id, effects);
    }
    fn execute(&mut self, id: NodeId, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Persist { barrier_id, .. } | Effect::PersistSnapshot { barrier_id, .. } => {
                    let more = self.cores.get_mut(&id).unwrap().persisted(barrier_id);
                    self.execute(id, more);
                }
                Effect::Send(message) => self.queue.push(message),
                Effect::Apply(entry) => {
                    let last = self.last_applied.entry(id).or_insert(0);
                    assert_eq!(
                        entry.index,
                        *last + 1,
                        "node {id} must apply in index order (lesson 09)"
                    );
                    *last = entry.index;
                    let first = self.applied.entry(entry.index).or_insert(entry.clone());
                    assert_eq!(
                        *first, entry,
                        "State Machine Safety: node {id} applied a different entry at index {}",
                        entry.index
                    );
                }
                Effect::MembershipDone {
                    id: request,
                    voters,
                    ..
                } => self.answers.push((request, Ok(voters))),
                Effect::MembershipFailed {
                    id: request,
                    status,
                } => self.answers.push((request, Err(status))),
                Effect::ReadReady { id, read_index } => self.reads.push((id, Ok(read_index))),
                Effect::ReadFailed { id, status } => self.reads.push((id, Err(status))),
                _ => {}
            }
        }
    }
    /// Deliver queued messages over the links `allowed` permits, and the
    /// messages they cause, until none is left; everything else is lost.
    fn deliver(&mut self, allowed: impl Fn(NodeId, NodeId) -> bool) {
        loop {
            let (now, _lost): (Vec<_>, Vec<_>) = std::mem::take(&mut self.queue)
                .into_iter()
                .partition(|m| allowed(m.from, m.to) && self.cores.contains_key(&m.to));
            if now.is_empty() {
                return;
            }
            for message in now {
                self.run(message.to, Event::Message(message));
            }
        }
    }
    /// Deliver only the first queued message from `from` to `to`; keep the rest.
    fn deliver_one(&mut self, from: NodeId, to: NodeId) {
        let at = self
            .queue
            .iter()
            .position(|m| m.from == from && m.to == to)
            .unwrap_or_else(|| panic!("no message {from} -> {to} in {:?}", self.queue));
        let message = self.queue.remove(at);
        self.run(to, Event::Message(message));
    }
    fn sent_to(&self, to: NodeId) -> usize {
        self.queue.iter().filter(|m| m.to == to).count()
    }
    fn timeout(&mut self, id: NodeId) {
        let generation = self.core(id).election_generation;
        self.run(id, Event::ElectionTimeout { generation });
    }
    fn expire_contact(&mut self, id: NodeId) {
        let generation = self.core(id).election_generation;
        self.run(id, Event::LeaderContactExpired { generation });
    }
    fn heartbeat(&mut self, id: NodeId) {
        let generation = self.core(id).heartbeat_generation;
        self.run(id, Event::Heartbeat { generation });
    }
    /// A crash and restart: only durable state survives (all of it is durable here).
    fn restart(&mut self, id: NodeId) {
        let core = &self.cores[&id];
        let restored = Core::restore(id, core.state.clone(), core.snapshot().cloned()).unwrap();
        self.last_applied.insert(id, restored.last_applied);
        self.cores.insert(id, restored);
    }
    fn change(&mut self, leader: NodeId, request: u64, change: Change) {
        self.run(
            leader,
            Event::ChangeMembership {
                id: request,
                change,
            },
        );
    }
}
fn all(_: NodeId, _: NodeId) -> bool {
    true
}

/// Node 1 bootstrapped alone (dissertation §4.4), elected and settled; nodes
/// in `fresh` start with empty logs.
fn bootstrapped(fresh: &[NodeId]) -> Net {
    let mut nodes = vec![(1, bootstrap_state(1))];
    nodes.extend(fresh.iter().map(|id| (*id, DurableState::default())));
    let mut net = Net::new(&nodes);
    net.timeout(1);
    assert_eq!(net.core(1).role, Role::Leader);
    assert_eq!(net.core(1).commit_index, 2, "configuration and no-op");
    net
}
/// Nodes 1..=n all hold a committed configuration of 1..=n; node 1 leads term 2
/// and every node knows that index 2 (its no-op) is committed.
fn settled(n: NodeId) -> Net {
    let ids: Vec<NodeId> = (1..=n).collect();
    let state = DurableState::new(1, None, vec![config(1, 1, &ids)]);
    let nodes: Vec<_> = ids.iter().map(|id| (*id, state.clone())).collect();
    let mut net = Net::new(&nodes);
    net.timeout(1);
    net.deliver(all);
    net.heartbeat(1);
    net.deliver(all);
    assert_eq!(net.core(1).role, Role::Leader);
    for id in ids {
        assert_eq!(net.core(id).commit_index, 2, "node {id}");
    }
    net
}

#[test]
fn a_bootstrapped_node_elects_itself_once_its_vote_is_durable() {
    let mut n = Core::new(1, bootstrap_state(1)).unwrap();
    assert_eq!(n.configuration(), Some((1, voters(&[1]))));
    let fx = n.step(Event::ElectionTimeout { generation: 0 });
    let Some(Effect::Persist { barrier_id, state }) = fx.first() else {
        panic!("the vote is a barrier: {fx:?}");
    };
    assert_eq!((state.term, state.voted_for), (2, Some(1)));
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::Send(_))),
        "nobody to ask"
    );
    assert_eq!(n.role, Role::Candidate, "not before the vote is durable");
    let fx = n.persisted(*barrier_id);
    assert_eq!(n.role, Role::Leader);
    let Some(Effect::Persist { barrier_id, state }) = fx.first() else {
        panic!("the no-op is a barrier: {fx:?}");
    };
    assert_eq!(state.last_index(), 2);
    assert_eq!(n.commit_index, 0, "not before the no-op is durable");
    let fx = n.persisted(*barrier_id);
    assert_eq!(n.commit_index, 2);
    let applied: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Apply(entry) => Some(entry.index),
            _ => None,
        })
        .collect();
    assert_eq!(applied, [1, 2]);
}

#[test]
fn a_node_without_configuration_never_campaigns() {
    let mut n = Core::new(2, DurableState::default()).unwrap();
    assert_eq!(n.configuration(), None);
    assert!(n.step(Event::ElectionTimeout { generation: 0 }).is_empty());
    assert_eq!((n.role, n.state.term), (Role::Follower, 0));
    // It still votes: RPCs are not judged by the receiver's configuration (§4.1).
    let fx = n.step(Event::Message(Envelope {
        version: 1,
        cluster_id: CLUSTER.into(),
        from: 1,
        to: 2,
        rpc_id: 1,
        body: Rpc::RequestVote {
            term: 2,
            candidate_id: 1,
            last_log_index: 1,
            last_log_term: 1,
        },
    }));
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Send(Envelope {
            body: Rpc::VoteResponse { granted: true, .. },
            ..
        })
    )));
}

#[test]
fn a_new_node_is_found_in_one_round_trip_and_added_after_ten_rounds() {
    let mut net = bootstrapped(&[2]);
    net.change(1, 10, Change::Add(2));
    assert_eq!(net.core(1).catching_up(), Some(2));
    let Some(Rpc::AppendEntries { prev_log_index, .. }) = net.queue.first().map(|m| &m.body) else {
        panic!("the leader starts replicating to the new node");
    };
    assert_eq!(
        *prev_log_index, 2,
        "the leader does not know the node is empty"
    );
    net.deliver_one(1, 2);
    let Some(Rpc::AppendResponse {
        success: false,
        last_log_index: 0,
        ..
    }) = net.queue.first().map(|m| &m.body)
    else {
        panic!(
            "an empty node rejects and reports its length: {:?}",
            net.queue
        );
    };
    net.deliver_one(2, 1);
    let Some(Rpc::AppendEntries {
        prev_log_index,
        entries,
        ..
    }) = net.queue.first().map(|m| &m.body)
    else {
        panic!("the leader retries");
    };
    assert_eq!(
        (*prev_log_index, entries.len()),
        (0, 2),
        "straight to index 1 (§4.2.1)"
    );
    net.deliver(all);
    // No new entries arrived, so all ten rounds finished at once; the
    // configuration entry was then appended and committed by both voters.
    assert_eq!(net.answers, [(10, Ok(voters(&[1, 2])))]);
    assert_eq!(net.core(1).configuration(), Some((3, voters(&[1, 2]))));
    assert_eq!(net.core(2).configuration(), Some((3, voters(&[1, 2]))));
    assert_eq!(net.core(1).catching_up(), None);
}

/// Drive a catch-up of node 2 in which every round has a new target, with
/// `beats` heartbeats during round 10.
fn catch_up_with_slow_last_round(beats: u32) -> Net {
    let mut net = bootstrapped(&[2]);
    net.change(1, 11, Change::Add(2));
    net.deliver_one(1, 2);
    net.deliver_one(2, 1); // empty: rejected, length 0
    for round in 1..CATCH_UP_ROUNDS {
        // A write arrives while this round's request is in flight, so the
        // next round has something new to replicate.
        net.run(1, Event::Propose(put(round as u64)));
        net.deliver_one(1, 2);
        net.deliver_one(2, 1);
        assert_eq!(net.core(1).catching_up(), Some(2), "round {round}");
    }
    for _ in 0..beats {
        net.heartbeat(1);
    }
    net.deliver(all);
    net
}

#[test]
fn the_last_round_must_be_shorter_than_the_minimum_election_timeout() {
    let quick = catch_up_with_slow_last_round(CATCH_UP_SHORT_ROUND - 1);
    assert_eq!(quick.answers, [(11, Ok(voters(&[1, 2])))]);
    let slow = catch_up_with_slow_last_round(CATCH_UP_SHORT_ROUND);
    assert_eq!(slow.answers, [(11, Err("CATCH_UP_FAILED"))]);
    assert_eq!(
        slow.core(1).configuration().map(|(_, v)| v),
        Some(voters(&[1]))
    );
    assert_eq!(slow.core(1).catching_up(), None);
}

#[test]
fn an_unreachable_node_is_given_up_after_one_hundred_heartbeats() {
    let mut net = bootstrapped(&[]);
    net.change(1, 12, Change::Add(2));
    for _ in 0..CATCH_UP_MAX_HEARTBEATS {
        net.heartbeat(1);
    }
    assert!(net.answers.is_empty(), "still trying after exactly 100");
    net.heartbeat(1);
    assert_eq!(net.answers, [(12, Err("CATCH_UP_FAILED"))]);
    assert_eq!(net.core(1).configuration(), Some((1, voters(&[1]))));
    net.queue.clear();
    net.heartbeat(1);
    assert_eq!(net.sent_to(2), 0, "nothing more is sent to it");
}

#[test]
fn removing_a_follower_and_then_the_leader() {
    let mut net = settled(3);
    net.change(1, 20, Change::Remove(3));
    assert_eq!(net.core(1).configuration(), Some((3, voters(&[1, 2]))));
    assert!(
        net.queue.iter().all(|m| m.to == 2),
        "node 3 gets nothing more"
    );
    net.deliver(all);
    assert_eq!(net.answers, [(20, Ok(voters(&[1, 2])))]);

    // Now the leader removes itself (§4.2.2): it keeps leading without
    // counting itself, and steps down once the entry is committed.
    net.change(1, 21, Change::Remove(1));
    assert_eq!(net.core(1).configuration(), Some((4, voters(&[2]))));
    let durable_alone = net.core(1).durable_index;
    assert_eq!(durable_alone, 4);
    assert_eq!(net.core(1).commit_index, 3, "its own copy does not count");
    assert_eq!(net.core(1).role, Role::Leader);
    net.deliver(all);
    assert_eq!(net.answers[1], (21, Ok(voters(&[2]))));
    assert_eq!(net.core(1).role, Role::Follower);
    assert_eq!(net.core(1).state.term, 2, "stepping down keeps the term");
    // It knows it was removed: no more elections (§9.2).
    net.timeout(1);
    assert_eq!(net.core(1).role, Role::Follower);
    assert!(net.queue.is_empty());
    // Node 2, now alone in its configuration, takes over.
    net.expire_contact(2);
    net.timeout(2);
    assert_eq!(net.core(2).role, Role::Leader);
}

#[test]
fn change_requests_are_checked_and_one_runs_at_a_time() {
    let mut net = settled(3);
    net.change(2, 1, Change::Add(4));
    net.change(1, 2, Change::Add(0));
    net.change(1, 3, Change::Add(8));
    net.change(1, 4, Change::Add(2));
    net.change(1, 5, Change::Remove(5));
    assert_eq!(
        net.answers,
        [
            (1, Err("NOT_LEADER")),
            (2, Err("INVALID_ARGUMENT")),
            (3, Err("INVALID_ARGUMENT")),
            (4, Ok(voters(&[1, 2, 3]))),
            (5, Ok(voters(&[1, 2, 3]))),
        ],
        "already satisfied changes succeed at once"
    );
    net.answers.clear();
    net.change(1, 6, Change::Add(4));
    assert_eq!(net.core(1).catching_up(), Some(4));
    net.change(1, 7, Change::Remove(3));
    assert_eq!(net.answers, [(7, Err("CHANGE_IN_PROGRESS"))]);
    // Cancelling forgets the request: no answer, no more catch-up traffic.
    net.queue.retain(|m| m.to != 4);
    net.run(1, Event::CancelMembership { id: 6 });
    assert_eq!(net.core(1).catching_up(), None);
    net.heartbeat(1);
    assert_eq!(net.sent_to(4), 0);
    net.deliver(all);
    // An appended but uncommitted configuration also blocks the next change.
    net.answers.clear();
    net.change(1, 8, Change::Remove(3));
    assert_eq!(net.core(1).configuration(), Some((3, voters(&[1, 2]))));
    net.change(1, 9, Change::Remove(2));
    assert_eq!(net.answers, [(9, Err("CHANGE_IN_PROGRESS"))]);
    // Even once its request is cancelled, the entry stays in the log and
    // blocks changes until it is committed.
    net.run(1, Event::CancelMembership { id: 8 });
    net.change(1, 10, Change::Remove(2));
    assert_eq!(net.answers[1], (10, Err("CHANGE_IN_PROGRESS")));
    net.deliver(all);
    assert_eq!(net.core(1).commit_index, 3);
    net.change(1, 11, Change::Remove(2));
    assert_eq!(
        net.answers[2],
        (11, Ok(voters(&[1]))),
        "committed: the next change runs"
    );
    // Removing the last voter is refused.
    let mut alone = bootstrapped(&[]);
    alone.change(1, 1, Change::Remove(1));
    assert_eq!(alone.answers, [(1, Err("INVALID_ARGUMENT"))]);
}

#[test]
fn losing_leadership_answers_the_pending_change() {
    // Before anything was appended: no change at all.
    let mut net = settled(3);
    net.change(1, 30, Change::Add(4));
    net.queue.clear();
    net.run(1, Event::Message(new_term_append(3, 1, 5)));
    assert_eq!(net.answers, [(30, Err("NOT_LEADER"))]);
    // After the configuration entry was appended: the outcome is unknown.
    let mut net = settled(3);
    net.change(1, 31, Change::Remove(3));
    net.queue.clear();
    net.run(1, Event::Message(new_term_append(2, 1, 5)));
    assert_eq!(net.answers, [(31, Err("TIMEOUT_UNKNOWN"))]);
}
/// An empty AppendEntries from `from`, leader of `term`.
fn new_term_append(from: NodeId, to: NodeId, term: Term) -> Envelope {
    Envelope {
        version: 1,
        cluster_id: CLUSTER.into(),
        from,
        to,
        rpc_id: 99,
        body: Rpc::AppendEntries {
            term,
            leader_id: from,
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        },
    }
}

/// Found by the simulator (lesson 09): in one step the leader applies its
/// no-op and, the 2015 condition now holding, appends a configuration that
/// it alone commits once durable. That later commit happens inside the
/// barrier's completion, so the no-op's Apply must come before the barrier.
#[test]
fn applies_stay_in_order_when_a_barrier_commits_more() {
    let state = DurableState::new(1, None, vec![config(1, 1, &[1, 2])]);
    let mut net = Net::new(&[(1, state.clone()), (2, state)]);
    net.timeout(1);
    net.deliver_one(1, 2);
    net.deliver_one(2, 1);
    assert_eq!(net.core(1).role, Role::Leader);
    net.change(1, 1, Change::Remove(2)); // waits: the no-op is not committed
    let ack = {
        net.deliver_one(1, 2);
        net.queue.remove(0)
    };
    let effects = net.cores.get_mut(&1).unwrap().step(Event::Message(ack));
    let kinds: Vec<&str> = effects
        .iter()
        .map(|e| match e {
            Effect::Apply(_) => "apply",
            Effect::Persist { .. } => "persist",
            _ => "other",
        })
        .filter(|kind| *kind != "other")
        .collect();
    // Indexes 1 and 2 become committed here; both applies precede the barrier.
    assert_eq!(
        kinds,
        ["apply", "apply", "persist"],
        "the applies precede the barrier"
    );
    net.execute(1, effects);
    assert_eq!(net.answers, [(1, Ok(voters(&[1])))]);
    assert_eq!(net.last_applied[&1], 3, "no-op (2), then configuration (3)");
}

#[test]
fn a_new_leader_waits_for_a_committed_entry_of_its_term() {
    let state = DurableState::new(1, None, vec![config(1, 1, &[1, 2, 3])]);
    let mut net = Net::new(&[(1, state.clone()), (2, state.clone()), (3, state)]);
    net.timeout(1);
    net.deliver_one(1, 2);
    net.deliver_one(2, 1);
    // Elected, but its no-op has not reached anyone yet.
    assert_eq!(net.core(1).role, Role::Leader);
    assert_eq!(net.core(1).commit_index, 0);
    net.change(1, 40, Change::Remove(3));
    assert_eq!(
        net.core(1).configuration(),
        Some((1, voters(&[1, 2, 3]))),
        "the 2015 fix: nothing is appended before an entry of this term commits"
    );
    assert!(
        net.answers.is_empty(),
        "the request waits instead of failing: {:?}",
        net.answers
    );
    net.deliver(all);
    assert_eq!(
        net.core(1).configuration().map(|(_, v)| v),
        Some(voters(&[1, 2]))
    );
    assert_eq!(net.answers, [(40, Ok(voters(&[1, 2])))]);
}

/// The first counterexample of the 2015 raft-dev post, step by step
/// (docs/reference/raft-notes.md): C = {1,2,3,4}; S1 adds S5 with D = {1..5}
/// that reaches S5 only; S2 then wins and removes S1 with E = {2,3,4}, whose
/// messages reach S3 only; S1 comes back with the votes of S4 and S5.
#[test]
fn the_2015_counterexample_cannot_lose_a_committed_entry() {
    let c = [1, 2, 3, 4];
    let state = DurableState::new(1, None, vec![config(1, 1, &c)]);
    let mut nodes: Vec<_> = c.iter().map(|id| (*id, state.clone())).collect();
    nodes.push((5, DurableState::default()));
    let mut net = Net::new(&nodes);
    let not_5 = |from: NodeId, to: NodeId| from != 5 && to != 5;
    // S1 leads term 2 of C = {1,2,3,4}; its no-op (index 2) is committed everywhere.
    net.timeout(1);
    net.deliver(not_5);
    net.heartbeat(1);
    net.deliver(not_5);
    assert_eq!(net.core(1).role, Role::Leader);
    // S1 adds S5: the catch-up talks to S5 only; D = {1..5} (index 3) is then
    // appended and reaches S5 only. S1 crashes.
    net.change(1, 1, Change::Add(5));
    net.deliver(|from, to| matches!((from, to), (1, 5) | (5, 1)));
    assert_eq!(
        net.core(5).configuration(),
        Some((3, voters(&[1, 2, 3, 4, 5])))
    );
    net.queue.clear();
    net.restart(1);
    // S2 wins term 3 with the votes of S3 and S4 (C needs three of four).
    net.expire_contact(3);
    net.expire_contact(4);
    net.timeout(2);
    for voter in [3, 4] {
        net.deliver_one(2, voter);
        net.deliver_one(voter, 2);
    }
    assert_eq!(
        (net.core(2).role, net.core(2).state.term),
        (Role::Leader, 3)
    );
    // S2 is asked to remove S1: E = {2,3,4}. Its messages reach S3 only.
    net.change(2, 2, Change::Remove(1));
    net.deliver(|from, to| matches!((from, to), (2, 3) | (3, 2)));
    net.heartbeat(2);
    net.deliver(|from, to| matches!((from, to), (2, 3) | (3, 2)));
    // The fix: S2's no-op (index 3) is not committed (S2 and S3 are two of
    // four), so E was never appended. Without the fix E would be appended at
    // index 4 and committed by S2 and S3, the majority of E.
    assert!(
        net.core(2)
            .state
            .entries()
            .iter()
            .all(|e| e.command.config() != Some(voters(&[2, 3, 4]))),
        "E must not be appended before an entry of S2's term is committed"
    );
    assert_eq!(net.core(2).commit_index, 2);
    // S1 comes back and wins term 4 with the votes of S4 and S5, the
    // majority of D. It replicates D over index 3 on S4 and commits.
    net.queue.clear();
    net.expire_contact(5);
    net.timeout(1); // term 3: S3 and S4 already voted in it
    net.queue.clear();
    net.timeout(1); // term 4
    let s1_links =
        |from: NodeId, to: NodeId| matches!((from, to), (1, 4) | (4, 1) | (1, 5) | (5, 1));
    net.deliver(s1_links);
    assert_eq!(
        (net.core(1).role, net.core(1).state.term),
        (Role::Leader, 4)
    );
    net.heartbeat(1);
    net.deliver(s1_links);
    // `Net` already checked every applied entry against the others: nothing
    // committed by S2 and S3 was overwritten, because nothing was.
    assert!(net.core(1).commit_index >= 4);
    assert_eq!(
        net.applied.get(&3).map(|e| e.command.config()),
        Some(Some(voters(&[1, 2, 3, 4, 5])))
    );
}

#[test]
fn after_a_term_commit_a_stale_configuration_cannot_win() {
    let c = [1, 2, 3, 4];
    let state = DurableState::new(1, None, vec![config(1, 1, &c)]);
    let mut nodes: Vec<_> = c.iter().map(|id| (*id, state.clone())).collect();
    nodes.push((5, DurableState::default()));
    let mut net = Net::new(&nodes);
    let not_5 = |from: NodeId, to: NodeId| from != 5 && to != 5;
    net.timeout(1);
    net.deliver(not_5);
    net.heartbeat(1);
    net.deliver(not_5);
    net.change(1, 1, Change::Add(5));
    net.deliver(|from, to| matches!((from, to), (1, 5) | (5, 1)));
    net.queue.clear();
    net.restart(1);
    net.expire_contact(3);
    net.expire_contact(4);
    net.timeout(2);
    for voter in [3, 4] {
        net.deliver_one(2, voter);
        net.deliver_one(voter, 2);
    }
    // This time S2's no-op reaches S3 and S4: it commits under C, and then E
    // is appended and committed.
    net.change(2, 2, Change::Remove(1));
    net.deliver(|from, to| from != 1 && to != 1 && from != 5 && to != 5);
    assert_eq!(net.answers.last(), Some(&(2, Ok(voters(&[2, 3, 4])))));
    // S1 (with D) asks S4 and S5 in ever higher terms: S4 holds an entry of
    // term 3, so S1's log is not up to date and S1 never wins.
    net.queue.clear();
    for _ in 0..3 {
        net.expire_contact(4);
        net.expire_contact(5);
        net.timeout(1);
        net.deliver(|from, to| matches!((from, to), (1, 4) | (4, 1) | (1, 5) | (5, 1)));
        assert_ne!(net.core(1).role, Role::Leader);
    }
}

#[test]
fn a_recent_leader_contact_blocks_votes_without_a_term_change() {
    let mut net = settled(3);
    // Node 3 heard from the leader just now; a disruptive higher-term request
    // changes nothing on it, nor on the leader.
    for id in [3, 1] {
        let before = net.core(id).state.clone();
        net.run(id, Event::Message(vote_request(5, id, 9)));
        assert_eq!(
            net.core(id).state,
            before,
            "node {id}: term and vote unchanged"
        );
        assert!(matches!(
            net.queue.last().map(|m| &m.body),
            Some(Rpc::VoteResponse {
                granted: false,
                term: 2
            })
        ));
    }
    assert_eq!(net.core(1).role, Role::Leader);
    // A stale expiry does nothing; the current one ends the contact.
    let stale = net.core(3).election_generation - 1;
    net.run(3, Event::LeaderContactExpired { generation: stale });
    assert!(net.core(3).leader_contact());
    net.expire_contact(3);
    assert!(!net.core(3).leader_contact());
    net.run(3, Event::Message(vote_request(5, 3, 9)));
    assert_eq!(net.core(3).state.term, 9);
    assert_eq!(net.core(3).state.voted_for, Some(5));
    // The leader never loses its own contact.
    net.expire_contact(1);
    assert!(net.core(1).leader_contact());
}
/// A RequestVote from `from` (with a log as long as anyone's) in `term`.
fn vote_request(from: NodeId, to: NodeId, term: Term) -> Envelope {
    Envelope {
        version: 1,
        cluster_id: CLUSTER.into(),
        from,
        to,
        rpc_id: 77,
        body: Rpc::RequestVote {
            term,
            candidate_id: from,
            last_log_index: 100,
            last_log_term: 2,
        },
    }
}

#[test]
fn only_voters_of_the_configuration_count() {
    // A candidate in {1,2,3} is not elected by nodes 4 and 5.
    let state = DurableState::new(1, None, vec![config(1, 1, &[1, 2, 3])]);
    let mut n = Core::new(1, state).unwrap();
    let fx = n.step(Event::ElectionTimeout { generation: 0 });
    let asked: Vec<NodeId> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Send(m) => Some(m.to),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [2, 3], "only the other voters are asked");
    for effect in fx {
        if let Effect::Persist { barrier_id, .. } = effect {
            n.persisted(barrier_id);
        }
    }
    for outsider in [4, 5] {
        n.vote_requests.insert(outsider, 1);
        n.step(Event::Message(Envelope {
            version: 1,
            cluster_id: CLUSTER.into(),
            from: outsider,
            to: 1,
            rpc_id: 1,
            body: Rpc::VoteResponse {
                term: 2,
                granted: true,
            },
        }));
    }
    assert_eq!(n.role, Role::Candidate, "votes from outside do not count");

    // Figure 4.6: node 1 is not in its latest configuration {2} (not known
    // committed). It campaigns without counting itself and needs node 2.
    let state = DurableState::new(
        2,
        None,
        vec![config(1, 1, &[1, 2]), noop(2, 2), config(3, 2, &[2])],
    );
    let mut net = Net::new(&[
        (1, state.clone()),
        (2, DurableState::new(2, None, state.entries()[..2].to_vec())),
    ]);
    net.timeout(1);
    assert_eq!(
        net.core(1).role,
        Role::Candidate,
        "its own vote is not enough"
    );
    net.deliver(all);
    assert_eq!(
        net.core(1).role,
        Role::Follower,
        "elected, committed {{2}}, stepped down"
    );
    assert_eq!(
        net.core(2).configuration().map(|(_, v)| v),
        Some(voters(&[2]))
    );
    assert!(net.core(1).commit_index >= 3);
    net.timeout(1);
    assert_eq!(net.core(1).role, Role::Follower, "it knows it was removed");
}

#[test]
fn read_index_needs_a_majority_of_the_current_configuration() {
    for size in [4, 5] {
        let mut net = settled(size);
        net.run(1, Event::Read { id: 1 });
        net.deliver_one(1, 2);
        net.deliver_one(2, 1);
        assert!(
            net.reads.is_empty(),
            "size {size}: the leader and one follower are too few"
        );
        net.deliver_one(1, 3);
        net.deliver_one(3, 1);
        assert_eq!(
            net.reads,
            [(1, Ok(2))],
            "size {size}: the leader and two followers"
        );
    }
    // G9: a leader outside its configuration {2,3} does not count itself, so
    // one follower is not a majority even though "leader + one" was one in {1,2,3}.
    let mut net = settled(3);
    net.change(1, 1, Change::Remove(1));
    net.run(1, Event::Read { id: 2 });
    net.deliver_one(1, 2); // the configuration entry, sent before the read
    net.deliver_one(2, 1); // so a request of the read's round follows
    net.deliver_one(1, 2);
    net.deliver_one(2, 1);
    assert!(
        net.reads.is_empty(),
        "one of {{2,3}} confirmed: not a majority"
    );
}

#[test]
fn a_snapshot_records_the_configuration_at_its_position() {
    let mut net = settled(3);
    // Applied through 2 under {1,2,3}; an uncommitted {1,2} follows.
    net.change(1, 2, Change::Remove(3));
    assert_eq!(net.core(1).configuration(), Some((3, voters(&[1, 2]))));
    net.run(
        1,
        Event::Compact {
            index: 2,
            data: b"kv".to_vec(),
        },
    );
    let snapshot = net.core(1).snapshot().unwrap().clone();
    assert_eq!(
        snapshot.voters,
        voters(&[1, 2, 3]).bits(),
        "not the later {{1,2}}"
    );
    // A node that has only the snapshot uses its configuration.
    let state = DurableState::with_base(2, None, snapshot.base, vec![]);
    let restored = Core::restore(2, state, Some(snapshot)).unwrap();
    assert_eq!(restored.configuration(), Some((2, voters(&[1, 2, 3]))));
}

#[test]
fn a_truncated_configuration_falls_back_to_the_previous_one() {
    let mut n = Core::new(
        3,
        DurableState::new(
            2,
            None,
            vec![
                config(1, 1, &[1, 2, 3]),
                noop(2, 2),
                config(3, 2, &[1, 2, 3, 4]),
            ],
        ),
    )
    .unwrap();
    assert_eq!(n.configuration(), Some((3, voters(&[1, 2, 3, 4]))));
    // A new leader of term 3 overwrites index 3.
    let fx = n.step(Event::Message(Envelope {
        version: 1,
        cluster_id: CLUSTER.into(),
        from: 2,
        to: 3,
        rpc_id: 5,
        body: Rpc::AppendEntries {
            term: 3,
            leader_id: 2,
            prev_log_index: 2,
            prev_log_term: 2,
            entries: vec![noop(3, 3)],
            leader_commit: 0,
        },
    }));
    assert!(matches!(fx.first(), Some(Effect::Persist { .. })));
    assert_eq!(n.configuration(), Some((1, voters(&[1, 2, 3]))));
}

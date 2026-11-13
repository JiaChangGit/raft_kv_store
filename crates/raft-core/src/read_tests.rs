//! S3 core behaviour: ReadIndex reads. docs/READ_INDEX.md §6 and §10.
use super::*;

fn env(from: u8, to: u8, rpc_id: u64, body: Rpc) -> Envelope {
    Envelope {
        version: 1,
        cluster_id: CLUSTER.into(),
        from,
        to,
        rpc_id,
        body,
    }
}
/// Run every durability barrier as a host would; return the other effects.
fn complete(n: &mut Core, effects: Vec<Effect>) -> Vec<Effect> {
    let mut out = vec![];
    for effect in effects {
        match effect {
            Effect::Persist { barrier_id, .. } | Effect::PersistSnapshot { barrier_id, .. } => {
                let follow_up = n.persisted(barrier_id);
                out.extend(complete(n, follow_up));
            }
            other => out.push(other),
        }
    }
    out
}
/// The latest AppendEntries sent to `peer` in `effects`.
fn append_to(effects: &[Effect], peer: u8) -> Option<Envelope> {
    effects.iter().rev().find_map(|e| match e {
        Effect::Send(m) if m.to == peer && matches!(m.body, Rpc::AppendEntries { .. }) => {
            Some(m.clone())
        }
        _ => None,
    })
}
/// Answer an AppendEntries the way a healthy follower in the same term would.
fn answer(n: &mut Core, request: &Envelope, success: bool) -> Vec<Effect> {
    let Rpc::AppendEntries {
        term,
        prev_log_index,
        entries,
        ..
    } = &request.body
    else {
        panic!("not an AppendEntries: {request:?}");
    };
    let matched = if success {
        prev_log_index + entries.len() as u64
    } else {
        0
    };
    let fx = n.step(Event::Message(env(
        request.to,
        request.from,
        request.rpc_id,
        Rpc::AppendResponse {
            term: *term,
            success,
            matched_through: matched,
            last_log_index: matched,
        },
    )));
    complete(n, fx)
}
fn ready(effects: &[Effect]) -> Vec<(u64, u64)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ReadReady { id, read_index } => Some((*id, *read_index)),
            _ => None,
        })
        .collect()
}
fn failed(effects: &[Effect]) -> Vec<(u64, &'static str)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ReadFailed { id, status } => Some((*id, *status)),
            _ => None,
        })
        .collect()
}

/// Node 1 just became leader of term 2 in a three-node configuration (index 1,
/// term 1). Its no-op (index 2) is not committed; returns the AppendEntries
/// carrying it to nodes 2 and 3.
fn new_leader() -> (Core, Envelope, Envelope) {
    let three = LogEntry {
        index: 1,
        term: 1,
        command: Command::Config {
            voters: vec![1, 2, 3],
        },
    };
    let mut n = Core::new(1, DurableState::new(1, None, vec![three])).unwrap();
    let fx = n.step(Event::ElectionTimeout { generation: 0 });
    let vote = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.rpc_id),
            _ => None,
        })
        .unwrap();
    complete(&mut n, fx);
    let fx = n.step(Event::Message(env(
        2,
        1,
        vote,
        Rpc::VoteResponse {
            term: 2,
            granted: true,
        },
    )));
    let fx = complete(&mut n, fx);
    assert_eq!(n.role, Role::Leader);
    (
        n,
        append_to(&fx, 2).expect("no-op sent to 2"),
        append_to(&fx, 3).expect("no-op sent to 3"),
    )
}
/// A leader whose no-op is committed; nodes 2 and 3 have nothing in flight.
fn settled_leader() -> Core {
    let (mut n, to2, to3) = new_leader();
    let fx = answer(&mut n, &to2, true);
    assert_eq!(n.commit_index, 2);
    if let Some(again) = append_to(&fx, 2) {
        answer(&mut n, &again, true);
    }
    answer(&mut n, &to3, true);
    n.inflight = std::array::from_fn(|_| None);
    n
}

#[test]
fn follower_rejects_reads() {
    let mut n = Core::new(2, DurableState::default()).unwrap();
    let fx = n.step(Event::Read { id: 9 });
    assert_eq!(failed(&fx), [(9, "NOT_LEADER")]);
}

#[test]
fn read_waits_for_a_committed_entry_of_its_term() {
    let (mut n, to2, _to3) = new_leader();
    let fx = n.step(Event::Read { id: 1 });
    assert!(ready(&fx).is_empty() && failed(&fx).is_empty());
    assert_eq!(
        n.pending_reads[0].read_index, None,
        "step 1: no readIndex yet"
    );
    // Node 2 acknowledges the no-op: it commits, and the read gets readIndex 2
    // and a new round. That same reply was sent before the round began, so it
    // does not confirm it.
    let fx = answer(&mut n, &to2, true);
    assert_eq!(n.commit_index, 2);
    assert_eq!(n.pending_reads[0].read_index, Some(2));
    assert!(
        ready(&fx).is_empty(),
        "a reply from before the round cannot confirm it"
    );
    let round = append_to(&fx, 2).expect("a request of the new round goes to node 2");
    let fx = answer(&mut n, &round, true);
    assert_eq!(ready(&fx), [(1, 2)]);
    assert!(n.pending_reads.is_empty());
}

#[test]
fn replies_to_requests_sent_before_the_read_do_not_confirm_it() {
    let mut n = settled_leader();
    // Requests already in flight to both followers, sent before the read.
    let fx = n.step(Event::Heartbeat {
        generation: n.heartbeat_generation,
    });
    let (old2, old3) = (append_to(&fx, 2).unwrap(), append_to(&fx, 3).unwrap());
    let fx = n.step(Event::Read { id: 5 });
    assert!(
        append_to(&fx, 2).is_none(),
        "one request in flight per follower"
    );
    let fx2 = answer(&mut n, &old2, true);
    let fx3 = answer(&mut n, &old3, true);
    assert!(ready(&fx2).is_empty() && ready(&fx3).is_empty());
    // Each old reply is followed by a request of the read's round.
    let new2 = append_to(&fx2, 2).expect("a new-round request after the old reply");
    let fx = answer(&mut n, &new2, true);
    assert_eq!(
        ready(&fx),
        [(5, 2)],
        "one follower plus the leader is a majority"
    );
}

#[test]
fn one_round_confirms_every_waiting_read() {
    let (mut n, to2, _) = new_leader();
    for id in 1..=3 {
        assert!(n.step(Event::Read { id }).is_empty());
    }
    let fx = answer(&mut n, &to2, true);
    let round = append_to(&fx, 2).unwrap();
    let fx = answer(&mut n, &round, true);
    assert_eq!(
        ready(&fx),
        [(1, 2), (2, 2), (3, 2)],
        "all three share one round"
    );
}

#[test]
fn a_same_term_rejection_still_confirms_leadership() {
    let mut n = settled_leader();
    let fx = n.step(Event::Read { id: 4 });
    let request = append_to(&fx, 2).unwrap();
    // success = false (log mismatch) but in our term: still a vote of confidence.
    let fx = answer(&mut n, &request, false);
    assert_eq!(ready(&fx), [(4, 2)]);
}

#[test]
fn read_waits_until_the_state_machine_reaches_read_index() {
    let mut n = settled_leader();
    let fx = n.step(Event::Read { id: 6 });
    let request = append_to(&fx, 2).unwrap();
    n.last_applied = 1; // as if applying lagged behind the commit index
    let fx = answer(&mut n, &request, true);
    assert!(
        ready(&fx).is_empty(),
        "confirmed but not yet applied through readIndex"
    );
    n.last_applied = 2;
    let fx = n.step(Event::Heartbeat {
        generation: n.heartbeat_generation,
    });
    let again = append_to(&fx, 2).unwrap();
    let fx = answer(&mut n, &again, true);
    assert_eq!(ready(&fx), [(6, 2)]);
}

#[test]
fn reads_fail_when_leadership_is_lost() {
    let mut n = settled_leader();
    let fx = n.step(Event::Read { id: 7 });
    let request = append_to(&fx, 2).unwrap();
    n.step(Event::Read { id: 8 });
    // A higher term appears: node 1 steps down. Since S4 a leader ignores a
    // higher-term RequestVote (docs/MEMBERSHIP.md §9.4), so the higher term
    // arrives the other way it can: from the leader of that term.
    let fx = n.step(Event::Message(env(
        3,
        1,
        77,
        Rpc::AppendEntries {
            term: 3,
            leader_id: 3,
            prev_log_index: 2,
            prev_log_term: 2,
            entries: vec![],
            leader_commit: 2,
        },
    )));
    assert_eq!(n.role, Role::Follower);
    assert_eq!(n.state.term, 3);
    assert_eq!(failed(&fx), [(7, "NOT_LEADER"), (8, "NOT_LEADER")]);
    // A late reply from the old term changes nothing.
    let late = n.step(Event::Message(env(
        2,
        1,
        request.rpc_id,
        Rpc::AppendResponse {
            term: 2,
            success: true,
            matched_through: 2,
            last_log_index: 2,
        },
    )));
    assert!(ready(&late).is_empty());
}

#[test]
fn cancelled_read_is_not_answered() {
    let mut n = settled_leader();
    let fx = n.step(Event::Read { id: 3 });
    let request = append_to(&fx, 2).unwrap();
    assert!(n.step(Event::CancelRead { id: 3 }).is_empty());
    let fx = answer(&mut n, &request, true);
    assert!(ready(&fx).is_empty());
    assert!(n.pending_reads.is_empty());
}

#[test]
fn reads_never_write_durable_state() {
    let mut n = settled_leader();
    let before = n.state.clone();
    for id in 0..5 {
        let fx = n.step(Event::Read { id });
        assert!(
            !fx.iter()
                .any(|e| matches!(e, Effect::Persist { .. } | Effect::PersistSnapshot { .. })),
            "a read must not produce a durability barrier"
        );
    }
    assert_eq!(n.state, before);
}

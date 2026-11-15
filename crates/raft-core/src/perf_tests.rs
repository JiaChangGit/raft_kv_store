//! S5 core behaviour: batched proposals, byte-bounded AppendEntries, and the
//! cost of a step not growing with the log. docs/PERFORMANCE.md §6 and §10.
use super::*;

fn config(index: u64) -> LogEntry {
    LogEntry {
        index,
        term: 1,
        command: Command::Config {
            voters: vec![1, 2, 3],
        },
    }
}
fn put(client: u128, value_len: usize) -> ClientRequest {
    ClientRequest {
        client_id: client,
        sequence: 1,
        operation: Operation::Put {
            key: format!("k{client}"),
            value: "v".repeat(value_len),
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
fn complete(n: &mut Core, effects: Vec<Effect>) -> Vec<Effect> {
    let mut out = vec![];
    for effect in effects {
        match effect {
            Effect::Persist { barrier_id, .. } | Effect::PersistSnapshot { barrier_id, .. } => {
                let more = n.persisted(barrier_id);
                out.extend(complete(n, more));
            }
            other => out.push(other),
        }
    }
    out
}
fn appends(effects: &[Effect]) -> Vec<(NodeId, u64, usize)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send(Envelope {
                to,
                rpc_id,
                body: Rpc::AppendEntries { entries, .. },
                ..
            }) => Some((*to, *rpc_id, entries.len())),
            _ => None,
        })
        .collect()
}
/// Node 1 leads term 2 over `extra` entries after the configuration; the
/// followers acknowledged everything, so nothing is in flight.
fn leader(extra: u64) -> Core {
    let mut entries = vec![config(1)];
    entries.extend((2..2 + extra).map(|i| noop(i, 1)));
    let mut n = Core::new(1, DurableState::new(1, None, entries)).unwrap();
    let fx = n.step(Event::ElectionTimeout { generation: 0 });
    let fx = complete(&mut n, fx);
    let vote = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.rpc_id),
            _ => None,
        })
        .unwrap();
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
    let last = n.state.last_index();
    for (peer, rpc_id, _) in appends(&fx) {
        let fx = n.step(Event::Message(env(
            peer,
            1,
            rpc_id,
            Rpc::AppendResponse {
                term: 2,
                success: true,
                matched_through: last,
                last_log_index: last,
            },
        )));
        complete(&mut n, fx);
    }
    // Followers that were already up to date get the no-op in a second request.
    n.inflight = std::array::from_fn(|_| None);
    n.matches[2] = last;
    n.matches[3] = last;
    n.next[2] = last + 1;
    n.next[3] = last + 1;
    n
}

#[test]
fn a_batch_is_one_barrier_and_one_append_per_follower() {
    let mut n = leader(0);
    let last = n.state.last_index();
    let mut requests: Vec<_> = (1..=5).map(|c| put(c, 8)).collect();
    requests[2].operation = Operation::Put {
        key: "k".repeat(MAX_KEY + 1),
        value: "bad".into(),
    };
    let fx = n.step(Event::ProposeBatch(requests));
    let barriers: Vec<&Delta> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Persist { delta, .. } => Some(delta),
            _ => None,
        })
        .collect();
    assert_eq!(barriers.len(), 1, "one barrier for the whole batch");
    let delta = barriers[0];
    assert_eq!(delta.truncate_from, last + 1);
    let clients: Vec<u128> = delta
        .append
        .iter()
        .map(|e| match &e.command {
            Command::Client(r) => r.client_id,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        clients,
        [1, 2, 4, 5],
        "in order; the invalid one is left out"
    );
    assert!(
        matches!(fx.first(), Some(Effect::Persist { .. })),
        "nothing goes out before it"
    );
    let rejected: Vec<_> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Reject(r, status) => Some((r.client_id, *status)),
            _ => None,
        })
        .collect();
    assert_eq!(
        rejected,
        [(3, "INVALID_ARGUMENT")],
        "only the invalid request"
    );
    let sent = appends(&fx);
    assert_eq!(sent.len(), 2, "one AppendEntries per follower");
    assert!(sent.iter().all(|(_, _, count)| *count == 4), "{sent:?}");

    // Not the leader: every request is turned away, nothing is persisted.
    let mut follower = Core::new(2, DurableState::new(1, None, vec![config(1)])).unwrap();
    let fx = follower.step(Event::ProposeBatch(vec![put(1, 8), put(2, 8)]));
    assert_eq!(fx.len(), 2);
    assert!(
        fx.iter()
            .all(|e| matches!(e, Effect::Reject(_, "NOT_LEADER")))
    );
}

#[test]
fn an_append_carries_at_most_one_batch() {
    // A follower 202 entries behind gets all of them in one request: below
    // both bounds. Until S5 a request carried at most 32.
    let mut n = leader(0);
    let fx = n.step(Event::ProposeBatch((1..=200).map(|c| put(c, 8)).collect()));
    complete(&mut n, fx);
    let mut n2 = n.clone();
    n2.next[2] = 1;
    n2.inflight[2] = None;
    let fx = n2.step(Event::Heartbeat {
        generation: n2.heartbeat_generation,
    });
    let to2: Vec<_> = appends(&fx)
        .into_iter()
        .filter(|(p, _, _)| *p == 2)
        .collect();
    assert_eq!(to2[0].2, 202, "everything the follower lacks: 202 of them");
    let entries = n2.state.entries_from(1).unwrap();
    assert_eq!(batch_len(entries), 202);
    // Large entries: the byte bound decides, and at least one always goes.
    let big: Vec<LogEntry> = (1..=300)
        .map(|i| LogEntry {
            index: i,
            term: 1,
            command: Command::Client(put(i as u128, MAX_VALUE)),
        })
        .collect();
    let count = batch_len(&big);
    let bytes: usize = big[..count].iter().map(LogEntry::encoded_len).sum();
    assert!(
        bytes <= MAX_BATCH_BYTES && count > 100,
        "{count} entries, {bytes} bytes"
    );
    assert!(bytes + big[count].encoded_len() > MAX_BATCH_BYTES);
    let small: Vec<LogEntry> = (1..=300).map(|i| noop(i, 1)).collect();
    assert_eq!(batch_len(&small), MAX_BATCH_ENTRIES);
    assert_eq!(batch_len(&big[..1]), 1);
}

/// G4: what a write costs the core does not depend on how long the log is.
#[test]
fn the_cost_of_a_write_does_not_grow_with_the_log() {
    let cost = |extra: u64| {
        let mut n = leader(extra);
        let before = n.entries_copied;
        let fx = n.step(Event::ProposeBatch(vec![put(1, 8)]));
        complete(&mut n, fx);
        n.entries_copied - before
    };
    assert_eq!(cost(10), cost(10_000));
    assert_eq!(cost(10), 1, "only the new entry is copied into the barrier");
}

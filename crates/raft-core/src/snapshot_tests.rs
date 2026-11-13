//! S2 core behaviour: compaction, InstallSnapshot on both sides, and
//! AppendEntries that start before a follower's snapshot.
//! docs/SNAPSHOT_COMPACTION.md §8-§9.
use super::*;

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
fn base(index: u64, term: u64) -> LogBase {
    LogBase { index, term }
}
fn three() -> Voters {
    Voters::from_ids(&[1, 2, 3]).unwrap()
}
fn sent(effects: &[Effect]) -> Vec<&Rpc> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send(m) => Some(&m.body),
            _ => None,
        })
        .collect()
}
fn last_send(effects: &[Effect]) -> Envelope {
    effects
        .iter()
        .rev()
        .find_map(|e| match e {
            Effect::Send(m) => Some(m.clone()),
            _ => None,
        })
        .expect("a message was sent")
}
/// Run every durability barrier as a host would, and return the rest in order.
fn complete(n: &mut Core, effects: Vec<Effect>) -> Vec<Effect> {
    let mut out = vec![];
    for effect in effects {
        match effect {
            Effect::Persist { barrier_id, .. } | Effect::PersistSnapshot { barrier_id, .. } => {
                out.extend(n.persisted(barrier_id))
            }
            other => out.push(other),
        }
    }
    out
}

/// Node 1 as leader of term 4 whose log was compacted through index 5.
fn compacted_leader(data: &[u8]) -> Core {
    let state = DurableState::with_base(3, Some(1), base(5, 2), vec![noop(6, 3), noop(7, 3)]);
    let snapshot = Snapshot::new(base(5, 2), three(), data.to_vec());
    let mut n = Core::restore(1, state, Some(snapshot)).unwrap();
    let fx = n.step(Event::ElectionTimeout { generation: 0 });
    let request = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.clone()),
            _ => None,
        })
        .expect("a vote request to node 2");
    complete(&mut n, fx);
    let fx = n.step(Event::Message(env(
        2,
        1,
        request.rpc_id,
        Rpc::VoteResponse {
            term: 4,
            granted: true,
        },
    )));
    complete(&mut n, fx);
    assert_eq!(n.role, Role::Leader);
    n
}

fn chunk_of(rpc: &Rpc) -> (u64, Vec<u8>, bool) {
    match rpc {
        Rpc::InstallSnapshot {
            offset, data, done, ..
        } => (*offset, from_hex(data).unwrap(), *done),
        other => panic!("expected InstallSnapshot, got {other:?}"),
    }
}

fn snapshot_reply(rpc_id: u64, success: bool, next_offset: u64, done: bool) -> Event {
    Event::Message(env(
        2,
        1,
        rpc_id,
        Rpc::InstallSnapshotResponse {
            term: 4,
            success,
            last_included_index: 5,
            next_offset,
            done,
        },
    ))
}

#[test]
fn restore_requires_the_snapshot_matching_the_base() {
    let state = DurableState::with_base(3, None, base(5, 2), vec![]);
    assert!(
        Core::restore(1, state.clone(), None).is_err(),
        "base without snapshot"
    );
    let other = Snapshot::new(base(4, 2), three(), b"x".to_vec());
    assert!(Core::restore(1, state.clone(), Some(other)).is_err());
    let core = Core::restore(
        1,
        state,
        Some(Snapshot::new(base(5, 2), three(), b"x".to_vec())),
    )
    .unwrap();
    assert_eq!((core.commit_index, core.last_applied), (5, 5));
}

#[test]
fn compaction_only_covers_applied_entries() {
    // Index 1 is the configuration (S4: a snapshot records the configuration
    // at its position, so a log without one cannot be compacted).
    let config = LogEntry {
        index: 1,
        term: 2,
        command: Command::Config {
            voters: vec![1, 2, 3],
        },
    };
    let entries = std::iter::once(config).chain((2..=4).map(|i| noop(i, 2)));
    let mut n = Core::new(1, DurableState::new(2, Some(1), entries.collect())).unwrap();
    n.commit_index = 3;
    n.last_applied = 3;
    for (what, index, data) in [
        ("beyond applied", 4, b"s".to_vec()),
        ("behind applied", 2, b"s".to_vec()),
        ("empty snapshot", 3, vec![]),
        ("not in the log", 9, b"s".to_vec()),
    ] {
        assert!(n.step(Event::Compact { index, data }).is_empty(), "{what}");
        assert_eq!(n.state.base(), base(0, 0), "{what}");
    }
    let fx = n.step(Event::Compact {
        index: 3,
        data: b"state-3".to_vec(),
    });
    let Some(Effect::PersistSnapshot {
        barrier_id,
        state,
        snapshot,
        restore,
    }) = fx.first()
    else {
        panic!("compaction is a durability barrier: {fx:?}");
    };
    assert_eq!(fx.len(), 1);
    assert!(!restore);
    assert_eq!(state.base(), base(3, 2));
    assert_eq!(state.entries(), [noop(4, 2)]);
    assert_eq!(&*snapshot.data, b"state-3");
    assert_eq!(
        snapshot.voters,
        three().bits(),
        "the configuration at index 3"
    );
    assert!(n.snapshot().is_none(), "not usable before it is durable");
    assert!(
        n.step(Event::Heartbeat { generation: 0 }).is_empty(),
        "nothing happens while the barrier is pending"
    );
    n.persisted(*barrier_id);
    assert_eq!(n.snapshot().map(|s| s.base), Some(base(3, 2)));
    assert!(
        n.step(Event::Compact {
            index: 3,
            data: b"again".to_vec()
        })
        .is_empty(),
        "already compacted"
    );
}

#[test]
fn leader_sends_snapshot_chunks_in_order_and_resumes() {
    let data: Vec<u8> = (0..20).collect();
    let mut n = compacted_leader(&data);
    n.set_chunk_size(8);
    // Follower 2 is far behind: it needs entry 3, which the leader compacted.
    n.next[2] = 3;
    n.inflight[2] = None;
    let fx = n.step(Event::Heartbeat {
        generation: n.heartbeat_generation,
    });
    let first = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(chunk_of(&first.body), (0, data[0..8].to_vec(), false));
    if let Rpc::InstallSnapshot {
        total_len,
        total_crc,
        last_included_index,
        last_included_term,
        ..
    } = &first.body
    {
        assert_eq!((*total_len, *total_crc), (20, crc32c(&data)));
        assert_eq!((*last_included_index, *last_included_term), (5, 2));
    }
    // A reply to some other request is ignored.
    assert!(
        n.step(snapshot_reply(first.rpc_id + 99, true, 8, false))
            .is_empty()
    );
    let fx = n.step(snapshot_reply(first.rpc_id, true, 8, false));
    let second = last_send(&fx);
    assert_eq!(chunk_of(&second.body), (8, data[8..16].to_vec(), false));
    // The follower says it only has 4 bytes: resend from there.
    let fx = n.step(snapshot_reply(second.rpc_id, false, 4, false));
    let resent = last_send(&fx);
    assert_eq!(chunk_of(&resent.body), (4, data[4..12].to_vec(), false));
    // A timeout resends the same chunk.
    let fx = n.step(Event::RpcTimeout {
        peer: 2,
        rpc_id: resent.rpc_id,
    });
    let retried = last_send(&fx);
    assert_eq!(chunk_of(&retried.body), (4, data[4..12].to_vec(), false));
    let fx = n.step(snapshot_reply(retried.rpc_id, true, 12, false));
    let last = last_send(&fx);
    assert_eq!(chunk_of(&last.body), (12, data[12..20].to_vec(), true));
    let fx = n.step(snapshot_reply(last.rpc_id, true, 20, true));
    assert_eq!((n.matched(2), n.next[2]), (5, 6));
    // Back to ordinary replication right after the snapshot.
    let append = last_send(&fx);
    assert!(matches!(
        append.body,
        Rpc::AppendEntries {
            prev_log_index: 5,
            prev_log_term: 2,
            ..
        }
    ));
}

#[test]
fn a_newer_snapshot_restarts_the_transfer() {
    let data: Vec<u8> = (0..20).collect();
    let mut n = compacted_leader(&data);
    n.set_chunk_size(8);
    n.next[2] = 3;
    n.inflight[2] = None;
    let fx = n.step(Event::Heartbeat {
        generation: n.heartbeat_generation,
    });
    let first = last_send(&fx);
    // Leader commits and applies through 7, then compacts again.
    n.commit_index = 7;
    n.last_applied = 7;
    let fx = n.step(Event::Compact {
        index: 7,
        data: b"newer-state".to_vec(),
    });
    complete(&mut n, fx);
    let fx = n.step(snapshot_reply(first.rpc_id, true, 8, false));
    let next = last_send(&fx);
    assert_eq!(chunk_of(&next.body), (0, b"newer-st".to_vec(), false));
    assert!(matches!(
        next.body,
        Rpc::InstallSnapshot {
            last_included_index: 7,
            ..
        }
    ));
}

/// Follower 2 with entries 1..=6, committed through 2.
fn follower(terms: [u64; 6]) -> Core {
    let entries = terms
        .iter()
        .enumerate()
        .map(|(i, t)| noop(i as u64 + 1, *t))
        .collect();
    let mut n = Core::new(2, DurableState::new(2, None, entries)).unwrap();
    n.commit_index = 2;
    n.last_applied = 2;
    n
}
fn install(rpc_id: u64, term: u64, at: LogBase, offset: u64, data: &[u8], whole: &[u8]) -> Event {
    Event::Message(env(
        1,
        2,
        rpc_id,
        Rpc::InstallSnapshot {
            term,
            leader_id: 1,
            last_included_index: at.index,
            last_included_term: at.term,
            voters: three().bits(),
            total_len: whole.len() as u64,
            total_crc: crc32c(whole),
            offset,
            data: to_hex(data),
            done: offset + data.len() as u64 == whole.len() as u64,
        },
    ))
}
fn response(effects: &[Effect]) -> (bool, u64, bool) {
    match sent(effects).last() {
        Some(Rpc::InstallSnapshotResponse {
            success,
            next_offset,
            done,
            ..
        }) => (*success, *next_offset, *done),
        other => panic!("expected an InstallSnapshot reply, got {other:?}"),
    }
}

#[test]
fn follower_installs_only_after_the_snapshot_is_durable() {
    let whole = b"snap-4";
    let mut n = follower([1, 1, 2, 2, 2, 2]);
    let fx = n.step(install(1, 2, base(4, 2), 0, &whole[..3], whole));
    assert_eq!(response(&fx), (true, 3, false));
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Persist { .. } | Effect::PersistSnapshot { .. })),
        "a partial snapshot is not stored durably and nothing depends on it"
    );
    let fx = n.step(install(2, 2, base(4, 2), 3, &whole[3..], whole));
    let Some(Effect::PersistSnapshot {
        barrier_id,
        state,
        snapshot,
        restore,
    }) = fx.first()
    else {
        panic!("the install must be a barrier before the reply: {fx:?}");
    };
    assert!(*restore);
    assert_eq!(&*snapshot.data, whole);
    // Figure 13 step 6: the log agrees at index 4, so 5 and 6 are kept.
    assert_eq!(state.base(), base(4, 2));
    assert_eq!(state.entries(), [noop(5, 2), noop(6, 2)]);
    assert_eq!(
        response(&fx[1..]),
        (true, 6, true),
        "the reply follows the barrier"
    );
    assert_eq!((n.commit_index, n.last_applied), (4, 4));
    assert!(n.snapshot().is_none());
    n.persisted(*barrier_id);
    assert_eq!(n.snapshot().map(|s| s.base), Some(base(4, 2)));
}

#[test]
fn a_conflicting_log_is_discarded_on_install() {
    let whole = b"snap-4";
    // Entry 4 has term 1 here, the snapshot says term 2.
    let mut n = follower([1, 1, 1, 1, 1, 1]);
    let fx = n.step(install(1, 2, base(4, 2), 0, whole, whole));
    let Some(Effect::PersistSnapshot { state, .. }) = fx.first() else {
        panic!("expected an install");
    };
    assert_eq!(state.base(), base(4, 2));
    assert!(state.entries().is_empty(), "Figure 13 step 7");
    assert_eq!(state.last_index(), 4);
}

#[test]
fn out_of_order_foreign_and_corrupt_chunks_are_refused() {
    let whole = b"abcdefgh";
    let mut n = follower([1, 1, 2, 2, 2, 2]);
    let before = n.state.clone();
    // A chunk that does not continue where we are.
    let fx = n.step(install(1, 2, base(4, 2), 4, &whole[4..6], whole));
    assert_eq!(response(&fx), (false, 0, false), "nothing started yet");
    n.step(install(2, 2, base(4, 2), 0, &whole[..4], whole));
    let fx = n.step(install(3, 2, base(4, 2), 6, &whole[6..], whole));
    assert_eq!(response(&fx), (false, 4, false), "resume from what we hold");
    // A chunk of another snapshot in the middle of this one: start over.
    let fx = n.step(install(4, 2, base(5, 2), 4, &whole[4..6], whole));
    assert_eq!(response(&fx), (false, 0, false));
    // Every byte arrives but the content does not match the declared CRC.
    n.step(install(5, 2, base(4, 2), 0, &whole[..4], whole));
    // `install` declares the CRC of `whole`, which these bytes do not match.
    let fx = n.step(install(6, 2, base(4, 2), 4, b"XXXX", whole));
    assert_eq!(response(&fx), (false, 0, false));
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::PersistSnapshot { .. })),
        "a corrupt snapshot is never stored"
    );
    assert_eq!(n.state, before);
    assert_eq!((n.commit_index, n.last_applied), (2, 2));
}

#[test]
fn a_snapshot_we_already_cover_is_acknowledged_without_change() {
    let mut n = follower([1, 1, 2, 2, 2, 2]);
    n.commit_index = 6;
    n.last_applied = 6;
    let before = n.state.clone();
    let fx = n.step(install(1, 2, base(4, 2), 0, b"old", b"old"));
    assert_eq!(response(&fx), (true, 3, true));
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::PersistSnapshot { .. }))
    );
    assert_eq!(n.state, before);
    assert_eq!(
        (n.commit_index, n.last_applied),
        (6, 6),
        "state never moves backwards"
    );
}

#[test]
fn a_higher_term_install_persists_term_and_snapshot_together() {
    let mut n = follower([1, 1, 2, 2, 2, 2]);
    let fx = n.step(install(1, 5, base(4, 2), 0, b"s", b"s"));
    assert_eq!(
        fx.iter()
            .filter(|e| matches!(e, Effect::Persist { .. } | Effect::PersistSnapshot { .. }))
            .count(),
        1,
        "one barrier"
    );
    let Some(Effect::PersistSnapshot { state, .. }) = fx.first() else {
        panic!("expected an install");
    };
    assert_eq!((state.term, state.voted_for), (5, None));
    // A stale leader is refused and nothing is installed.
    let mut n = follower([1, 1, 2, 2, 2, 2]);
    n.state.term = 7;
    let fx = n.step(install(1, 5, base(4, 2), 0, b"s", b"s"));
    assert_eq!(response(&fx), (false, 0, false));
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::PersistSnapshot { .. }))
    );
}

#[test]
fn append_entries_that_start_before_the_base_count_as_matching() {
    let snapshot = Snapshot::new(base(5, 2), three(), b"s".to_vec());
    let state = DurableState::with_base(3, None, base(5, 2), vec![noop(6, 2)]);
    let append = |prev_log_index, prev_log_term, entries: Vec<LogEntry>| {
        Event::Message(env(
            1,
            2,
            9,
            Rpc::AppendEntries {
                term: 3,
                leader_id: 1,
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit: 0,
            },
        ))
    };
    let matched = |fx: &[Effect]| match sent(fx).last() {
        Some(Rpc::AppendResponse {
            success,
            matched_through,
            ..
        }) => (*success, *matched_through),
        other => panic!("expected AppendResponse, got {other:?}"),
    };
    let restored = || Core::restore(2, state.clone(), Some(snapshot.clone())).unwrap();

    let mut n = restored();
    let fx = n.step(append(
        3,
        1,
        vec![noop(4, 2), noop(5, 2), noop(6, 2), noop(7, 3)],
    ));
    assert_eq!(matched(&fx), (true, 7));
    assert_eq!(n.state.entries(), [noop(6, 2), noop(7, 3)]);

    let mut n = restored();
    let fx = n.step(append(1, 1, vec![noop(2, 1), noop(3, 2)]));
    assert_eq!(matched(&fx), (true, 3), "entirely inside the snapshot");
    assert_eq!(n.state.entries(), [noop(6, 2)]);

    let mut n = restored();
    let fx = n.step(append(4, 2, vec![noop(5, 3), noop(6, 3)]));
    assert_eq!(
        matched(&fx),
        (false, 0),
        "disagrees with the snapshot at its base"
    );
    assert_eq!(n.state.entries(), [noop(6, 2)]);
}

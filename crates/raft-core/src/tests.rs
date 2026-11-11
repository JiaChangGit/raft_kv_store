use super::*;

fn entry(index: u64, term: u64) -> LogEntry {
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
    let mut result = vec![];
    for effect in effects {
        if let Effect::Persist { barrier_id, .. } = effect {
            result.extend(n.persisted(barrier_id));
        } else {
            result.push(effect);
        }
    }
    result
}
fn elected() -> (Core, Vec<Effect>) {
    let mut n = Core::new(1, DurableState::default()).unwrap();
    let fx = n.step(Event::ElectionTimeout { generation: 0 });
    let fx = complete(&mut n, fx);
    let request = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    let fx = n.step(Event::Message(env(
        2,
        1,
        request.rpc_id,
        Rpc::VoteResponse {
            term: 1,
            granted: true,
        },
    )));
    let fx = complete(&mut n, fx);
    assert_eq!(n.role, Role::Leader);
    (n, fx)
}
#[test]
fn previous_term_entry_requires_current_term_commit() {
    let state = DurableState {
        term: 2,
        voted_for: Some(1),
        log: vec![entry(1, 1), entry(2, 2)],
    };
    let mut n = Core::new(1, state).unwrap();
    n.role = Role::Leader;
    n.matches[1] = 2;
    n.matches[2] = 1;
    let mut fx = vec![];
    n.advance_commit(&mut fx);
    assert_eq!(n.commit_index, 0, "old-term majority is insufficient");
    assert!(fx.is_empty());
    n.matches[2] = 2;
    n.advance_commit(&mut fx);
    assert_eq!(n.commit_index, 2);
    assert_eq!(n.last_applied, 2);
    assert_eq!(
        fx.iter()
            .filter_map(|e| if let Effect::Apply(e) = e {
                Some(e.index)
            } else {
                None
            })
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let mut before_sync = Core::new(
        1,
        DurableState {
            term: 1,
            voted_for: Some(1),
            log: vec![],
        },
    )
    .unwrap();
    before_sync.role = Role::Leader;
    before_sync.state.log.push(entry(1, 1));
    before_sync.matches[2] = 1;
    before_sync.advance_commit(&mut vec![]);
    assert_eq!(
        before_sync.commit_index, 0,
        "RAM-only self match does not count"
    );
}
#[test]
fn conflicting_suffix_repaired() {
    let mut n = Core::new(
        2,
        DurableState {
            term: 3,
            voted_for: None,
            log: vec![entry(1, 1), entry(2, 2), entry(3, 2)],
        },
    )
    .unwrap();
    n.commit_index = 1;
    n.last_applied = 1;
    let fx = n.step(Event::Message(env(
        1,
        2,
        20,
        Rpc::AppendEntries {
            term: 3,
            leader_id: 1,
            prev_log_index: 1,
            prev_log_term: 1,
            entries: vec![entry(2, 3), entry(3, 3)],
            leader_commit: 3,
        },
    )));
    assert!(matches!(fx.first(), Some(Effect::Persist { .. })));
    let fx = complete(&mut n, fx);
    assert_eq!(n.state.log, vec![entry(1, 1), entry(2, 3), entry(3, 3)]);
    assert_eq!(n.commit_index, 3);
    assert_eq!(n.last_applied, 3);
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Send(Envelope {
            body: Rpc::AppendResponse {
                success: true,
                matched_through: 3,
                ..
            },
            ..
        })
    )));
    let original = n.state.log.clone();
    let fx = n.step(Event::Message(env(
        1,
        2,
        21,
        Rpc::AppendEntries {
            term: 3,
            leader_id: 1,
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![entry(1, 3)],
            leader_commit: 3,
        },
    )));
    assert_eq!(n.state.log, original, "committed prefix cannot be deleted");
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Send(Envelope {
            body: Rpc::AppendResponse { success: false, .. },
            ..
        })
    )));
    let mut n = Core::new(
        2,
        DurableState {
            term: 3,
            voted_for: None,
            log: vec![entry(1, 1), entry(2, 2), entry(3, 2)],
        },
    )
    .unwrap();
    let fx = n.step(Event::Message(env(
        1,
        2,
        22,
        Rpc::AppendEntries {
            term: 3,
            leader_id: 1,
            prev_log_index: 1,
            prev_log_term: 1,
            entries: vec![],
            leader_commit: 3,
        },
    )));
    assert_eq!(
        n.commit_index, 1,
        "heartbeat only confirms prev index, not unmatched suffix"
    );
    assert_eq!(
        fx.iter().filter(|e| matches!(e, Effect::Apply(_))).count(),
        1
    );
}
#[test]
fn append_response_cannot_inflate_match_index() {
    let (mut n, fx) = elected();
    let m = fx
        .iter()
        .find_map(|e| {
            if let Effect::Send(m) = e {
                if m.to == 2 { Some(m.clone()) } else { None }
            } else {
                None
            }
        })
        .unwrap();
    n.step(Event::Message(env(
        2,
        1,
        m.rpc_id,
        Rpc::AppendResponse {
            term: 1,
            success: true,
            matched_through: 999,
        },
    )));
    assert_eq!(n.matched(2), 0);
    assert_eq!(n.commit_index, 0);
    let fx = n.step(Event::Heartbeat {
        generation: n.heartbeat_generation,
    });
    let current = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    n.step(Event::Message(env(
        2,
        1,
        m.rpc_id,
        Rpc::AppendResponse {
            term: 1,
            success: true,
            matched_through: 1,
        },
    )));
    assert_eq!(n.matched(2), 0, "retired correlation has no effect");
    let fx = n.step(Event::Message(env(
        2,
        1,
        current.rpc_id,
        Rpc::AppendResponse {
            term: 1,
            success: true,
            matched_through: 1,
        },
    )));
    assert_eq!(n.matched(2), 1);
    assert_eq!(n.commit_index, 1);
    let heartbeat = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    assert!(matches!(&heartbeat.body,Rpc::AppendEntries{entries,..} if entries.is_empty()));
    n.step(Event::Message(env(
        2,
        1,
        heartbeat.rpc_id,
        Rpc::AppendResponse {
            term: 1,
            success: true,
            matched_through: 1000,
        },
    )));
    assert_eq!(n.matched(2), 1);
    let fx = n.step(Event::Message(env(
        2,
        1,
        0,
        Rpc::AppendResponse {
            term: 5,
            success: true,
            matched_through: 1000,
        },
    )));
    assert_eq!(n.role, Role::Follower);
    assert_eq!(n.state.term, 5);
    assert!(matches!(fx.first(), Some(Effect::Persist { .. })));
}
#[test]
fn replicated_get_linearizable() {
    let (mut n, initial) = elected();
    let req = ClientRequest {
        client_id: 1,
        sequence: 1,
        operation: Operation::Get { key: "x".into() },
    };
    let fx = n.step(Event::Propose(req.clone()));
    let fx = complete(&mut n, fx);
    assert_eq!(n.state.log.last().unwrap().command, Command::Client(req));
    assert!(
        fx.iter().all(|e| !matches!(e, Effect::Apply(_))),
        "isolated leader cannot apply new GET"
    );
    assert_eq!(n.commit_index, 0);
    assert_eq!(n.last_applied, 0);
    let pending = initial
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.rpc_id),
            _ => None,
        })
        .unwrap();
    let fx = n.step(Event::RpcTimeout {
        peer: 2,
        rpc_id: pending,
    });
    let request = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    let end = match request.body {
        Rpc::AppendEntries {
            prev_log_index,
            entries,
            ..
        } => prev_log_index + entries.len() as u64,
        _ => unreachable!(),
    };
    let fx = n.step(Event::Message(env(
        2,
        1,
        request.rpc_id,
        Rpc::AppendResponse {
            term: n.state.term,
            success: true,
            matched_through: end,
        },
    )));
    assert_eq!(n.commit_index, 2);
    assert_eq!(n.last_applied, 2);
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Apply(LogEntry {
            command: Command::Client(ClientRequest {
                operation: Operation::Get { .. },
                ..
            }),
            ..
        })
    )));
}

#[test]
fn heartbeat_does_not_retire_slow_valid_append() {
    let (mut n, fx) = elected();
    let request = fx
        .iter()
        .find_map(|e| match e {
            Effect::Send(m) if m.to == 2 => Some(m.clone()),
            _ => None,
        })
        .unwrap();
    let fx = n.step(Event::Heartbeat {
        generation: n.heartbeat_generation,
    });
    assert!(
        !fx.iter()
            .any(|e| matches!(e, Effect::Send(Envelope { to: 2, .. }))),
        "100ms heartbeat must not replace an outstanding 150ms response"
    );
    let fx = n.step(Event::Message(env(
        2,
        1,
        request.rpc_id,
        Rpc::AppendResponse {
            term: 1,
            success: true,
            matched_through: 1,
        },
    )));
    assert_eq!(n.commit_index, 1);
    assert!(fx.iter().any(|e| matches!(e, Effect::Apply(_))));
    let before = n.matched(2);
    n.step(Event::RpcTimeout {
        peer: 2,
        rpc_id: request.rpc_id,
    });
    assert_eq!(
        n.matched(2),
        before,
        "late timeout cannot cancel the replacement correlation"
    );
}

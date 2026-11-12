use raft_core::*;
use raft_protocol::*;
use std::collections::{BTreeMap, VecDeque};

fn envelope(from: u8, to: u8, rpc_id: u64, body: Rpc) -> Envelope {
    Envelope {
        version: 1,
        cluster_id: CLUSTER.into(),
        from,
        to,
        rpc_id,
        body,
    }
}
fn execute(core: &mut Core, effects: Vec<Effect>) -> Vec<Envelope> {
    let mut output = vec![];
    for e in effects {
        match e {
            Effect::Persist { barrier_id, .. } => {
                assert!(core.persisted(barrier_id).is_empty());
            }
            Effect::Send(m) => output.push(m),
            _ => {}
        }
    }
    output
}
#[test]
fn single_leader_per_term() {
    for seed in 0..100u64 {
        let mut random = seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut choose = |length: usize| {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random as usize % length
        };
        let mut nodes: Vec<_> = NODES
            .into_iter()
            .map(|id| Core::new(id, DurableState::default()).unwrap())
            .collect();
        let mut messages = VecDeque::new();
        for offset in 0..3 {
            let i = (seed as usize + offset) % 3;
            let generation = nodes[i].election_generation;
            let fx = nodes[i].step(Event::ElectionTimeout { generation });
            messages.extend(execute(&mut nodes[i], fx));
        }
        while !messages.is_empty() {
            let m = messages.remove(choose(messages.len())).unwrap();
            let i = (m.to - 1) as usize;
            let fx = nodes[i].step(Event::Message(m));
            messages.extend(execute(&mut nodes[i], fx));
        }
        assert!(nodes.iter().all(|n| n.role != Role::Leader));
        let i = choose(3);
        let generation = nodes[i].election_generation;
        let fx = nodes[i].step(Event::ElectionTimeout { generation });
        messages.extend(execute(&mut nodes[i], fx));
        let mut winners = BTreeMap::new();
        for _ in 0..100 {
            if messages.is_empty() {
                break;
            }
            let m = messages.remove(choose(messages.len())).unwrap();
            let i = (m.to - 1) as usize;
            let fx = nodes[i].step(Event::Message(m));
            messages.extend(execute(&mut nodes[i], fx));
            for n in &nodes {
                if n.role == Role::Leader {
                    assert_eq!(*winners.entry(n.state.term).or_insert(n.id), n.id);
                }
            }
        }
        assert_eq!(winners.len(), 1);
    }
}
#[test]
fn stale_candidate_rejected() {
    let state = DurableState::new(
        4,
        None,
        vec![LogEntry {
            index: 1,
            term: 3,
            command: Command::NoOp,
        }],
    );
    let mut n = Core::new(1, state).unwrap();
    let fx = n.step(Event::Message(envelope(
        2,
        1,
        9,
        Rpc::RequestVote {
            term: 4,
            candidate_id: 2,
            last_log_index: 200,
            last_log_term: 2,
        },
    )));
    assert!(
        execute(&mut n, fx)
            .iter()
            .any(|m| matches!(m.body, Rpc::VoteResponse { granted: false, .. }))
    );
    assert_eq!(n.state.voted_for, None);
    let fx = n.step(Event::Message(envelope(
        2,
        1,
        10,
        Rpc::RequestVote {
            term: 4,
            candidate_id: 2,
            last_log_index: 1,
            last_log_term: 3,
        },
    )));
    assert!(matches!(fx.first(), Some(Effect::Persist { .. })));
    execute(&mut n, fx);
    let fx = n.step(Event::Message(envelope(
        3,
        1,
        11,
        Rpc::RequestVote {
            term: 4,
            candidate_id: 3,
            last_log_index: 1,
            last_log_term: 3,
        },
    )));
    assert!(
        execute(&mut n, fx)
            .iter()
            .any(|m| matches!(m.body, Rpc::VoteResponse { granted: false, .. }))
    );
    assert_eq!(n.state.voted_for, Some(2));
}
#[test]
fn vote_persist_intent_before_reply() {
    let mut n = Core::new(1, DurableState::default()).unwrap();
    let fx = n.step(Event::ElectionTimeout { generation: 0 });
    let barrier = match fx.first().unwrap() {
        Effect::Persist { barrier_id, state } => {
            assert_eq!(state.term, 1);
            assert_eq!(state.voted_for, Some(1));
            *barrier_id
        }
        _ => panic!("vote messages lack leading durability barrier"),
    };
    assert_eq!(
        fx.iter().filter(|e| matches!(e, Effect::Send(_))).count(),
        2
    );
    assert!(
        n.step(Event::ElectionTimeout {
            generation: n.election_generation
        })
        .is_empty()
    );
    assert_eq!(n.state.term, 1);
    assert!(n.persisted(barrier + 1).is_empty());
    assert!(
        n.step(Event::ElectionTimeout {
            generation: n.election_generation
        })
        .is_empty()
    );
    n.storage_failed();
    assert!(n.persisted(barrier).is_empty());
    assert!(n.failed());
    let mut follower = Core::new(2, DurableState::default()).unwrap();
    let fx = follower.step(Event::Message(envelope(
        1,
        2,
        1,
        Rpc::RequestVote {
            term: 1,
            candidate_id: 1,
            last_log_index: 0,
            last_log_term: 0,
        },
    )));
    assert!(matches!(fx.first(), Some(Effect::Persist { .. })));
    assert!(matches!(
        fx.last(),
        Some(Effect::Send(Envelope {
            body: Rpc::VoteResponse { granted: true, .. },
            ..
        }))
    ));
}

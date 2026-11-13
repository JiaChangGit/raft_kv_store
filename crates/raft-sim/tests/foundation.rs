use raft_protocol::*;
use raft_sim::*;

#[test]
fn simulation_seed_is_replayable() {
    fn run(seed: u64) -> Vec<((u64, u64), u64)> {
        let mut q = Scheduler::default();
        let mut rng = Random::new(seed);
        for time in [4, 0, 4, 2, 4] {
            q.schedule(time, rng.next_u64());
        }
        let mut out = vec![];
        while let Some(e) = q.pop() {
            out.push(e);
        }
        out
    }
    let a = run(42);
    assert_eq!(a, run(42));
    assert_eq!(
        a.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        vec![(0, 2), (2, 4), (4, 1), (4, 3), (4, 5)]
    );
    assert_ne!(a, run(43));
    assert_eq!(a.len(), 5);
}
/// Replaces the V1 `quorum_is_two_of_three` together with raft-protocol's
/// `majority_follows_the_configuration` (S4, docs/MEMBERSHIP.md §13): node IDs
/// are 1..=7, and a core refuses any other identity.
#[test]
fn node_ids_are_one_to_seven() {
    for id in 1..=MAX_NODE {
        assert!(valid_node(id));
        assert!(raft_core::Core::new(id, DurableState::default()).is_ok());
    }
    for id in [0, MAX_NODE + 1, u8::MAX] {
        assert!(!valid_node(id));
        assert!(raft_core::Core::new(id, DurableState::default()).is_err());
    }
    assert_eq!(raft_sim::SLOTS, 5, "the simulator uses five of them");
}
#[test]
fn mock_disk_distinguishes_durable_and_volatile() {
    let mut d = MockDisk::default();
    d.write(DurableState::new(2, None, vec![]));
    d.crash(false);
    assert_eq!(d.durable.term, 0);
    d.write(DurableState::new(2, None, vec![]));
    d.crash(true);
    assert_eq!(d.durable.term, 2);
    d.fail_sync = true;
    d.write(DurableState::new(3, None, vec![]));
    assert!(d.sync().is_err());
    assert_eq!(d.durable.term, 2);
}

#[test]
fn scheduler_fault_hooks_preserve_explicit_order() {
    let mut q = Scheduler::default();
    q.schedule(1, "drop");
    q.schedule(2, "delay");
    q.schedule(3, "keep");
    assert!(q.drop_next());
    assert!(q.delay_next(5));
    assert!(q.duplicate_next());
    assert_eq!(q.pop().unwrap().1, "keep");
    assert_eq!(q.pop().unwrap().1, "keep");
    let (key, value) = q.pop().unwrap();
    assert_eq!((key.0, value), (7, "delay"));
    assert!(!q.drop_next());
    assert!(!q.delay_next(1));
    assert!(!q.duplicate_next());
}

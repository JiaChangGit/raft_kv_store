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
#[test]
fn quorum_is_two_of_three() {
    assert!(!has_quorum(&[]));
    for id in NODES {
        assert!(!has_quorum(&[id]));
        assert!(!has_quorum(&[id, id]));
    }
    for pair in [[1, 2], [1, 3], [2, 3]] {
        assert!(has_quorum(&pair));
    }
    assert!(has_quorum(&NODES));
    assert!(!has_quorum(&[1, 4]));
    assert!(CoreIdCheck::invalid());
}
struct CoreIdCheck;
impl CoreIdCheck {
    fn invalid() -> bool {
        raft_core::Core::new(0, DurableState::default()).is_err()
            && raft_core::Core::new(4, DurableState::default()).is_err()
    }
}
#[test]
fn mock_disk_distinguishes_durable_and_volatile() {
    let mut d = MockDisk::default();
    d.write(DurableState {
        term: 2,
        ..Default::default()
    });
    d.crash(false);
    assert_eq!(d.durable.term, 0);
    d.write(DurableState {
        term: 2,
        ..Default::default()
    });
    d.crash(true);
    assert_eq!(d.durable.term, 2);
    d.fail_sync = true;
    d.write(DurableState {
        term: 3,
        ..Default::default()
    });
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

use raft_sim::*;

#[test]
fn simulation_randomized_invariants() {
    let seeds = std::env::var("RAFT_SIM_SEEDS")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(1000);
    let events = std::env::var("RAFT_SIM_EVENTS")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(10000);
    let report = run_campaign(seeds, events);
    assert!(
        report.failures.is_empty(),
        "{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
    assert_eq!(report.pass_count, seeds);
    assert_eq!(report.event_count, seeds * events);
    assert!(report.elections >= seeds && report.applies > 0);
    // The campaign must reach the situations its invariants are about: with
    // these at zero it would pass without having looked (lessons 11 and 12).
    if seeds >= 100 {
        assert!(
            report.stale_leader_requests > 0,
            "no request ever reached a deposed leader"
        );
        assert!(
            report.one_message_snapshots > 0,
            "no snapshot was ever delivered in a single message"
        );
    }
}

#[test]
fn production_core_simulation_trace_replays() {
    let mut a = Cluster::new(41);
    let mut b = Cluster::new(41);
    a.run(1000, true).unwrap();
    b.run(1000, true).unwrap();
    assert_eq!(
        serde_json::to_vec(&a.trace).unwrap(),
        serde_json::to_vec(&b.trace).unwrap()
    );
    assert!(a.oracle.election_count > 0 && a.oracle.apply_count > 0);
}


// Lesson 13. repro.sh appends this module to crates/raft-node/src/main.rs of the
// version under test and replaces the two markers below, because the actor has
// different fields before and after the fix. It builds a state machine just
// over the 64 MiB snapshot limit, then does what the actor does after every
// event: ask whether a snapshot is due.
#[cfg(test)]
mod lesson_13 {
    use super::*;

    #[test]
    fn lesson_13_oversize_state_machine() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/lesson-13")
            .join(format!("node-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let value = "v".repeat(4096);
        let mut entries = vec![LogEntry {
            index: 1,
            term: 1,
            command: Command::Config {
                voters: vec![1, 2, 3],
            },
        }];
        for index in 2..=16_401_u64 {
            entries.push(LogEntry {
                index,
                term: 1,
                command: Command::Client(ClientRequest {
                    client_id: 1,
                    sequence: index,
                    operation: Operation::Put {
                        key: format!("key-{index:08}"),
                        value: value.clone(),
                    },
                }),
            });
        }
        let mut kv = StateMachine::default();
        for entry in &entries {
            kv.apply(entry);
        }
        let mut core = Core::new(1, DurableState::new(1, Some(1), entries)).unwrap();
        core.commit_index = 16_401;
        core.last_applied = 16_401;
        let config: Config = toml::from_str(include_str!("../../../config/cluster.toml")).unwrap();
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let mut actor = Actor {
            core,
            wal: Wal::open(&dir, 1).unwrap(),
            kv,
            config,
            tx,
            rx,
            incoming: BTreeMap::new(),
            pending: BTreeMap::new(),
            outbound_active: Arc::new(AtomicUsize::new(0)),
            rpc_deadlines: BTreeMap::new(),
            election: None,
            contact: None,
            heartbeat: None,
            random: 1,
            snapshot_every: Some(1),
            snapshots_taken: 0,
            snapshots_installed: 0,
            /*LESSON_FIELDS*/
            reads: BTreeMap::new(),
            next_read: 0,
            reads_served: 0,
            reads_failed: 0,
            changes: BTreeMap::new(),
            next_change: 0,
        };
        println!(
            "LESSON state machine image: {} bytes; snapshot limit: {} bytes",
            actor.kv.snapshot().len(),
            MAX_SNAPSHOT
        );
        let mut total = 0;
        for event in 1..=10 {
            let started = Instant::now();
            actor.maybe_compact().unwrap();
            let millis = started.elapsed().as_millis();
            total += millis;
            println!("LESSON after event {event:2}: the snapshot check took {millis:4} ms");
        }
        println!("LESSON ten events: {total} ms spent on snapshot checks; snapshots taken: {}", actor.snapshots_taken);
        /*LESSON_REPORT*/
        drop(actor);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Lesson 11. Copied by repro.sh into crates/raft-storage/tests/ of the version
//! under test; it uses only the public API of raft-core and raft-storage, so the
//! same file builds before and after the fix.
//!
//! A follower whose durable term is 3 receives, as one message, a snapshot that
//! ends at index 6 of term 5. What does core ask the host to persist, and what
//! happens if the process dies after the snapshot file is durable but before
//! the WAL is rewritten?
use raft_core::{Core, Effect, Event};
use raft_protocol::*;
use raft_storage::Wal;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lesson-11")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn lesson_11_interrupted_install_from_a_newer_term() {
    let voters = Voters::from_ids(&[1, 2, 3]).unwrap();
    let dir = scratch("follower");
    // Node 2 was last up in term 3 and voted for node 3 then.
    let old = DurableState::new(
        3,
        Some(3),
        vec![
            LogEntry {
                index: 1,
                term: 1,
                command: Command::Config {
                    voters: vec![1, 2, 3],
                },
            },
            LogEntry {
                index: 2,
                term: 3,
                command: Command::NoOp,
            },
        ],
    );
    Wal::open(&dir, 2).unwrap().save(&old).unwrap();
    let wal = Wal::open(&dir, 2).unwrap();
    println!(
        "LESSON 1. on disk before: term {} vote {:?} last index {}",
        wal.state().term,
        wal.state().voted_for,
        wal.state().last_index()
    );
    let mut core = Core::restore(2, wal.state().clone(), None).unwrap();
    drop(wal);

    // Meanwhile node 1 became leader of term 5 and compacted through index 6.
    let data = b"{\"state machine image\":\"small enough for one message\"}".to_vec();
    let snapshot = Snapshot::new(LogBase { index: 6, term: 5 }, voters, data.clone());
    let effects = core.step(Event::Message(Envelope {
        version: 1,
        cluster_id: CLUSTER.into(),
        from: 1,
        to: 2,
        rpc_id: 1,
        body: Rpc::InstallSnapshot {
            term: 5,
            leader_id: 1,
            last_included_index: 6,
            last_included_term: 5,
            voters: voters.bits(),
            total_len: data.len() as u64,
            total_crc: snapshot.crc(),
            offset: 0,
            data: to_hex(&data),
            done: true,
        },
    }));
    let barriers: Vec<&Effect> = effects
        .iter()
        .filter(|e| matches!(e, Effect::Persist { .. } | Effect::PersistSnapshot { .. }))
        .collect();
    let [
        Effect::PersistSnapshot {
            state,
            snapshot: to_install,
            ..
        },
    ] = barriers[..]
    else {
        panic!("expected exactly one PersistSnapshot barrier");
    };
    println!(
        "LESSON 2. core asks for {} barrier: PersistSnapshot with term {} and a snapshot ending at index {} of term {}",
        barriers.len(),
        state.term,
        to_install.base.index,
        to_install.base.term
    );

    // Wal::install writes the snapshot file first and the WAL second. To get the
    // disk of a process killed between the two, let a throw-away directory do a
    // complete install and take only its snapshot file.
    let donor = scratch("donor");
    Wal::open(&donor, 2)
        .unwrap()
        .install(state, to_install)
        .unwrap();
    std::fs::copy(donor.join("snapshot"), dir.join("snapshot")).unwrap();
    println!("LESSON 3. killed between the two steps: new snapshot file, old WAL");

    for attempt in 1..=2 {
        match Wal::open(&dir, 2) {
            Ok(wal) => println!(
                "LESSON 4. restart {attempt}: STARTED with term {} vote {:?} log base index {} of term {}",
                wal.state().term,
                wal.state().voted_for,
                wal.state().base().index,
                wal.state().base().term
            ),
            Err(error) => println!("LESSON 4. restart {attempt}: REFUSED: {error}"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&donor);
}

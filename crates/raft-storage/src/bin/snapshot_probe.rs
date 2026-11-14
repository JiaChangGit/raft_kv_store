//! Real-process fixture for snapshot installation; never implements Raft.
//! Writes entries 1..=3 in term 1 if the directory is new, then installs a
//! snapshot covering entry 2. Opt-in failpoints stop it inside the install.
//! With `newer-term` it instead installs what a follower gets from a leader of
//! term 4 in a single message: a snapshot at index 5 of term 4, with the term
//! raised in the same install (lesson 11).
use raft_protocol::{Command, LogBase, LogEntry, Snapshot, Voters, adopt_snapshot};
use raft_storage::Wal;
use std::io::Write;

fn main() -> std::io::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let newer_term = args.get(2).map(String::as_str) == Some("newer-term");
    if !(args.len() == 2 || (args.len() == 3 && newer_term)) {
        return Err(std::io::Error::other(
            "usage: snapshot_probe DIRECTORY [newer-term]",
        ));
    }
    let mut wal = Wal::open(&args[1], 1)?;
    let mut state = wal.state().clone();
    if state.last_index() == 0 {
        state.term = 1;
        state.voted_for = Some(1);
        for index in 1..=3 {
            state.push(LogEntry {
                index,
                term: 1,
                command: Command::NoOp,
            });
        }
        wal.save(&state)?;
    }
    if state.base().index == 0 && newer_term {
        let voters = Voters::from_ids(&[1, 2, 3]).expect("valid");
        let snapshot = Snapshot::new(
            LogBase { index: 5, term: 4 },
            voters,
            b"state-at-5".to_vec(),
        );
        // What raft-core does on such an InstallSnapshot: a higher term clears
        // the vote, and one barrier carries both the term and the snapshot.
        state.term = 4;
        state.voted_for = None;
        wal.install(&adopt_snapshot(&state, snapshot.base), &snapshot)?;
    } else if state.base().index == 0 {
        let voters = Voters::from_ids(&[1, 2, 3]).expect("valid");
        let snapshot = Snapshot::new(
            LogBase { index: 2, term: 1 },
            voters,
            b"state-at-2".to_vec(),
        );
        wal.install(&adopt_snapshot(&state, snapshot.base), &snapshot)?;
    }
    println!("INSTALLED {}", wal.state().base().index);
    std::io::stdout().flush()
}

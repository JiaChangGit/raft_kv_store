//! Lesson 19 helper, copied into a source tree as an example of raft-storage:
//! which rule refuses each case? Prints the error that `save_delta` returns.
use raft_protocol::{ClientRequest, Command, Delta, DurableState, LogEntry, Operation};
use raft_storage::Wal;

/// The same entry as the helper in crates/raft-storage/src/tests.rs.
fn entry(index: u64, term: u64, value: &str) -> LogEntry {
    LogEntry {
        index,
        term,
        command: Command::Client(ClientRequest {
            client_id: 1,
            sequence: index,
            operation: Operation::Put {
                key: "x".into(),
                value: value.into(),
            },
        }),
    }
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("an empty directory"));
    let mut wal = Wal::open(&dir, 1).unwrap();
    // As in the test: term 2, vote for node 1, entries 1..=3 of term 2.
    let mut state = DurableState::new(2, Some(1), vec![]);
    for index in 1..=3 {
        state.push(entry(index, 2, "x"));
    }
    wal.save(&state).unwrap();
    let cases = [
        (
            "S5 case \"entry term below the previous entry\"",
            Delta {
                term: 3,
                voted_for: Some(1),
                truncate_from: 2,
                append: vec![entry(2, 3, "y"), entry(3, 2, "z")],
            },
        ),
        (
            "S6 case \"new entry term below the previous entry\"",
            Delta {
                term: 2,
                voted_for: Some(1),
                truncate_from: 4,
                append: vec![entry(4, 1, "y")],
            },
        ),
    ];
    for (what, delta) in cases {
        match wal.save_delta(&delta) {
            Ok(()) => println!("   {what}: ACCEPTED"),
            Err(error) => println!("   {what}: refused, \"{error}\""),
        }
    }
}

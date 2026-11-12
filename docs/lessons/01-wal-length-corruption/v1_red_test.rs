//! A regression test written for the OLD format 1 code. `repro.sh` copies it into
//! a checkout of git tag v1-baseline-verified-20261112 and runs it there, where
//! it is expected to FAIL. It states the rule that code breaks:
//! a WAL whose length field was damaged must be refused and left unchanged.
use raft_protocol::{Command, DurableState, LogEntry};
use raft_storage::{HEADER_LEN, Wal};
use std::fs;

#[test]
fn damaged_length_must_not_cut_the_log() {
    assert_eq!(HEADER_LEN, 20, "written for the format 1 record layout");
    let dir = std::env::temp_dir().join(format!("wal-red-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let mut state = DurableState::default();
    {
        let mut wal = Wal::open(&dir, 1).unwrap();
        for term in 1..=2 {
            state.term = term;
            state.voted_for = Some(1);
            state.log.push(LogEntry {
                index: term,
                term,
                command: Command::NoOp,
            });
            wal.save(&state).unwrap();
        }
    }
    let path = dir.join("raft.wal");
    let mut damaged = fs::read(&path).unwrap();
    // Format 1: the first record's payload length is at bytes 8..12.
    damaged[8..12].copy_from_slice(&65_536_u32.to_le_bytes());
    fs::write(&path, &damaged).unwrap();

    let reopened = Wal::open(&dir, 1).map(|wal| (wal.state().term, wal.state().log.len()));
    let after = fs::read(&path).unwrap();
    assert!(
        reopened.is_err(),
        "the damaged WAL was opened: durable state went from (term 2, 2 entries) to {reopened:?}"
    );
    assert_eq!(after, damaged, "the damaged WAL was modified");
}

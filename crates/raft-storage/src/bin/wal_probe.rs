//! Small real-process crash fixture; never implements Raft or a second consensus core.
use raft_protocol::{Command, LogEntry};
use raft_storage::Wal;
use std::io::Write;

fn main() -> std::io::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err(std::io::Error::other("usage: wal_probe DIRECTORY TERM"));
    }
    let mut wal = Wal::open(&args[1], 1)?;
    let mut state = wal.state().clone();
    state.term = args[2].parse().map_err(std::io::Error::other)?;
    state.voted_for = Some(1);
    state.log.push(LogEntry {
        index: state.last_index() + 1,
        term: state.term,
        command: Command::NoOp,
    });
    wal.save(&state)?;
    println!("DURABLE {}", wal.durable_sequence());
    std::io::stdout().flush()
}

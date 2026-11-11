//! Append-only, checksummed transactions. A successful save means sync_all completed.
//! Storage restores persistent consensus data only; it never decides commitment.
use raft_protocol::{CLUSTER, DurableState, LogEntry, NodeId, valid_node};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const HEADER_LEN: usize = 20;
pub const MAX_RECORD_PAYLOAD: usize = 2 * 1024 * 1024;
const MAGIC: &[u8; 4] = b"RFT1";
const VERSION: u16 = 1;
const TRANSACTION: u16 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    cluster_id: String,
    node_id: NodeId,
    format_version: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    term: u64,
    voted_for: Option<NodeId>,
    /// First suffix index removed; append is atomic with this truncation.
    truncate_from: u64,
    append: Vec<LogEntry>,
}

pub struct Wal {
    file: File,
    // Held for the entire lifetime; prevents two writers owning one node directory.
    _lock: File,
    state: DurableState,
    sequence: u64,
    poisoned: bool,
    fail_sync: bool,
    directory: PathBuf,
    last_sync_us: f64,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

fn create_directory(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if parent != path {
        create_directory(parent)?;
    }
    fs::create_dir(path)?;
    sync_directory(parent)?;
    sync_directory(path)
}

/// Reflected CRC32C/Castagnoli: polynomial 0x82F63B78, init/final xor 0xFFFFFFFF.
pub fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0x82f6_3b78 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn validate_change(previous: &DurableState, next: &DurableState) -> io::Result<()> {
    if !next.valid() || next.term < previous.term {
        return Err(invalid("invalid or decreasing persistent state"));
    }
    if next.term == previous.term
        && previous.voted_for.is_some()
        && next.voted_for != previous.voted_for
    {
        return Err(invalid(
            "cannot clear or change a durable vote in the same term",
        ));
    }
    for (old, new) in previous.log.iter().zip(&next.log) {
        if old.term == new.term && old != new {
            return Err(invalid(
                "same index and term cannot identify different commands",
            ));
        }
    }
    Ok(())
}

fn apply_transaction(
    previous: &DurableState,
    transaction: Transaction,
) -> io::Result<DurableState> {
    if transaction.truncate_from == 0 || transaction.truncate_from > previous.last_index() + 1 {
        return Err(invalid("transaction truncation index outside current log"));
    }
    let mut next = DurableState {
        term: transaction.term,
        voted_for: transaction.voted_for,
        log: previous.log.clone(),
    };
    next.log.truncate((transaction.truncate_from - 1) as usize);
    next.log.extend(transaction.append);
    validate_change(previous, &next)?;
    Ok(next)
}

fn header(sequence: u64, payload_len: usize) -> [u8; HEADER_LEN] {
    let mut out = [0; HEADER_LEN];
    out[..4].copy_from_slice(MAGIC);
    out[4..6].copy_from_slice(&VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&TRANSACTION.to_le_bytes());
    out[8..12].copy_from_slice(&(payload_len as u32).to_le_bytes());
    out[12..20].copy_from_slice(&sequence.to_le_bytes());
    out
}

fn check_header(bytes: &[u8], sequence: u64) -> io::Result<Option<usize>> {
    let expected = header(sequence, 1);
    for index in 0..bytes.len().min(HEADER_LEN) {
        if !(8..12).contains(&index) && bytes[index] != expected[index] {
            return Err(invalid(
                "WAL magic, version, record type or sequence mismatch",
            ));
        }
    }
    if bytes.len() < 12 {
        return Ok(None);
    }
    let length = u32::from_le_bytes(bytes[8..12].try_into().expect("four length bytes")) as usize;
    if length == 0 || length > MAX_RECORD_PAYLOAD {
        return Err(invalid("WAL record length out of bounds"));
    }
    Ok(Some(length))
}

impl Wal {
    pub fn open(path: impl AsRef<Path>, node_id: NodeId) -> io::Result<Self> {
        if !valid_node(node_id) {
            return Err(invalid("node ID must be 1, 2 or 3"));
        }
        let directory = path.as_ref().to_path_buf();
        create_directory(&directory)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("node.lock"))?;
        lock.try_lock()
            .map_err(|error| io::Error::other(format!("node directory already owned: {error}")))?;
        let metadata_path = directory.join("node.meta");
        let wal_path = directory.join("raft.wal");
        let existing_identity = metadata_path.exists();
        if existing_identity {
            if fs::metadata(&metadata_path)?.len() > 4096 {
                return Err(invalid("identity metadata too large"));
            }
            let identity: Identity = serde_json::from_slice(&fs::read(&metadata_path)?)
                .map_err(|e| invalid(e.to_string()))?;
            if identity.cluster_id != CLUSTER
                || identity.node_id != node_id
                || identity.format_version != VERSION
            {
                return Err(invalid("node, cluster or storage format identity mismatch"));
            }
        } else {
            if wal_path.exists() {
                return Err(invalid("WAL exists without identity metadata"));
            }
            let identity = Identity {
                cluster_id: CLUSTER.into(),
                node_id,
                format_version: VERSION,
            };
            let bytes = serde_json::to_vec(&identity).map_err(io::Error::other)?;
            let mut metadata = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&metadata_path)?;
            metadata.write_all(&bytes)?;
            metadata.sync_all()?;
            sync_directory(&directory)?;
        }
        let file = if existing_identity {
            // Missing WAL is ambiguous (loss or interrupted initialization),
            // never evidence that an existing identity is a fresh node.
            OpenOptions::new().read(true).write(true).open(&wal_path)?
        } else {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&wal_path)?;
            file.sync_all()?;
            sync_directory(&directory)?;
            file
        };
        let mut wal = Self {
            file,
            _lock: lock,
            state: DurableState::default(),
            sequence: 0,
            poisoned: false,
            fail_sync: false,
            directory,
            last_sync_us: 0.0,
        };
        wal.recover()?;
        Ok(wal)
    }

    fn recover(&mut self) -> io::Result<()> {
        self.recover_with_sync(File::sync_all)
    }

    fn recover_with_sync(&mut self, sync: impl FnOnce(&File) -> io::Result<()>) -> io::Result<()> {
        let length = self.file.metadata()?.len();
        let mut offset = 0_u64;
        while offset < length {
            self.file.seek(SeekFrom::Start(offset))?;
            let available = (length - offset).min(HEADER_LEN as u64) as usize;
            let mut record_header = [0_u8; HEADER_LEN];
            self.file.read_exact(&mut record_header[..available])?;
            let next_sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| invalid("WAL sequence exhausted"))?;
            let payload_len = check_header(&record_header[..available], next_sequence)?;
            if available < HEADER_LEN {
                self.truncate_tail(offset)?;
                break;
            }
            let payload_len = payload_len.ok_or_else(|| invalid("missing WAL length"))?;
            let record_len = (HEADER_LEN + payload_len + 4) as u64;
            if length - offset < record_len {
                self.truncate_tail(offset)?;
                break;
            }
            let mut payload = vec![0; payload_len];
            self.file.read_exact(&mut payload)?;
            let mut checksum = [0; 4];
            self.file.read_exact(&mut checksum)?;
            let mut checksum_input = Vec::with_capacity(HEADER_LEN + payload_len);
            checksum_input.extend_from_slice(&record_header);
            checksum_input.extend_from_slice(&payload);
            if crc32c(&checksum_input) != u32::from_le_bytes(checksum) {
                return Err(invalid("WAL checksum mismatch; refusing repair"));
            }
            let transaction = serde_json::from_slice(&payload)
                .map_err(|e| invalid(format!("invalid transaction: {e}")))?;
            self.state = apply_transaction(&self.state, transaction)?;
            self.sequence = next_sequence;
            offset += record_len;
        }
        self.file.seek(SeekFrom::End(0))?;
        // Complete records can survive an unsynced crash. Establish a durable
        // recovery baseline before Core may ACK an unchanged recovered vote/log.
        if let Err(error) = sync(&self.file) {
            self.poisoned = true;
            return Err(error);
        }
        Ok(())
    }

    fn truncate_tail(&mut self, offset: u64) -> io::Result<()> {
        self.file.set_len(offset)?;
        self.file.sync_all()
    }

    pub fn state(&self) -> &DurableState {
        &self.state
    }
    pub fn durable_sequence(&self) -> u64 {
        self.sequence
    }
    /// Time of the latest transaction's actual sync_all call, excluding encoding/write.
    pub fn last_sync_us(&self) -> f64 {
        self.last_sync_us
    }
    pub fn poisoned(&self) -> bool {
        self.poisoned
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    /// Deterministic failure injection. Once a save fails, this instance cannot resume.
    pub fn inject_sync_failure(&mut self, fail: bool) {
        self.fail_sync = fail;
    }

    pub fn save(&mut self, next: &DurableState) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(
                "WAL is poisoned; restart and recover before use",
            ));
        }
        validate_change(&self.state, next)?;
        if next == &self.state {
            return Ok(());
        }
        let common = self
            .state
            .log
            .iter()
            .zip(&next.log)
            .take_while(|(left, right)| left == right)
            .count();
        let transaction = Transaction {
            term: next.term,
            voted_for: next.voted_for,
            truncate_from: common as u64 + 1,
            append: next.log[common..].to_vec(),
        };
        let payload = serde_json::to_vec(&transaction).map_err(io::Error::other)?;
        if payload.len() > MAX_RECORD_PAYLOAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "WAL transaction exceeds payload bound",
            ));
        }
        let next_sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("WAL sequence exhausted"))?;
        let mut record = Vec::with_capacity(HEADER_LEN + payload.len() + 4);
        record.extend_from_slice(&header(next_sequence, payload.len()));
        record.extend_from_slice(&payload);
        record.extend_from_slice(&crc32c(&record).to_le_bytes());
        failpoint("before_write");
        let result = (|| {
            self.file.write_all(&record)?;
            failpoint("after_write_before_sync");
            if self.fail_sync {
                return Err(io::Error::other("injected sync failure"));
            }
            let started = Instant::now();
            let result = self.file.sync_all();
            self.last_sync_us = started.elapsed().as_secs_f64() * 1_000_000.0;
            result
        })();
        if let Err(error) = result {
            self.poisoned = true;
            return Err(error);
        }
        self.state = next.clone();
        self.sequence = next_sequence;
        failpoint("after_sync_before_ack");
        Ok(())
    }
}

/// Test-only opt-in environment hook: mark the reached boundary, wait for SIGKILL.
/// Ordinary runtime does not set these variables. This intentionally never returns.
fn failpoint(name: &str) {
    if std::env::var("RAFT_WAL_FAILPOINT").as_deref() != Ok(name) {
        return;
    }
    if let Ok(marker) = std::env::var("RAFT_WAL_FAILPOINT_MARKER")
        && let Ok(mut file) = File::create(marker)
    {
        let _ = file.write_all(name.as_bytes());
        let _ = file.sync_all();
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kv_state::StateMachine;
    use raft_core::Core;
    use raft_protocol::{ClientRequest, Command, Operation};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/storage-tests")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn state(term: u64, voted_for: Option<u8>) -> DurableState {
        DurableState {
            term,
            voted_for,
            log: vec![],
        }
    }
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

    #[test]
    fn crc32c_has_castagnoli_known_vector() {
        assert_eq!(crc32c(b"123456789"), 0xe3069283);
        assert_eq!(crc32c(b""), 0);
    }

    #[test]
    fn existing_identity_without_wal_fails_closed() {
        let dir = TestDir::new();
        {
            let mut wal = Wal::open(&dir.0, 1).unwrap();
            wal.save(&state(5, Some(2))).unwrap();
        }
        fs::rename(dir.0.join("raft.wal"), dir.0.join("saved.wal")).unwrap();
        assert!(
            Wal::open(&dir.0, 1).is_err(),
            "must not silently reset a used node"
        );
        assert!(!dir.0.join("raft.wal").exists());
    }

    #[test]
    fn recovered_prefix_requires_successful_sync() {
        let dir = TestDir::new();
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        wal.save(&state(5, Some(2))).unwrap();
        wal.state = DurableState::default();
        wal.sequence = 0;
        let called = std::cell::Cell::new(false);
        let result = wal.recover_with_sync(|_| {
            called.set(true);
            Err(io::Error::other("recovery sync failure"))
        });
        assert!(called.get());
        assert!(result.is_err());
        assert!(wal.poisoned());
        assert!(wal.save(&state(6, Some(1))).is_err());
        drop(wal);
        let recovered = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(recovered.state().term, 5);
        assert_eq!(recovered.state().voted_for, Some(2));
    }

    #[test]
    fn vote_after_restart_not_duplicated() {
        let dir = TestDir::new();
        {
            let mut wal = Wal::open(&dir.0, 1).unwrap();
            wal.save(&state(4, Some(2))).unwrap();
            assert_eq!(wal.durable_sequence(), 1);
        }
        let mut recovered = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(recovered.state().voted_for, Some(2));
        assert!(recovered.save(&state(4, Some(3))).is_err());
        assert!(recovered.save(&state(4, None)).is_err());
        recovered.save(&state(5, None)).unwrap();
        recovered.save(&state(5, Some(3))).unwrap();
        assert_eq!(recovered.state().voted_for, Some(3));
    }

    #[test]
    fn append_ack_after_durable_sync() {
        let dir = TestDir::new();
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        let mut next = state(3, Some(1));
        next.log.push(entry(1, 3, "A"));
        wal.inject_sync_failure(true);
        let mut acknowledgments = vec![];
        if wal.save(&next).is_ok() {
            acknowledgments.push(next.last_index());
        }
        assert!(acknowledgments.is_empty());
        assert_eq!(wal.durable_sequence(), 0);
        assert_eq!(wal.state().last_index(), 0);
        assert!(wal.poisoned());
        wal.inject_sync_failure(false);
        assert!(
            wal.save(&next).is_err(),
            "I/O failure cannot silently resume service"
        );
        drop(wal);
        let mut recovered = Wal::open(&dir.0, 1).unwrap();
        // A complete, unacknowledged transaction is allowed to survive.
        assert_eq!(recovered.state(), &next);
        let mut later = next.clone();
        later.log.push(entry(2, 3, "B"));
        recovered.save(&later).unwrap();
        acknowledgments.push(later.last_index());
        assert_eq!(acknowledgments, vec![2]);
        assert_eq!(recovered.durable_sequence(), 2);
        drop(recovered);
        assert_eq!(Wal::open(&dir.0, 1).unwrap().state(), &later);
    }

    #[test]
    fn uncommitted_suffix_not_applied_after_recovery() {
        let dir = TestDir::new();
        let mut persistent = state(2, Some(1));
        persistent.log.push(entry(1, 2, "uncommitted"));
        {
            let mut wal = Wal::open(&dir.0, 1).unwrap();
            wal.save(&persistent).unwrap();
        }
        let recovered = Wal::open(&dir.0, 1).unwrap();
        let core = Core::new(1, recovered.state().clone()).unwrap();
        let kv = StateMachine::default();
        assert_eq!(core.state.log.len(), 1);
        assert_eq!(core.commit_index, 0);
        assert_eq!(core.last_applied, 0);
        assert_eq!(kv.get("x"), None);
        assert_eq!(kv.session_count(), 0);
    }

    #[test]
    fn truncated_wal_tail_safe() {
        let dir = TestDir::new();
        let mut first = state(2, Some(1));
        first.log.push(entry(1, 2, "A"));
        let second = {
            let mut wal = Wal::open(&dir.0, 1).unwrap();
            wal.save(&first).unwrap();
            let mut next = first.clone();
            next.log.push(entry(2, 2, "B"));
            wal.save(&next).unwrap();
            next
        };
        let path = dir.0.join("raft.wal");
        let bytes = fs::read(&path).unwrap();
        let first_len = {
            let payload = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
            HEADER_LEN + payload + 4
        };
        for cut in first_len + 1..bytes.len() {
            fs::write(&path, &bytes[..cut]).unwrap();
            let wal = Wal::open(&dir.0, 1).unwrap();
            assert_eq!(wal.state(), &first, "cut={cut}");
            assert_eq!(fs::metadata(&path).unwrap().len(), first_len as u64);
            drop(wal);
        }
        fs::write(&path, &bytes).unwrap();
        {
            let wal = Wal::open(&dir.0, 1).unwrap();
            assert_eq!(wal.state(), &second);
        }
        let mut corrupt = bytes.clone();
        corrupt[HEADER_LEN + 1] ^= 1;
        fs::write(&path, &corrupt).unwrap();
        assert!(
            Wal::open(&dir.0, 1).is_err(),
            "middle corruption must fail closed"
        );
        let mut corrupt = bytes.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        fs::write(&path, &corrupt).unwrap();
        assert!(
            Wal::open(&dir.0, 1).is_err(),
            "complete final CRC mismatch must fail closed"
        );
        fs::write(&path, &bytes).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"garbage").unwrap();
        drop(file);
        assert!(
            Wal::open(&dir.0, 1).is_err(),
            "arbitrary garbage is not proven partial header"
        );
    }

    #[test]
    fn atomic_suffix_transaction_and_identity_are_checked() {
        let dir = TestDir::new();
        let mut initial = state(2, Some(1));
        initial.log = vec![entry(1, 1, "A"), entry(2, 2, "B")];
        let replacement = {
            let mut wal = Wal::open(&dir.0, 1).unwrap();
            assert!(Wal::open(&dir.0, 1).is_err());
            wal.save(&initial).unwrap();
            let mut next = initial.clone();
            next.term = 3;
            next.voted_for = None;
            next.log[1] = entry(2, 3, "C");
            next.log.push(entry(3, 3, "D"));
            wal.save(&next).unwrap();
            next
        };
        {
            let mut wal = Wal::open(&dir.0, 1).unwrap();
            assert_eq!(wal.state(), &replacement);
            let mut wrong = replacement.clone();
            wrong.log[1].command = entry(2, 3, "E").command;
            assert!(wal.save(&wrong).is_err());
        }
        assert!(Wal::open(&dir.0, 2).is_err());
    }
}

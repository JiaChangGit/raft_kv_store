//! Append-only, checksummed transactions (WAL format 2). A successful save means
//! sync_all completed. Storage restores persistent consensus data only; it never
//! decides commitment. Byte layout and recovery rules: docs/STORAGE_FORMAT_V2.md.
use raft_protocol::{CLUSTER, DurableState, LogEntry, NodeId, valid_node};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(test)]
mod tests;

pub const FORMAT_VERSION: u16 = 2;
/// Fixed file header at offset 0; records follow it.
pub const FILE_HEADER_LEN: usize = 80;
/// Fixed record header; the last four bytes are its own checksum.
pub const HEADER_LEN: usize = 24;
pub const MAX_RECORD_PAYLOAD: usize = 2 * 1024 * 1024;
/// Preserved copies of removed tails kept under `torn/`.
pub const MAX_TORN_COPIES: usize = 8;
const FILE_MAGIC: &[u8; 4] = b"RFTW";
const RECORD_MAGIC: &[u8; 4] = b"RFTR";
const TRANSACTION: u16 = 1;
const CLUSTER_FIELD: std::ops::Range<usize> = 12..36;
const _: () = assert!(CLUSTER.len() <= CLUSTER_FIELD.end - CLUSTER_FIELD.start);

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

/// Where a WAL file starts. Format 2 reserves these fields so that log
/// compaction can later begin a file after a snapshot without another format
/// change; this build writes and accepts only [`PRISTINE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStart {
    base_index: u64,
    base_term: u64,
    start_term: u64,
    first_sequence: u64,
    start_voted_for: u32,
}
const PRISTINE: FileStart = FileStart {
    base_index: 0,
    base_term: 0,
    start_term: 0,
    first_sequence: 1,
    start_voted_for: 0,
};

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

/// Write a whole small file so that a crash leaves either no file or all of it.
fn write_atomically(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let temporary = directory.join(format!("{name}.tmp"));
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    failpoint(&format!("init_before_rename_{name}"));
    fs::rename(&temporary, directory.join(name))?;
    sync_directory(directory)
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

fn file_header(node_id: NodeId, start: FileStart) -> [u8; FILE_HEADER_LEN] {
    let mut out = [0; FILE_HEADER_LEN];
    out[..4].copy_from_slice(FILE_MAGIC);
    out[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&(FILE_HEADER_LEN as u16).to_le_bytes());
    out[8..12].copy_from_slice(&u32::from(node_id).to_le_bytes());
    out[CLUSTER_FIELD.start..CLUSTER_FIELD.start + CLUSTER.len()]
        .copy_from_slice(CLUSTER.as_bytes());
    // 36..40 is reserved and stays zero.
    out[40..48].copy_from_slice(&start.base_index.to_le_bytes());
    out[48..56].copy_from_slice(&start.base_term.to_le_bytes());
    out[56..64].copy_from_slice(&start.start_term.to_le_bytes());
    out[64..72].copy_from_slice(&start.first_sequence.to_le_bytes());
    out[72..76].copy_from_slice(&start.start_voted_for.to_le_bytes());
    let checksum = crc32c(&out[..76]);
    out[76..80].copy_from_slice(&checksum.to_le_bytes());
    out
}

fn check_file_header(bytes: &[u8; FILE_HEADER_LEN], node_id: NodeId) -> io::Result<()> {
    if crc32c(&bytes[..76]) != u32::from_le_bytes(bytes[76..80].try_into().expect("four bytes")) {
        return Err(invalid("WAL file header checksum mismatch"));
    }
    let expected = file_header(node_id, PRISTINE);
    if bytes[..8] != expected[..8] {
        return Err(invalid("not a format 2 WAL file header"));
    }
    if bytes[8..CLUSTER_FIELD.end] != expected[8..CLUSTER_FIELD.end] {
        return Err(invalid("WAL file header names another node or cluster"));
    }
    if bytes != &expected {
        return Err(invalid(
            "WAL file header declares a compacted start; unsupported by this build",
        ));
    }
    Ok(())
}

fn record_header(sequence: u64, payload_len: usize) -> [u8; HEADER_LEN] {
    let mut out = [0; HEADER_LEN];
    out[..4].copy_from_slice(RECORD_MAGIC);
    out[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&TRANSACTION.to_le_bytes());
    out[8..12].copy_from_slice(&(payload_len as u32).to_le_bytes());
    out[12..20].copy_from_slice(&sequence.to_le_bytes());
    let checksum = crc32c(&out[..20]);
    out[20..24].copy_from_slice(&checksum.to_le_bytes());
    out
}

fn length_in_bounds(bytes: &[u8]) -> io::Result<usize> {
    let length = u32::from_le_bytes(bytes[8..12].try_into().expect("four length bytes")) as usize;
    if length == 0 || length > MAX_RECORD_PAYLOAD {
        return Err(invalid("WAL record length out of bounds"));
    }
    Ok(length)
}

/// Validate a complete record header and return its payload length.
///
/// The length decides whether the rest of the file is "a record that was never
/// finished" (safe to remove) or "a complete record" (must be kept), so it is
/// trusted only after the header's own checksum has passed.
fn check_header(bytes: &[u8; HEADER_LEN], sequence: u64) -> io::Result<usize> {
    let stored = u32::from_le_bytes(bytes[20..24].try_into().expect("four checksum bytes"));
    if crc32c(&bytes[..20]) != stored {
        return Err(invalid(
            "WAL record header checksum mismatch; refusing repair",
        ));
    }
    let expected = record_header(sequence, 1);
    if bytes[..8] != expected[..8] {
        return Err(invalid("WAL record magic, version or type mismatch"));
    }
    let length = length_in_bounds(bytes)?;
    if bytes[12..20] != expected[12..20] {
        return Err(invalid("WAL record sequence is not the next one"));
    }
    Ok(length)
}

/// Accept fewer than HEADER_LEN trailing bytes only if every byte that can be
/// checked is what the next record's header would begin with.
fn check_partial_header(bytes: &[u8], sequence: u64) -> io::Result<()> {
    let mismatch = || invalid("incomplete WAL tail is not the start of the next record");
    let expected = record_header(sequence, 1);
    for (index, byte) in bytes.iter().enumerate().take(20) {
        // The length (8..12) is unknown in advance; everything else is predictable.
        if !(8..12).contains(&index) && *byte != expected[index] {
            return Err(mismatch());
        }
    }
    if bytes.len() >= 12 {
        length_in_bounds(bytes)?;
    }
    if bytes.len() > 20 {
        let checksum = crc32c(&bytes[..20]).to_le_bytes();
        if bytes[20..] != checksum[..bytes.len() - 20] {
            return Err(mismatch());
        }
    }
    Ok(())
}

fn torn_copy_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("seq")
        .and_then(|n| n.strip_suffix(".bin"))
    else {
        return false;
    };
    let Some((sequence, checksum)) = rest.split_once('-') else {
        return false;
    };
    sequence.len() == 20
        && sequence.bytes().all(|b| b.is_ascii_digit())
        && checksum.len() == 8
        && checksum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Keep the bytes about to be removed. Identical tails map to one file name, so
/// repeating an interrupted recovery does not multiply copies.
fn preserve_torn_tail(directory: &Path, sequence: u64, tail: &[u8]) -> io::Result<()> {
    let torn = directory.join("torn");
    if !torn.is_dir() {
        fs::create_dir(&torn)?;
        sync_directory(directory)?;
    }
    let name = format!("seq{sequence:020}-{:08x}.bin", crc32c(tail));
    let temporary = torn.join(format!("{name}.tmp"));
    let mut file = File::create(&temporary)?;
    file.write_all(tail)?;
    file.sync_all()?;
    fs::rename(&temporary, torn.join(&name))?;
    let mut older = vec![];
    for entry in fs::read_dir(&torn)? {
        let found = entry?.file_name().to_string_lossy().into_owned();
        if let Some(finished) = found.strip_suffix(".tmp")
            && torn_copy_name(finished)
        {
            // Left by a recovery that was itself interrupted; never data.
            fs::remove_file(torn.join(&found))?;
        } else if torn_copy_name(&found) && found != name {
            older.push(found);
        }
    }
    older.sort();
    while older.len() >= MAX_TORN_COPIES {
        fs::remove_file(torn.join(older.remove(0)))?;
    }
    sync_directory(&torn)
}

fn check_identity(path: &Path, node_id: NodeId) -> io::Result<()> {
    if fs::metadata(path)?.len() > 4096 {
        return Err(invalid("identity metadata too large"));
    }
    let identity: Identity =
        serde_json::from_slice(&fs::read(path)?).map_err(|e| invalid(e.to_string()))?;
    if identity.format_version == 1 {
        return Err(invalid(
            "data directory uses WAL format 1; this build reads only format 2 and never converts data",
        ));
    }
    if identity.cluster_id != CLUSTER
        || identity.node_id != node_id
        || identity.format_version != FORMAT_VERSION
    {
        return Err(invalid("node, cluster or storage format identity mismatch"));
    }
    Ok(())
}

fn create_identity(directory: &Path, node_id: NodeId) -> io::Result<()> {
    let identity = Identity {
        cluster_id: CLUSTER.into(),
        node_id,
        format_version: FORMAT_VERSION,
    };
    let bytes = serde_json::to_vec(&identity).map_err(io::Error::other)?;
    write_atomically(directory, "node.meta", &bytes)
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
        // First initialization writes the WAL, then the identity. Identity is
        // therefore the proof that initialization finished: with it, a missing
        // WAL is data loss; without it, only an untouched header is acceptable.
        match (metadata_path.exists(), wal_path.exists()) {
            (false, false) => {
                write_atomically(&directory, "raft.wal", &file_header(node_id, PRISTINE))?;
                failpoint("init_after_wal_before_identity");
                create_identity(&directory, node_id)?;
            }
            (false, true) => {
                let untouched = fs::metadata(&wal_path)?.len() == FILE_HEADER_LEN as u64
                    && fs::read(&wal_path)? == file_header(node_id, PRISTINE);
                if !untouched {
                    return Err(invalid("WAL exists without identity metadata"));
                }
                create_identity(&directory, node_id)?;
            }
            (true, false) => {
                check_identity(&metadata_path, node_id)?;
                // Loss, not a fresh node: never recreate an empty WAL here.
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "identity exists but raft.wal is missing; refusing to assume an empty node",
                ));
            }
            (true, true) => check_identity(&metadata_path, node_id)?,
        }
        let mut file = OpenOptions::new().read(true).write(true).open(&wal_path)?;
        if file.metadata()?.len() < FILE_HEADER_LEN as u64 {
            return Err(invalid("WAL is shorter than its file header"));
        }
        let mut header = [0_u8; FILE_HEADER_LEN];
        file.read_exact(&mut header)?;
        check_file_header(&header, node_id)?;
        let mut wal = Self {
            file,
            _lock: lock,
            state: DurableState::default(),
            sequence: PRISTINE.first_sequence - 1,
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
        let mut offset = FILE_HEADER_LEN as u64;
        while offset < length {
            let remaining = length - offset;
            let available = remaining.min(HEADER_LEN as u64) as usize;
            let mut header = [0_u8; HEADER_LEN];
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.read_exact(&mut header[..available])?;
            let next_sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| invalid("WAL sequence exhausted"))?;
            if available < HEADER_LEN {
                check_partial_header(&header[..available], next_sequence)?;
                self.remove_torn_tail(offset, length, next_sequence)?;
                break;
            }
            let payload_len = check_header(&header, next_sequence)?;
            let record_len = (HEADER_LEN + payload_len + 4) as u64;
            if remaining < record_len {
                // The header is authentic and says more bytes were coming.
                self.remove_torn_tail(offset, length, next_sequence)?;
                break;
            }
            let mut payload = vec![0; payload_len];
            self.file.read_exact(&mut payload)?;
            let mut checksum = [0; 4];
            self.file.read_exact(&mut checksum)?;
            let mut checksum_input = Vec::with_capacity(HEADER_LEN + payload_len);
            checksum_input.extend_from_slice(&header);
            checksum_input.extend_from_slice(&payload);
            if crc32c(&checksum_input) != u32::from_le_bytes(checksum) {
                return Err(invalid("WAL record checksum mismatch; refusing repair"));
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

    /// Remove an unfinished final record, after a copy of it is durable.
    fn remove_torn_tail(&mut self, offset: u64, length: u64, sequence: u64) -> io::Result<()> {
        // Bounded: shorter than one header plus one maximum payload and checksum.
        let mut tail = vec![0; (length - offset) as usize];
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut tail)?;
        preserve_torn_tail(&self.directory, sequence, &tail)?;
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
        record.extend_from_slice(&record_header(next_sequence, payload.len()));
        record.extend_from_slice(&payload);
        record.extend_from_slice(&crc32c(&record).to_le_bytes());
        failpoint("before_write");
        let result = (|| {
            partial_write_failpoint(&mut self.file, &record)?;
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

fn failpoint_selected(name: &str) -> bool {
    std::env::var("RAFT_WAL_FAILPOINT").as_deref() == Ok(name)
}

/// Test-only opt-in environment hook: mark the reached boundary, wait for SIGKILL.
/// Ordinary runtime does not set these variables. This intentionally never returns.
fn failpoint(name: &str) {
    if !failpoint_selected(name) {
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

/// Test-only: leave just the beginning of a record on disk, then wait to be
/// killed, so that recovery of a torn tail is exercised by a real process.
fn partial_write_failpoint(file: &mut File, record: &[u8]) -> io::Result<()> {
    let cuts = [
        ("after_partial_header", HEADER_LEN / 2 + 1),
        (
            "after_partial_payload",
            HEADER_LEN + (record.len() - HEADER_LEN) / 2,
        ),
    ];
    for (name, cut) in cuts {
        if failpoint_selected(name) {
            file.write_all(&record[..cut])?;
            file.sync_all()?;
            failpoint(name);
        }
    }
    Ok(())
}

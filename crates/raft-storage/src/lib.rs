//! Append-only, checksummed transactions (WAL format 2) plus one snapshot file.
//! A successful save or install means sync_all completed. Storage restores
//! persistent consensus data only; it never decides commitment. Byte layout and
//! recovery rules: docs/STORAGE_FORMAT_V2.md and docs/SNAPSHOT_COMPACTION.md.
pub use raft_protocol::crc32c;
use raft_protocol::{
    CLUSTER, DurableState, LogBase, LogEntry, MAX_SNAPSHOT, NodeId, Snapshot, adopt_snapshot,
    valid_node,
};
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
/// Fixed snapshot file header; the payload follows it.
pub const SNAPSHOT_HEADER_LEN: usize = 64;
const SNAPSHOT_MAGIC: &[u8; 4] = b"RFTS";
const SNAPSHOT_VERSION: u16 = 1;
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

/// Where a WAL file starts. A fresh node starts at [`PRISTINE`]; after a
/// snapshot the file is rewritten to start right after it, carrying the term
/// and vote in force at that moment.
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

impl FileStart {
    fn of(state: &DurableState, first_sequence: u64) -> Self {
        Self {
            base_index: state.base().index,
            base_term: state.base().term,
            start_term: state.term,
            first_sequence,
            start_voted_for: state.voted_for.map_or(0, u32::from),
        }
    }
    fn state(&self) -> DurableState {
        let vote = (self.start_voted_for != 0).then_some(self.start_voted_for as NodeId);
        let base = LogBase {
            index: self.base_index,
            term: self.base_term,
        };
        DurableState::with_base(self.start_term, vote, base, vec![])
    }
}

pub struct Wal {
    file: File,
    // Held for the entire lifetime; prevents two writers owning one node directory.
    _lock: File,
    node_id: NodeId,
    state: DurableState,
    snapshot: Option<Snapshot>,
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

/// Write a whole file so that a crash leaves either the old file or all of the
/// new one: temporary file, sync, rename, sync the directory.
fn write_atomically(
    directory: &Path,
    name: &str,
    bytes: &[u8],
    before_rename: &str,
) -> io::Result<()> {
    let temporary = directory.join(format!("{name}.tmp"));
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    failpoint(before_rename);
    fs::rename(&temporary, directory.join(name))?;
    sync_directory(directory)
}

fn encode_snapshot(snapshot: &Snapshot) -> Vec<u8> {
    let mut out = vec![0; SNAPSHOT_HEADER_LEN];
    out[..4].copy_from_slice(SNAPSHOT_MAGIC);
    out[4..6].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    out[6..8].copy_from_slice(&(SNAPSHOT_HEADER_LEN as u16).to_le_bytes());
    out[8..8 + CLUSTER.len()].copy_from_slice(CLUSTER.as_bytes());
    out[32..40].copy_from_slice(&snapshot.base.index.to_le_bytes());
    out[40..48].copy_from_slice(&snapshot.base.term.to_le_bytes());
    out[48..52].copy_from_slice(&snapshot.voters.to_le_bytes());
    out[52..56].copy_from_slice(&(snapshot.data.len() as u32).to_le_bytes());
    out[56..60].copy_from_slice(&snapshot.crc().to_le_bytes());
    let checksum = crc32c(&out[..60]);
    out[60..64].copy_from_slice(&checksum.to_le_bytes());
    out.extend_from_slice(&snapshot.data);
    out
}

fn decode_snapshot(bytes: &[u8]) -> io::Result<Snapshot> {
    if bytes.len() < SNAPSHOT_HEADER_LEN {
        return Err(invalid("snapshot is shorter than its header"));
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    let long = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"));
    if crc32c(&bytes[..60]) != word(60) {
        return Err(invalid("snapshot header checksum mismatch"));
    }
    let mut cluster = [0_u8; 24];
    cluster[..CLUSTER.len()].copy_from_slice(CLUSTER.as_bytes());
    if &bytes[..4] != SNAPSHOT_MAGIC
        || bytes[4..6] != SNAPSHOT_VERSION.to_le_bytes()
        || bytes[6..8] != (SNAPSHOT_HEADER_LEN as u16).to_le_bytes()
        || bytes[8..32] != cluster
    {
        return Err(invalid("not a snapshot of this cluster and version"));
    }
    let length = word(52) as usize;
    if length == 0 || length > MAX_SNAPSHOT || bytes.len() != SNAPSHOT_HEADER_LEN + length {
        return Err(invalid("snapshot length does not match its header"));
    }
    let data = &bytes[SNAPSHOT_HEADER_LEN..];
    if crc32c(data) != word(56) {
        return Err(invalid("snapshot payload checksum mismatch"));
    }
    let snapshot = Snapshot {
        base: LogBase {
            index: long(32),
            term: long(40),
        },
        voters: word(48),
        data: data.into(),
    };
    if !snapshot.valid() {
        return Err(invalid("snapshot position or membership is invalid"));
    }
    Ok(snapshot)
}

fn read_snapshot(directory: &Path) -> io::Result<Option<Snapshot>> {
    let path = directory.join("snapshot");
    let length = match fs::metadata(&path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if length > (SNAPSHOT_HEADER_LEN + MAX_SNAPSHOT) as u64 {
        return Err(invalid("snapshot file exceeds the size limit"));
    }
    decode_snapshot(&fs::read(path)?).map(Some)
}

/// The bytes of a WAL that starts right after `state.base()` and holds its
/// remaining entries, split so that no record exceeds the payload bound.
fn wal_image(
    node_id: NodeId,
    state: &DurableState,
    first_sequence: u64,
) -> io::Result<(Vec<u8>, u64)> {
    let mut out = file_header(node_id, FileStart::of(state, first_sequence)).to_vec();
    let mut sequence = first_sequence - 1;
    let mut pending: Vec<LogEntry> = vec![];
    let mut flush = |pending: &mut Vec<LogEntry>, out: &mut Vec<u8>| -> io::Result<()> {
        if pending.is_empty() {
            return Ok(());
        }
        let transaction = Transaction {
            term: state.term,
            voted_for: state.voted_for,
            truncate_from: pending[0].index,
            append: std::mem::take(pending),
        };
        let payload = serde_json::to_vec(&transaction).map_err(io::Error::other)?;
        if payload.len() > MAX_RECORD_PAYLOAD {
            return Err(invalid(
                "a single log entry exceeds the record payload bound",
            ));
        }
        sequence += 1;
        let mut record = record_header(sequence, payload.len()).to_vec();
        record.extend_from_slice(&payload);
        record.extend_from_slice(&crc32c(&record).to_le_bytes());
        out.extend_from_slice(&record);
        Ok(())
    };
    let mut size = 0;
    for entry in state.entries() {
        let entry_size = serde_json::to_vec(entry).map_err(io::Error::other)?.len() + 1;
        // 200 bytes covers the transaction's own fields.
        if size + entry_size + 200 > MAX_RECORD_PAYLOAD {
            flush(&mut pending, &mut out)?;
            size = 0;
        }
        size += entry_size;
        pending.push(entry.clone());
    }
    flush(&mut pending, &mut out)?;
    Ok((out, sequence))
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
    let start = previous.base().index.max(next.base().index) + 1;
    let end = previous.last_index().min(next.last_index());
    for (old, new) in (start..=end).filter_map(|i| Some((previous.entry(i)?, next.entry(i)?))) {
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
    if transaction.truncate_from <= previous.base().index
        || transaction.truncate_from > previous.last_index() + 1
    {
        return Err(invalid("transaction truncation index outside current log"));
    }
    let mut next = previous.clone();
    next.term = transaction.term;
    next.voted_for = transaction.voted_for;
    next.truncate_from(transaction.truncate_from);
    for entry in transaction.append {
        next.push(entry);
    }
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

fn check_file_header(bytes: &[u8; FILE_HEADER_LEN], node_id: NodeId) -> io::Result<FileStart> {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    let long = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"));
    if crc32c(&bytes[..76]) != word(76) {
        return Err(invalid("WAL file header checksum mismatch"));
    }
    let expected = file_header(node_id, PRISTINE);
    if bytes[..8] != expected[..8] {
        return Err(invalid("not a format 2 WAL file header"));
    }
    if bytes[8..CLUSTER_FIELD.end] != expected[8..CLUSTER_FIELD.end] {
        return Err(invalid("WAL file header names another node or cluster"));
    }
    let start = FileStart {
        base_index: long(40),
        base_term: long(48),
        start_term: long(56),
        first_sequence: long(64),
        start_voted_for: word(72),
    };
    let consistent = bytes[36..40] == [0; 4]
        && (start.base_index == 0) == (start.base_term == 0)
        && start.base_term <= start.start_term
        && start.first_sequence >= 1
        && (start.start_voted_for == 0
            || (start.start_term > 0
                && NodeId::try_from(start.start_voted_for).is_ok_and(valid_node)));
    if !consistent {
        return Err(invalid("WAL file header start fields are inconsistent"));
    }
    Ok(start)
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
    write_atomically(
        directory,
        "node.meta",
        &bytes,
        "init_before_rename_node.meta",
    )
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
        // A snapshot is written only after initialization finished.
        if directory.join("snapshot").exists() && !metadata_path.exists() {
            return Err(invalid("snapshot exists without identity metadata"));
        }
        // Left by an install interrupted before its rename; never data.
        for leftover in ["snapshot.tmp", "raft.wal.tmp"] {
            if metadata_path.exists() && directory.join(leftover).exists() {
                fs::remove_file(directory.join(leftover))?;
            }
        }
        // First initialization writes the WAL, then the identity. Identity is
        // therefore the proof that initialization finished: with it, a missing
        // WAL is data loss; without it, only an untouched header is acceptable.
        match (metadata_path.exists(), wal_path.exists()) {
            (false, false) => {
                write_atomically(
                    &directory,
                    "raft.wal",
                    &file_header(node_id, PRISTINE),
                    "init_before_rename_raft.wal",
                )?;
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
        let start = check_file_header(&header, node_id)?;
        let initial = start.state();
        if !initial.valid() {
            return Err(invalid(
                "WAL file header describes an invalid starting state",
            ));
        }
        let mut wal = Self {
            file,
            _lock: lock,
            node_id,
            state: initial,
            snapshot: None,
            sequence: start.first_sequence - 1,
            poisoned: false,
            fail_sync: false,
            directory,
            last_sync_us: 0.0,
        };
        wal.recover()?;
        wal.reconcile_snapshot()?;
        Ok(wal)
    }

    /// Make the snapshot file and the WAL agree (docs/SNAPSHOT_COMPACTION.md §7.3).
    fn reconcile_snapshot(&mut self) -> io::Result<()> {
        let snapshot = read_snapshot(&self.directory)?;
        let base = self.state.base();
        match &snapshot {
            None if base.index > 0 => {
                return Err(invalid(
                    "WAL starts after a snapshot but the snapshot file is missing",
                ));
            }
            None => {}
            Some(found) if base.index > found.base.index => {
                return Err(invalid("WAL starts after the snapshot it should follow"));
            }
            Some(found) if base.index == found.base.index => {
                if base.term != found.base.term {
                    return Err(invalid("WAL base term differs from the snapshot"));
                }
            }
            Some(found) => {
                // The snapshot became durable but the WAL was not rewritten yet:
                // finish the job. Everything the snapshot covers is committed.
                let mut next = adopt_snapshot(&self.state, found.base);
                // The interrupted install may also have carried a newer term: a
                // follower that learns the term from a one-message snapshot
                // raises it in the same barrier (lesson 11). The snapshot proves
                // that its term existed, and this node holds no durable vote in
                // it, so adopting the term without a vote is safe.
                if next.term < found.base.term {
                    next.term = found.base.term;
                    next.voted_for = None;
                }
                if !next.valid() {
                    return Err(invalid("snapshot and WAL cannot be reconciled"));
                }
                self.rewrite_wal(&next)?;
            }
        }
        self.snapshot = snapshot;
        Ok(())
    }

    /// Replace the WAL with one that starts at `next.base()` (§7.4 steps 2-3).
    fn rewrite_wal(&mut self, next: &DurableState) -> io::Result<()> {
        let first_sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("WAL sequence exhausted"))?;
        let (image, last_sequence) = wal_image(self.node_id, next, first_sequence)?;
        if self.fail_sync {
            return Err(io::Error::other("injected sync failure"));
        }
        write_atomically(
            &self.directory,
            "raft.wal",
            &image,
            "install_before_wal_rename",
        )?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.directory.join("raft.wal"))?;
        file.seek(SeekFrom::End(0))?;
        self.file = file;
        self.state = next.clone();
        self.sequence = last_sequence;
        Ok(())
    }

    /// Persist a snapshot and drop the log it covers (§7.4). `next` must be the
    /// state after adopting `snapshot`; nothing is acknowledged until this returns.
    pub fn install(&mut self, next: &DurableState, snapshot: &Snapshot) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(
                "WAL is poisoned; restart and recover before use",
            ));
        }
        if !snapshot.valid()
            || next.base() != snapshot.base
            || snapshot.base.index <= self.state.base().index
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "snapshot must move the log base forward and match the new state",
            ));
        }
        validate_change(&self.state, next)?;
        let result = (|| {
            if self.fail_sync {
                return Err(io::Error::other("injected sync failure"));
            }
            write_atomically(
                &self.directory,
                "snapshot",
                &encode_snapshot(snapshot),
                "install_before_snapshot_rename",
            )?;
            failpoint("install_after_snapshot_before_wal");
            self.rewrite_wal(next)
        })();
        if let Err(error) = result {
            self.poisoned = true;
            return Err(error);
        }
        self.snapshot = Some(snapshot.clone());
        failpoint("install_after_wal_rename");
        Ok(())
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }
    /// Current size of `raft.wal`, used to decide when to take a snapshot.
    pub fn wal_bytes(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
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
        if next.base() != self.state.base() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a new log base must be installed with its snapshot, not saved",
            ));
        }
        let common = self
            .state
            .entries()
            .iter()
            .zip(next.entries())
            .take_while(|(left, right)| left == right)
            .count();
        let transaction = Transaction {
            term: next.term,
            voted_for: next.voted_for,
            truncate_from: self.state.base().index + common as u64 + 1,
            append: next.entries()[common..].to_vec(),
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
    // Write, then rename: a test that sees the marker exist must also see its
    // whole content. Creating it in place let a test read "" in between
    // (docs/lessons/05-marker-seen-before-written/).
    if let Ok(marker) = std::env::var("RAFT_WAL_FAILPOINT_MARKER") {
        let temporary = format!("{marker}.tmp");
        if let Ok(mut file) = File::create(&temporary) {
            let _ = file.write_all(name.as_bytes());
            let _ = file.sync_all();
            marker_failpoint_gap();
            let _ = fs::rename(&temporary, &marker);
        }
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// Lesson 05 widens the gap between writing the marker and publishing it, to
/// show that a reader can never observe it half-written. Unset otherwise.
fn marker_failpoint_gap() {
    if let Some(millis) = std::env::var("RAFT_WAL_MARKER_GAP_MS")
        .ok()
        .and_then(|value| value.parse().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(millis));
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

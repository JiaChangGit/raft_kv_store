//! Storage tests. Byte offsets are written out literally where a test pins the
//! on-disk layout, so that a wrong encoder cannot agree with itself.
use super::*;
use kv_state::StateMachine;
use raft_core::Core;
use raft_protocol::{ClientRequest, Command, Operation, Voters};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
/// A real WAL and identity written by the format 1 implementation on 2026-11-10.
const V1_META: &[u8] = include_bytes!("../tests/fixtures/v1/node.meta");
const V1_WAL: &[u8] = include_bytes!("../tests/fixtures/v1/raft.wal");

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
    fn wal(&self) -> PathBuf {
        self.0.join("raft.wal")
    }
    fn meta(&self) -> PathBuf {
        self.0.join("node.meta")
    }
    fn names(&self, sub: &str) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(self.0.join(sub))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn state(term: u64, voted_for: Option<u8>) -> DurableState {
    DurableState::new(term, voted_for, vec![])
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

/// A WAL produced by the real writer: three records, one log entry each.
struct Sample {
    bytes: Vec<u8>,
    /// Start of each record, then the end of the file.
    offsets: Vec<usize>,
    /// Durable state after each record.
    states: Vec<DurableState>,
}
fn sample(dir: &TestDir) -> Sample {
    let mut states = vec![];
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        let mut next = state(2, Some(1));
        for (index, value) in ["A", "B", "C"].into_iter().enumerate() {
            next.push(entry(index as u64 + 1, 2, value));
            wal.save(&next).unwrap();
            states.push(next.clone());
        }
    }
    let bytes = fs::read(dir.wal()).unwrap();
    let mut offsets = vec![80];
    while *offsets.last().unwrap() < bytes.len() {
        let at = *offsets.last().unwrap();
        let payload = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        offsets.push(at + 24 + payload + 4);
    }
    assert_eq!(offsets.len(), 4, "three records and the end of file");
    Sample {
        bytes,
        offsets,
        states,
    }
}
/// Replace the WAL with `bytes` and try to open the directory.
fn outcome(dir: &TestDir, bytes: &[u8]) -> Result<DurableState, String> {
    fs::write(dir.wal(), bytes).unwrap();
    Wal::open(&dir.0, 1)
        .map(|wal| wal.state().clone())
        .map_err(|error| error.to_string())
}
/// A refusal must leave the WAL byte-identical and must not set anything aside.
fn untouched(dir: &TestDir, bytes: &[u8]) -> bool {
    fs::read(dir.wal()).unwrap() == bytes && !dir.0.join("torn").exists()
}
fn torn_name(sequence: u64, tail: &[u8]) -> String {
    format!("seq{sequence:020}-{:08x}.bin", crc32c(tail))
}
fn reseal_file_header(bytes: &mut [u8]) {
    let checksum = crc32c(&bytes[..76]);
    bytes[76..80].copy_from_slice(&checksum.to_le_bytes());
}
fn reseal_record_header(bytes: &mut [u8], at: usize) {
    let checksum = crc32c(&bytes[at..at + 20]);
    bytes[at + 20..at + 24].copy_from_slice(&checksum.to_le_bytes());
}
/// Make both checksums of the record in `at..end` valid for its current bytes.
fn reseal_record(bytes: &mut [u8], at: usize, end: usize) {
    reseal_record_header(bytes, at);
    let checksum = crc32c(&bytes[at..end - 4]);
    bytes[end - 4..end].copy_from_slice(&checksum.to_le_bytes());
}

#[test]
fn crc32c_has_castagnoli_known_vector() {
    assert_eq!(crc32c(b"123456789"), 0xe3069283);
    assert_eq!(crc32c(b""), 0);
}

#[test]
fn new_directory_layout_and_file_header() {
    let dir = TestDir::new();
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(wal.state(), &DurableState::default());
        assert_eq!(wal.durable_sequence(), 0);
        assert_eq!(fs::metadata(dir.wal()).unwrap().len(), 80);
        wal.save(&state(7, Some(3))).unwrap();
    }
    let identity: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.meta()).unwrap()).unwrap();
    assert_eq!(
        identity,
        serde_json::json!({"cluster_id": "raft-kv-v1", "node_id": 1, "format_version": 2})
    );
    assert_eq!(dir.names(""), ["node.lock", "node.meta", "raft.wal"]);

    let bytes = fs::read(dir.wal()).unwrap();
    assert_eq!(&bytes[0..4], b"RFTW");
    assert_eq!(bytes[4..6], [2, 0], "format version");
    assert_eq!(bytes[6..8], [80, 0], "file header length");
    assert_eq!(bytes[8..12], [1, 0, 0, 0], "node id");
    assert_eq!(&bytes[12..22], b"raft-kv-v1");
    assert!(
        bytes[22..64].iter().all(|byte| *byte == 0),
        "cluster padding, reserved bytes, base index, base term and start term"
    );
    assert_eq!(bytes[64..72], 1_u64.to_le_bytes(), "first sequence");
    assert_eq!(bytes[72..76], [0; 4], "no starting vote");
    assert_eq!(bytes[76..80], crc32c(&bytes[..76]).to_le_bytes());

    let payload = br#"{"term":7,"voted_for":3,"truncate_from":1,"append":[]}"#;
    let record = &bytes[80..];
    assert_eq!(record.len(), 24 + payload.len() + 4);
    assert_eq!(&record[0..4], b"RFTR");
    assert_eq!(record[4..6], [2, 0], "record version");
    assert_eq!(record[6..8], [1, 0], "transaction record");
    assert_eq!(record[8..12], (payload.len() as u32).to_le_bytes());
    assert_eq!(record[12..20], 1_u64.to_le_bytes(), "sequence");
    assert_eq!(record[20..24], crc32c(&record[..20]).to_le_bytes());
    assert_eq!(&record[24..24 + payload.len()], payload);
    assert_eq!(
        record[24 + payload.len()..],
        crc32c(&record[..24 + payload.len()]).to_le_bytes()
    );
}

#[test]
fn existing_identity_without_wal_fails_closed() {
    let dir = TestDir::new();
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        wal.save(&state(5, Some(2))).unwrap();
    }
    fs::rename(dir.wal(), dir.0.join("saved.wal")).unwrap();
    assert!(
        Wal::open(&dir.0, 1).is_err(),
        "must not silently reset a used node"
    );
    assert!(!dir.wal().exists());
    assert!(!dir.0.join("raft.wal.tmp").exists());
}

#[test]
fn interrupted_initialization_is_completed_or_rejected() {
    // The WAL was renamed into place but the identity was not written yet.
    let dir = TestDir::new();
    fs::write(dir.wal(), file_header(1, PRISTINE)).unwrap();
    fs::write(dir.0.join("node.meta.tmp"), b"{\"cluster").unwrap();
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(wal.state(), &DurableState::default());
        wal.save(&state(1, Some(1))).unwrap();
    }
    assert_eq!(Wal::open(&dir.0, 1).unwrap().state().term, 1);

    // Only a temporary file exists: nothing was ever initialized.
    let dir = TestDir::new();
    fs::write(dir.0.join("raft.wal.tmp"), b"partial header").unwrap();
    assert_eq!(
        Wal::open(&dir.0, 1).unwrap().state(),
        &DurableState::default()
    );
    assert_eq!(fs::metadata(dir.wal()).unwrap().len(), 80);

    // Another node's untouched WAL is not ours to adopt.
    let dir = TestDir::new();
    let foreign = file_header(2, PRISTINE);
    fs::write(dir.wal(), foreign).unwrap();
    assert!(Wal::open(&dir.0, 1).is_err());
    assert!(!dir.meta().exists());
    assert_eq!(fs::read(dir.wal()).unwrap(), foreign);

    // A WAL that already holds records but has no identity means lost metadata.
    let dir = TestDir::new();
    let sample = sample(&dir);
    fs::remove_file(dir.meta()).unwrap();
    assert!(Wal::open(&dir.0, 1).is_err());
    assert!(!dir.meta().exists());
    assert_eq!(fs::read(dir.wal()).unwrap(), sample.bytes);
}

#[test]
fn v1_directory_rejected_without_modification() {
    let dir = TestDir::new();
    fs::write(dir.meta(), V1_META).unwrap();
    fs::write(dir.wal(), V1_WAL).unwrap();
    let error = Wal::open(&dir.0, 1)
        .err()
        .expect("format 1 data must not be opened")
        .to_string();
    assert!(error.contains("format 1"), "{error}");
    assert_eq!(fs::read(dir.meta()).unwrap(), V1_META);
    assert_eq!(fs::read(dir.wal()).unwrap(), V1_WAL);
    assert_eq!(dir.names(""), ["node.lock", "node.meta", "raft.wal"]);

    // Without its identity file the old WAL is still not adopted or rewritten.
    fs::remove_file(dir.meta()).unwrap();
    assert!(Wal::open(&dir.0, 1).is_err());
    assert_eq!(fs::read(dir.wal()).unwrap(), V1_WAL);
    assert_eq!(dir.names(""), ["node.lock", "raft.wal"]);
}

#[test]
fn file_header_corruption_and_identity_mismatch_rejected() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    for short in [0, 1, 79] {
        let error = outcome(&dir, &sample.bytes[..short]).unwrap_err();
        assert!(error.contains("shorter than its file header"), "{error}");
        assert!(untouched(&dir, &sample.bytes[..short]));
    }
    let mut unsealed = sample.bytes.clone();
    unsealed[79] ^= 0x80;
    let error = outcome(&dir, &unsealed).unwrap_err();
    assert!(error.contains("file header checksum"), "{error}");
    assert!(untouched(&dir, &unsealed));

    // With a valid checksum, each field rule must reject on its own.
    for (what, index, value, expected) in [
        ("magic", 0, b'X', "not a format 2"),
        ("version", 4, 3, "not a format 2"),
        ("header length", 6, 81, "not a format 2"),
        ("node id", 8, 2, "another node or cluster"),
        ("cluster id", 12, b'X', "another node or cluster"),
        ("cluster padding", 35, 1, "another node or cluster"),
    ] {
        let mut changed = sample.bytes.clone();
        changed[index] = value;
        reseal_file_header(&mut changed);
        let error = outcome(&dir, &changed).unwrap_err();
        assert!(error.contains(expected), "{what}: {error}");
        assert!(untouched(&dir, &changed), "{what}");
    }
    fs::write(dir.wal(), &sample.bytes).unwrap();
    assert!(
        Wal::open(&dir.0, 2).is_err(),
        "node 2 cannot open node 1 data"
    );
    assert_eq!(outcome(&dir, &sample.bytes).unwrap(), sample.states[2]);
}

/// Replaces S1's `reserved_header_fields_must_be_pristine`: since S2 the start
/// fields carry a snapshot position, so they are checked for consistency.
/// Every case keeps a valid header checksum so that only the field rule refuses.
#[test]
fn file_header_start_fields_validated() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    let set = |bytes: &mut Vec<u8>, at: usize, value: &[u8]| {
        bytes[at..at + value.len()].copy_from_slice(value);
        reseal_file_header(bytes);
    };
    type Fields = Vec<(usize, Vec<u8>)>;
    let cases: [(&str, Fields); 7] = [
        ("reserved bytes", vec![(36, vec![1])]),
        (
            "base index without a term",
            vec![(40, 3_u64.to_le_bytes().to_vec())],
        ),
        (
            "base term without an index",
            vec![(48, 1_u64.to_le_bytes().to_vec())],
        ),
        (
            "base term above start term",
            vec![
                (40, 3_u64.to_le_bytes().to_vec()),
                (48, 5_u64.to_le_bytes().to_vec()),
                (56, 4_u64.to_le_bytes().to_vec()),
            ],
        ),
        ("first sequence 0", vec![(64, 0_u64.to_le_bytes().to_vec())]),
        (
            // S4: IDs 1..=7 are valid, so the first invalid one is 8.
            "vote for node 8",
            vec![(56, 1_u64.to_le_bytes().to_vec()), (72, vec![8])],
        ),
        ("vote in term 0", vec![(72, vec![2])]),
    ];
    for (what, fields) in cases {
        let mut changed = sample.bytes.clone();
        for (at, value) in fields {
            set(&mut changed, at, &value);
        }
        let error = outcome(&dir, &changed).unwrap_err();
        assert!(
            error.contains("start fields are inconsistent"),
            "{what}: {error}"
        );
        assert!(untouched(&dir, &changed), "{what}");
    }
    // A consistent start is accepted: term 5 and a vote for node 2 before any record.
    let mut header = sample.bytes[..80].to_vec();
    set(&mut header, 56, &5_u64.to_le_bytes());
    set(&mut header, 72, &[2]);
    let opened = outcome(&dir, &header).unwrap();
    assert_eq!(
        (opened.term, opened.voted_for, opened.last_index()),
        (5, Some(2), 0)
    );
    // A base without its snapshot file is data loss, not a fresh start.
    set(&mut header, 40, &3_u64.to_le_bytes());
    set(&mut header, 48, &4_u64.to_le_bytes());
    let error = outcome(&dir, &header).unwrap_err();
    assert!(error.contains("snapshot file is missing"), "{error}");
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
    next.push(entry(1, 3, "A"));
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
    later.push(entry(2, 3, "B"));
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
    persistent.push(entry(1, 2, "uncommitted"));
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        wal.save(&persistent).unwrap();
    }
    let recovered = Wal::open(&dir.0, 1).unwrap();
    let core = Core::new(1, recovered.state().clone()).unwrap();
    let kv = StateMachine::default();
    assert_eq!(core.state.last_index(), 1);
    assert_eq!(core.commit_index, 0);
    assert_eq!(core.last_applied, 0);
    assert_eq!(kv.get("x"), None);
    assert_eq!(kv.session_count(), 0);
}

#[test]
fn truncated_wal_tail_safe() {
    let dir = TestDir::new();
    let mut first = state(2, Some(1));
    first.push(entry(1, 2, "A"));
    let second = {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        wal.save(&first).unwrap();
        let mut next = first.clone();
        next.push(entry(2, 2, "B"));
        wal.save(&next).unwrap();
        next
    };
    let path = dir.wal();
    let bytes = fs::read(&path).unwrap();
    let first_end = {
        let payload = u32::from_le_bytes(bytes[80 + 8..80 + 12].try_into().unwrap()) as usize;
        80 + 24 + payload + 4
    };
    for cut in first_end + 1..bytes.len() {
        fs::write(&path, &bytes[..cut]).unwrap();
        let wal = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(wal.state(), &first, "cut={cut}");
        assert_eq!(fs::metadata(&path).unwrap().len(), first_end as u64);
        drop(wal);
        // What was cut off is set aside under its documented name, not destroyed.
        let kept = dir
            .0
            .join("torn")
            .join(torn_name(2, &bytes[first_end..cut]));
        assert_eq!(fs::read(kept).unwrap(), &bytes[first_end..cut], "cut={cut}");
    }
    assert!(dir.names("torn").len() <= MAX_TORN_COPIES);
    fs::write(&path, &bytes).unwrap();
    {
        let wal = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(wal.state(), &second);
    }
    let mut corrupt = bytes.clone();
    corrupt[80 + 24 + 1] ^= 1;
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
    let initial = DurableState::new(2, Some(1), vec![entry(1, 1, "A"), entry(2, 2, "B")]);
    let replacement = {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        assert!(Wal::open(&dir.0, 1).is_err());
        wal.save(&initial).unwrap();
        let mut next = initial.clone();
        next.term = 3;
        next.voted_for = None;
        next.truncate_from(2);
        next.push(entry(2, 3, "C"));
        next.push(entry(3, 3, "D"));
        wal.save(&next).unwrap();
        next
    };
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(wal.state(), &replacement);
        let mut wrong = replacement.clone();
        // Same index and term as the durable entry 2, different command.
        wrong.truncate_from(2);
        wrong.push(entry(2, 3, "E"));
        wrong.push(entry(3, 3, "D"));
        assert!(wal.save(&wrong).is_err());
    }
    assert!(Wal::open(&dir.0, 2).is_err());
}

/// The two corruptions reproduced against format 1 on 2026-11-12, where both
/// were mistaken for an unfinished tail and the log was cut.
#[test]
fn reproduced_length_corruption_fails_closed_unchanged() {
    let dir = TestDir::new();
    let sample = sample(&dir);

    // Case A: the first record's length now points far past the end of the file.
    let first = sample.offsets[0];
    let mut case_a = sample.bytes.clone();
    case_a[first + 8..first + 12].copy_from_slice(&65_536_u32.to_le_bytes());
    let error = outcome(&dir, &case_a).unwrap_err();
    assert!(error.contains("header checksum"), "{error}");
    assert!(untouched(&dir, &case_a));

    // Case B: the last, already durable record claims one more byte than exists.
    let last = sample.offsets[2];
    let length = u32::from_le_bytes(sample.bytes[last + 8..last + 12].try_into().unwrap());
    let mut case_b = sample.bytes.clone();
    case_b[last + 8..last + 12].copy_from_slice(&(length + 1).to_le_bytes());
    let error = outcome(&dir, &case_b).unwrap_err();
    assert!(error.contains("header checksum"), "{error}");
    assert!(untouched(&dir, &case_b));

    assert_eq!(outcome(&dir, &sample.bytes).unwrap(), sample.states[2]);
}

#[test]
fn every_single_bit_flip_fails_closed_unchanged() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    let mut cases = 0;
    for index in 0..sample.bytes.len() {
        for bit in 0..8 {
            let mut corrupt = sample.bytes.clone();
            corrupt[index] ^= 1 << bit;
            assert!(
                outcome(&dir, &corrupt).is_err(),
                "byte {index} bit {bit} was accepted"
            );
            assert!(untouched(&dir, &corrupt), "byte {index} bit {bit}");
            cases += 1;
        }
    }
    assert_eq!(cases, sample.bytes.len() * 8);
    assert!(sample.bytes.len() > 80 + 3 * (24 + 4));
    assert_eq!(outcome(&dir, &sample.bytes).unwrap(), sample.states[2]);
}

#[test]
fn header_field_rules_checked_after_crc() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    let (second, end) = (sample.offsets[1], sample.offsets[2]);
    // Both checksums are made valid again, so only the field rule can refuse.
    let cases: Vec<(&str, usize, Vec<u8>, &str)> = vec![
        (
            "format 1 magic",
            0,
            b"RFT1".to_vec(),
            "magic, version or type",
        ),
        ("version 1", 4, vec![1, 0], "magic, version or type"),
        ("version 3", 4, vec![3, 0], "magic, version or type"),
        ("record type 2", 6, vec![2, 0], "magic, version or type"),
        ("sequence 0", 12, 0_u64.to_le_bytes().to_vec(), "sequence"),
        (
            "repeated sequence",
            12,
            1_u64.to_le_bytes().to_vec(),
            "sequence",
        ),
        (
            "skipped sequence",
            12,
            3_u64.to_le_bytes().to_vec(),
            "sequence",
        ),
    ];
    for (what, field, value, expected) in cases {
        let mut changed = sample.bytes.clone();
        changed[second + field..second + field + value.len()].copy_from_slice(&value);
        reseal_record(&mut changed, second, end);
        let error = outcome(&dir, &changed).unwrap_err();
        assert!(error.contains(expected), "{what}: {error}");
        assert!(untouched(&dir, &changed), "{what}");
    }
    for length in [0_u32, MAX_RECORD_PAYLOAD as u32 + 1, u32::MAX] {
        let mut changed = sample.bytes.clone();
        changed[second + 8..second + 12].copy_from_slice(&length.to_le_bytes());
        reseal_record_header(&mut changed, second);
        let error = outcome(&dir, &changed).unwrap_err();
        assert!(error.contains("length out of bounds"), "{length}: {error}");
        assert!(untouched(&dir, &changed), "{length}");
    }
}

#[test]
fn seeded_multibyte_corruption_never_accepts_or_shortens() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    let mut random = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move || {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        random
    };
    for case in 0..10_000 {
        let mut corrupt = sample.bytes.clone();
        let start = (next() % corrupt.len() as u64) as usize;
        let span = 1 + (next() % 64) as usize;
        for byte in corrupt.iter_mut().skip(start).take(span) {
            // XOR with 1..=255 always changes the byte.
            *byte ^= 1 + (next() % 255) as u8;
        }
        assert!(
            outcome(&dir, &corrupt).is_err(),
            "case {case}: {span} bytes from {start} were accepted"
        );
        assert!(untouched(&dir, &corrupt), "case {case}");
    }
}

#[test]
fn zero_or_garbage_tail_rejected() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    for length in [1, 7, 12, 23, 24, 25, 4096] {
        let mut zeros = sample.bytes.clone();
        zeros.resize(zeros.len() + length, 0);
        assert!(outcome(&dir, &zeros).is_err(), "{length} zero bytes");
        assert!(untouched(&dir, &zeros), "{length} zero bytes");

        let mut garbage = sample.bytes.clone();
        garbage.extend((0..length).map(|i| 0xa5 ^ (i as u8).wrapping_mul(31)));
        assert!(outcome(&dir, &garbage).is_err(), "{length} garbage bytes");
        assert!(untouched(&dir, &garbage), "{length} garbage bytes");
    }
    // Starting like a record is not enough once a checkable byte disagrees.
    let mut lookalike = sample.bytes.clone();
    lookalike.extend_from_slice(b"RFTR\x02\x00\x09");
    assert!(outcome(&dir, &lookalike).is_err());
    assert!(untouched(&dir, &lookalike));
}

#[test]
fn partial_header_tail_checks_every_available_byte() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    let prefix = &sample.bytes[..sample.offsets[2]];
    let header = &sample.bytes[sample.offsets[2]..sample.offsets[2] + 24];
    for cut in 1..24 {
        let mut torn = prefix.to_vec();
        torn.extend_from_slice(&header[..cut]);
        assert_eq!(
            outcome(&dir, &torn).unwrap(),
            sample.states[1],
            "{cut} genuine header bytes are an unfinished record"
        );
        assert_eq!(fs::read(dir.wal()).unwrap(), prefix);
        assert_eq!(dir.names("torn"), [torn_name(3, &header[..cut])]);
        fs::remove_dir_all(dir.0.join("torn")).unwrap();

        for index in 0..cut {
            // The length cannot be predicted; magic, version, type, sequence and
            // any checksum byte that is present can.
            if (8..12).contains(&index) {
                continue;
            }
            for bit in 0..8 {
                let mut corrupt = torn.clone();
                corrupt[prefix.len() + index] ^= 1 << bit;
                assert!(
                    outcome(&dir, &corrupt).is_err(),
                    "cut {cut} byte {index} bit {bit} was accepted"
                );
                assert!(untouched(&dir, &corrupt), "cut {cut} byte {index}");
            }
        }
    }
    for length in [0_u32, MAX_RECORD_PAYLOAD as u32 + 1] {
        let mut torn = prefix.to_vec();
        torn.extend_from_slice(&header[..16]);
        torn[prefix.len() + 8..prefix.len() + 12].copy_from_slice(&length.to_le_bytes());
        assert!(
            outcome(&dir, &torn).is_err(),
            "partial header length {length}"
        );
        assert!(untouched(&dir, &torn));
    }
}

#[test]
fn torn_tail_copy_failure_blocks_truncation() {
    for inside in [5, 24 + 5] {
        let dir = TestDir::new();
        let sample = sample(&dir);
        let start = sample.offsets[2];
        let torn = &sample.bytes[..start + inside];
        fs::write(dir.wal(), torn).unwrap();
        // The copy cannot be made: `torn` is occupied by a regular file.
        fs::write(dir.0.join("torn"), b"occupied").unwrap();
        assert!(Wal::open(&dir.0, 1).is_err());
        assert_eq!(
            fs::read(dir.wal()).unwrap(),
            torn,
            "nothing may be cut off before its copy is safe"
        );
        fs::remove_file(dir.0.join("torn")).unwrap();
        assert_eq!(Wal::open(&dir.0, 1).unwrap().state(), &sample.states[1]);
        assert_eq!(fs::read(dir.wal()).unwrap(), &sample.bytes[..start]);
        let name = torn_name(3, &sample.bytes[start..start + inside]);
        assert_eq!(dir.names("torn"), [name.as_str()]);
        assert_eq!(
            fs::read(dir.0.join("torn").join(name)).unwrap(),
            &sample.bytes[start..start + inside]
        );
    }
}

#[test]
fn torn_tail_copies_are_idempotent_and_bounded() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    let start = sample.offsets[2];
    for _ in 0..2 {
        assert_eq!(
            outcome(&dir, &sample.bytes[..start + 30]).unwrap(),
            sample.states[1]
        );
    }
    assert_eq!(dir.names("torn").len(), 1, "the same tail is kept once");

    let stale = "seq00000000000000000003-deadbeef.bin.tmp";
    fs::write(dir.0.join("torn").join("note.txt"), b"not ours").unwrap();
    fs::write(dir.0.join("torn").join(stale), b"interrupted copy").unwrap();
    for extra in 1..=12 {
        outcome(&dir, &sample.bytes[..start + 30 + extra]).unwrap();
    }
    let names = dir.names("torn");
    assert_eq!(
        names.iter().filter(|name| torn_copy_name(name)).count(),
        MAX_TORN_COPIES
    );
    assert!(names.contains(&"note.txt".to_string()));
    assert!(!names.contains(&stale.to_string()));
    assert!(
        names.contains(&torn_name(3, &sample.bytes[start..start + 42])),
        "the newest copy is never the one discarded"
    );
}

/// Pins a documented non-guarantee: without an external anchor, a file that
/// ends exactly between two records looks like a shorter, valid log.
#[test]
fn boundary_truncation_is_documented_as_undetected() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    assert_eq!(
        outcome(&dir, &sample.bytes[..sample.offsets[1]]).unwrap(),
        sample.states[0]
    );
    assert_eq!(
        outcome(&dir, &sample.bytes[..80]).unwrap(),
        DurableState::default()
    );
    assert!(!dir.0.join("torn").exists());
}

// ---- S2: snapshots ------------------------------------------------------------

fn snap(index: u64, term: u64, text: &str) -> Snapshot {
    let voters = Voters::from_ids(&[1, 2, 3]).unwrap();
    Snapshot::new(LogBase { index, term }, voters, text.as_bytes().to_vec())
}

/// Five entries in term 2, then a snapshot covering the first three.
fn installed(dir: &TestDir) -> (DurableState, Snapshot) {
    let mut wal = Wal::open(&dir.0, 1).unwrap();
    let mut log = state(2, Some(1));
    for (index, value) in ["A", "B", "C", "D", "E"].into_iter().enumerate() {
        log.push(entry(index as u64 + 1, 2, value));
    }
    wal.save(&log).unwrap();
    let snapshot = snap(3, 2, "state-at-3");
    let next = adopt_snapshot(&log, snapshot.base);
    wal.install(&next, &snapshot).unwrap();
    assert_eq!(wal.state(), &next);
    assert_eq!(wal.snapshot(), Some(&snapshot));
    (next, snapshot)
}

#[test]
fn snapshot_file_layout_is_pinned() {
    let dir = TestDir::new();
    let (_, snapshot) = installed(&dir);
    let bytes = fs::read(dir.0.join("snapshot")).unwrap();
    assert_eq!(&bytes[0..4], b"RFTS");
    assert_eq!(bytes[4..6], [1, 0], "snapshot format version");
    assert_eq!(bytes[6..8], [64, 0], "header length");
    assert_eq!(&bytes[8..18], b"raft-kv-v1");
    assert!(bytes[18..32].iter().all(|byte| *byte == 0));
    assert_eq!(bytes[32..40], 3_u64.to_le_bytes(), "last included index");
    assert_eq!(bytes[40..48], 2_u64.to_le_bytes(), "last included term");
    assert_eq!(bytes[48..52], 0b1110_u32.to_le_bytes(), "voters 1, 2, 3");
    assert_eq!(bytes[52..56], 10_u32.to_le_bytes(), "payload length");
    assert_eq!(bytes[56..60], crc32c(b"state-at-3").to_le_bytes());
    assert_eq!(bytes[60..64], crc32c(&bytes[..60]).to_le_bytes());
    assert_eq!(&bytes[64..], b"state-at-3");
    assert_eq!(decode_snapshot(&bytes).unwrap(), snapshot);

    // The WAL now starts after the snapshot and holds entries 4 and 5.
    let wal = fs::read(dir.wal()).unwrap();
    assert_eq!(wal[40..48], 3_u64.to_le_bytes(), "base index");
    assert_eq!(wal[48..56], 2_u64.to_le_bytes(), "base term");
    assert_eq!(wal[56..64], 2_u64.to_le_bytes(), "start term");
    assert_eq!(
        wal[64..72],
        2_u64.to_le_bytes(),
        "first sequence follows the old one"
    );
    assert_eq!(wal[72..76], 1_u32.to_le_bytes(), "start vote");
    assert_eq!(
        dir.names(""),
        ["node.lock", "node.meta", "raft.wal", "snapshot"]
    );
}

#[test]
fn install_then_reopen_keeps_base_suffix_and_sequence() {
    let dir = TestDir::new();
    let (next, snapshot) = installed(&dir);
    let reopened = {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(wal.state(), &next);
        assert_eq!(wal.snapshot(), Some(&snapshot));
        assert_eq!(
            wal.durable_sequence(),
            2,
            "one record carries entries 4 and 5"
        );
        assert!(
            Core::new(1, wal.state().clone()).is_err(),
            "a compacted log cannot start without its snapshot"
        );
        let core = Core::restore(1, wal.state().clone(), wal.snapshot().cloned()).unwrap();
        assert_eq!((core.commit_index, core.last_applied), (3, 3));
        let mut later = next.clone();
        later.term = 3;
        later.voted_for = None;
        later.truncate_from(5);
        later.push(entry(5, 3, "F"));
        wal.save(&later).unwrap();
        later
    };
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(wal.state(), &reopened);
    assert_eq!(wal.durable_sequence(), 3);
}

#[test]
fn install_rejects_invalid_requests_without_writing() {
    let dir = TestDir::new();
    let (next, snapshot) = installed(&dir);
    let before = (
        fs::read(dir.wal()).unwrap(),
        fs::read(dir.0.join("snapshot")).unwrap(),
    );
    let mut wal = Wal::open(&dir.0, 1).unwrap();
    let older = snap(2, 2, "older");
    let same = snapshot.clone();
    let mismatched = snap(4, 2, "four");
    let mut other_vote = adopt_snapshot(&next, LogBase { index: 4, term: 2 });
    other_vote.voted_for = Some(3);
    // S4: any non-empty set of nodes 1..=7 is a configuration; these are not.
    let members = |voters: u32| {
        let mut snapshot = snap(4, 2, "members");
        snapshot.voters = voters;
        snapshot
    };
    for (what, state, snapshot) in [
        ("older base", adopt_snapshot(&next, older.base), older),
        ("same base", next.clone(), same),
        (
            "state does not match the snapshot",
            next.clone(),
            mismatched.clone(),
        ),
        (
            "vote changed within the term",
            other_vote,
            snap(4, 2, "vote"),
        ),
        (
            "empty membership",
            adopt_snapshot(&next, members(0).base),
            members(0),
        ),
        (
            "membership naming node 0",
            adopt_snapshot(&next, members(0b1).base),
            members(0b1),
        ),
        (
            "membership naming node 8",
            adopt_snapshot(&next, members(1 << 8).base),
            members(1 << 8),
        ),
    ] {
        assert!(wal.install(&state, &snapshot).is_err(), "{what}");
        assert!(
            !wal.poisoned(),
            "{what}: a refused request is not an I/O failure"
        );
    }
    assert!(
        wal.save(&adopt_snapshot(&next, mismatched.base)).is_err(),
        "a base change cannot be saved as a transaction"
    );
    drop(wal);
    assert_eq!(fs::read(dir.wal()).unwrap(), before.0);
    assert_eq!(fs::read(dir.0.join("snapshot")).unwrap(), before.1);
}

#[test]
fn install_sync_failure_poisons_and_keeps_the_old_state() {
    let dir = TestDir::new();
    let (next, snapshot) = installed(&dir);
    let mut wal = Wal::open(&dir.0, 1).unwrap();
    wal.inject_sync_failure(true);
    let newer = snap(5, 2, "state-at-5");
    assert!(
        wal.install(&adopt_snapshot(&next, newer.base), &newer)
            .is_err()
    );
    assert!(wal.poisoned());
    drop(wal);
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!((wal.state(), wal.snapshot()), (&next, Some(&snapshot)));
}

#[test]
fn snapshot_corruption_fails_closed_unchanged() {
    let dir = TestDir::new();
    installed(&dir);
    let path = dir.0.join("snapshot");
    let good = fs::read(&path).unwrap();
    let wal = fs::read(dir.wal()).unwrap();
    let refused = |bytes: &[u8]| {
        fs::write(&path, bytes).unwrap();
        let error = Wal::open(&dir.0, 1).err();
        error.is_some() && fs::read(&path).unwrap() == bytes && fs::read(dir.wal()).unwrap() == wal
    };
    let mut cases = 0;
    for index in 0..good.len() {
        for bit in 0..8 {
            let mut corrupt = good.clone();
            corrupt[index] ^= 1 << bit;
            assert!(refused(&corrupt), "byte {index} bit {bit}");
            cases += 1;
        }
    }
    assert_eq!(cases, good.len() * 8);
    assert!(refused(&good[..good.len() - 1]), "truncated payload");
    assert!(refused(&good[..63]), "truncated header");
    assert!(refused(&[good.as_slice(), b"x"].concat()), "trailing byte");
    let mut random = 0x2545_f491_4f6c_dd1d_u64;
    for case in 0..2_000 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let mut corrupt = good.clone();
        let at = (random % good.len() as u64) as usize;
        for byte in corrupt
            .iter_mut()
            .skip(at)
            .take(1 + (random >> 32) as usize % 16)
        {
            *byte ^= 1 + (random >> 40) as u8 % 255;
        }
        assert!(refused(&corrupt), "case {case}");
    }
    fs::write(&path, &good).unwrap();
    assert!(Wal::open(&dir.0, 1).is_ok());
}

/// docs/SNAPSHOT_COMPACTION.md §7.3, one row at a time.
#[test]
fn snapshot_and_wal_consistency_rules() {
    let write = |dir: &TestDir, snapshot: &Snapshot| {
        fs::write(dir.0.join("snapshot"), encode_snapshot(snapshot)).unwrap()
    };
    let refused_unchanged = |dir: &TestDir, expected: &str| {
        let files = (
            fs::read(dir.wal()).unwrap(),
            fs::read(dir.0.join("snapshot")).ok(),
        );
        let error = Wal::open(&dir.0, 1).err().expect("must refuse").to_string();
        assert!(error.contains(expected), "{error}");
        assert_eq!(
            (
                fs::read(dir.wal()).unwrap(),
                fs::read(dir.0.join("snapshot")).ok()
            ),
            files
        );
    };

    // No snapshot, B > 0: the snapshot file was lost.
    let dir = TestDir::new();
    installed(&dir);
    fs::remove_file(dir.0.join("snapshot")).unwrap();
    refused_unchanged(&dir, "snapshot file is missing");

    // S == B with a different term.
    let dir = TestDir::new();
    installed(&dir);
    write(&dir, &snap(3, 1, "state-at-3"));
    refused_unchanged(&dir, "base term differs");

    // B > S: the snapshot is older than the WAL's start.
    let dir = TestDir::new();
    installed(&dir);
    write(&dir, &snap(2, 2, "older"));
    refused_unchanged(&dir, "starts after the snapshot");

    // B < S, and the WAL agrees at S: keep what follows S.
    let dir = TestDir::new();
    let (next, _) = installed(&dir);
    write(&dir, &snap(4, 2, "state-at-4"));
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(
        wal.state(),
        &adopt_snapshot(&next, LogBase { index: 4, term: 2 })
    );
    assert_eq!(wal.state().entries(), [entry(5, 2, "E")]);

    // B < S, the WAL disagrees at S (or ends before it): the whole log is superseded.
    let dir = TestDir::new();
    let (mut next, _) = installed(&dir);
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        next.term = 4;
        next.voted_for = None;
        wal.save(&next).unwrap();
    }
    write(&dir, &snap(9, 4, "from-a-leader"));
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(wal.state().base(), LogBase { index: 9, term: 4 });
    assert!(wal.state().entries().is_empty());
    assert_eq!((wal.state().term, wal.state().voted_for), (4, None));

    // B < S and the snapshot's term is newer than the WAL's: an install that
    // also raised the term was interrupted. This row was refused as impossible
    // until the independent review of 2026-11-14 produced it with a real
    // process (lesson 11); see interrupted_install_from_a_newer_term_raises_the_term.
    let dir = TestDir::new();
    installed(&dir);
    write(&dir, &snap(9, 7, "from-a-newer-term"));
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(wal.state().base(), LogBase { index: 9, term: 7 });
    assert_eq!((wal.state().term, wal.state().voted_for), (7, None));
}

#[test]
fn interrupted_install_rolls_forward_idempotently() {
    let dir = TestDir::new();
    let (next, _) = installed(&dir);
    // As if the process died after the snapshot rename, before the WAL rewrite.
    let newer = snap(5, 2, "state-at-5");
    fs::write(dir.0.join("snapshot"), encode_snapshot(&newer)).unwrap();
    fs::write(dir.0.join("raft.wal.tmp"), b"half-written").unwrap();
    let first = {
        let wal = Wal::open(&dir.0, 1).unwrap();
        assert_eq!(wal.state(), &adopt_snapshot(&next, newer.base));
        assert_eq!(wal.snapshot(), Some(&newer));
        fs::read(dir.wal()).unwrap()
    };
    assert_eq!(
        first[40..48],
        5_u64.to_le_bytes(),
        "the WAL was rewritten to start at 5"
    );
    assert!(!dir.0.join("raft.wal.tmp").exists());
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(wal.state(), &adopt_snapshot(&next, newer.base));
    drop(wal);
    assert_eq!(
        fs::read(dir.wal()).unwrap(),
        first,
        "a second restart changes nothing"
    );
}

/// docs/SNAPSHOT_COMPACTION.md §7.3 (lesson 11). A follower raises its term and
/// installs a whole snapshot in one barrier; if it dies after the snapshot file
/// is durable and before the WAL is rewritten, the snapshot's term is newer
/// than the WAL's. Recovery adopts that term, without a vote.
#[test]
fn interrupted_install_from_a_newer_term_raises_the_term() {
    let dir = TestDir::new();
    let mut old = state(3, Some(2));
    old.push(entry(1, 1, "A"));
    old.push(entry(2, 3, "B"));
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        wal.save(&old).unwrap();
    }
    let newer = snap(6, 5, "state-at-6-from-term-5");
    fs::write(dir.0.join("snapshot"), encode_snapshot(&newer)).unwrap();
    let first = {
        let wal = Wal::open(&dir.0, 1)
            .unwrap_or_else(|error| panic!("a node must start after this interruption: {error}"));
        let recovered = wal.state();
        assert_eq!(recovered.base(), newer.base);
        assert_eq!(
            recovered.last_index(),
            6,
            "the old log ended before the snapshot"
        );
        assert_eq!(
            recovered.term, 5,
            "the snapshot's term, the least that is valid"
        );
        assert_eq!(
            recovered.voted_for, None,
            "the vote of term 3 is not a vote of term 5"
        );
        assert!(recovered.valid());
        assert_eq!(wal.snapshot(), Some(&newer));
        fs::read(dir.wal()).unwrap()
    };
    assert_eq!(
        first[56..64],
        5_u64.to_le_bytes(),
        "start_term in the file header"
    );
    assert_eq!(first[72..76], [0; 4], "no vote in the file header");
    let mut wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!((wal.state().term, wal.state().voted_for), (5, None));
    // The recovered node works: it can vote in a later term and append.
    let mut next = wal.state().clone();
    next.term = 6;
    next.voted_for = Some(3);
    next.push(entry(7, 6, "G"));
    wal.save(&next).unwrap();
    drop(wal);
    assert_eq!(Wal::open(&dir.0, 1).unwrap().state(), &next);

    // A snapshot that is not newer than the WAL's term changes neither term nor vote.
    let dir = TestDir::new();
    {
        let mut wal = Wal::open(&dir.0, 1).unwrap();
        wal.save(&old).unwrap();
    }
    let same = snap(6, 3, "state-at-6-from-term-3");
    fs::write(dir.0.join("snapshot"), encode_snapshot(&same)).unwrap();
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!((wal.state().term, wal.state().voted_for), (3, Some(2)));
}

#[test]
fn large_retained_suffix_is_split_across_records() {
    let dir = TestDir::new();
    let mut wal = Wal::open(&dir.0, 1).unwrap();
    let mut log = state(2, Some(1));
    let value = "v".repeat(4096);
    // 1,200 entries of about 4 KiB: more than two maximal records.
    for index in 1..=1_200 {
        log.push(entry(index, 2, &value));
        if index % 300 == 0 {
            wal.save(&log).unwrap();
        }
    }
    let snapshot = snap(1, 2, "state-at-1");
    let next = adopt_snapshot(&log, snapshot.base);
    wal.install(&next, &snapshot).unwrap();
    assert!(
        wal.durable_sequence() >= 4 + 3,
        "the suffix needed at least three records"
    );
    drop(wal);
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(wal.state(), &next);
}

// ---- S4: configurations (docs/MEMBERSHIP.md §11) ----------------------------

/// Configuration entries are ordinary Transaction JSON: they survive a reopen
/// with the same configuration, and a snapshot may record any valid set of
/// voters (not only 1, 2, 3), which also survives a reopen.
#[test]
fn configuration_entries_and_snapshot_voters_survive_reopen() {
    let dir = TestDir::new();
    let config = |index: u64, ids: &[u8]| LogEntry {
        index,
        term: 2,
        command: Command::Config {
            voters: ids.to_vec(),
        },
    };
    let mut log = state(2, Some(1));
    log.push(config(1, &[1]));
    log.push(entry(2, 2, "A"));
    log.push(config(3, &[1, 5]));
    log.push(entry(4, 2, "B"));
    let mut wal = Wal::open(&dir.0, 1).unwrap();
    wal.save(&log).unwrap();
    drop(wal);
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(wal.state(), &log);
    assert_eq!(
        wal.state().latest_config(),
        Some((3, Voters::from_ids(&[1, 5]).unwrap()))
    );
    drop(wal);
    // A snapshot through index 3 records {1, 5}, the configuration there.
    let voters = Voters::from_ids(&[1, 5]).unwrap();
    let snapshot = Snapshot::new(LogBase { index: 3, term: 2 }, voters, b"at-3".to_vec());
    let mut wal = Wal::open(&dir.0, 1).unwrap();
    wal.install(&adopt_snapshot(&log, snapshot.base), &snapshot)
        .unwrap();
    drop(wal);
    let wal = Wal::open(&dir.0, 1).unwrap();
    assert_eq!(wal.snapshot().map(|s| s.voters), Some(voters.bits()));
    assert_eq!(
        wal.state().latest_config(),
        None,
        "only in the snapshot now"
    );
    let core = Core::restore(1, wal.state().clone(), wal.snapshot().cloned()).unwrap();
    assert_eq!(core.configuration(), Some((3, voters)));
}

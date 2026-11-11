//! Storage tests. Byte offsets are written out literally where a test pins the
//! on-disk layout, so that a wrong encoder cannot agree with itself.
use super::*;
use kv_state::StateMachine;
use raft_core::Core;
use raft_protocol::{ClientRequest, Command, Operation};
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
            next.log.push(entry(index as u64 + 1, 2, value));
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

#[test]
fn reserved_header_fields_must_be_pristine() {
    let dir = TestDir::new();
    let sample = sample(&dir);
    for (what, index) in [
        ("reserved bytes", 36),
        ("base index", 40),
        ("base term", 48),
        ("start term", 56),
        ("first sequence", 64),
        ("start vote", 72),
    ] {
        let mut changed = sample.bytes.clone();
        changed[index] ^= 2;
        reseal_file_header(&mut changed);
        let error = outcome(&dir, &changed).unwrap_err();
        assert!(error.contains("compacted start"), "{what}: {error}");
        assert!(untouched(&dir, &changed), "{what}");
    }
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

use raft_storage::Wal;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
static NEXT: AtomicU64 = AtomicU64::new(0);
/// These tests spawn child processes while also holding `node.lock` (a `flock`)
/// in this process. A child forked by one test briefly inherits every open
/// descriptor until it calls exec, including another test's lock, so a quick
/// drop-then-reopen there can fail with "already owned". Running the tests one
/// at a time removes the overlap. See docs/lessons/02-flaky-lock-inherited-by-child/.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/crash-tests")
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

#[test]
fn real_process_crash_boundaries_restore_only_complete_transactions() {
    let _serial = serial();
    for boundary in [
        "before_write",
        "after_write_before_sync",
        "after_sync_before_ack",
    ] {
        let dir = TestDir::new();
        let marker = dir.0.join("reached");
        let data = dir.0.join("node");
        let first = Command::new(env!("CARGO_BIN_EXE_wal_probe"))
            .arg(&data)
            .arg("1")
            .output()
            .unwrap();
        assert!(first.status.success());
        assert!(String::from_utf8_lossy(&first.stdout).starts_with("DURABLE 1"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_wal_probe"))
            .arg(&data)
            .arg("2")
            .env("RAFT_WAL_FAILPOINT", boundary)
            .env("RAFT_WAL_FAILPOINT_MARKER", &marker)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() && Instant::now() < deadline {
            assert!(
                child.try_wait().unwrap().is_none(),
                "probe exited before reaching {boundary}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        if !marker.exists() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("probe never reached {boundary}");
        }
        assert_eq!(fs::read_to_string(&marker).unwrap(), boundary);
        child.kill().unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(
            output.stdout.is_empty(),
            "crashed transaction was never acknowledged"
        );
        let wal = Wal::open(&data, 1).unwrap();
        match boundary {
            "before_write" => assert_eq!(wal.state().term, 1),
            "after_sync_before_ack" => assert_eq!(wal.state().term, 2),
            _ => assert!([1, 2].contains(&wal.state().term)),
        }
        assert_eq!(wal.state().last_index(), wal.state().term);
        assert_eq!(wal.durable_sequence(), wal.state().term);
    }
}

fn probe(data: &Path, term: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wal_probe"))
        .arg(data)
        .arg(term)
        .output()
        .unwrap()
}

/// Run the probe until it reports `boundary`, then SIGKILL it.
fn crash_at(data: &Path, marker: &Path, boundary: &str, term: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wal_probe"))
        .arg(data)
        .arg(term)
        .env("RAFT_WAL_FAILPOINT", boundary)
        .env("RAFT_WAL_FAILPOINT_MARKER", marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "probe exited before reaching {boundary}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    if !marker.exists() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("probe never reached {boundary}");
    }
    assert_eq!(fs::read_to_string(marker).unwrap(), boundary);
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "crashed transaction was never acknowledged"
    );
}

#[test]
fn real_process_partial_write_is_preserved_then_truncated() {
    let _serial = serial();
    for boundary in ["after_partial_header", "after_partial_payload"] {
        let dir = TestDir::new();
        let marker = dir.0.join("reached");
        let data = dir.0.join("node");
        assert!(probe(&data, "1").status.success());
        let wal_path = data.join("raft.wal");
        let durable = fs::read(&wal_path).unwrap();

        crash_at(&data, &marker, boundary, "2");
        let torn = fs::read(&wal_path).unwrap();
        assert!(
            torn.len() > durable.len(),
            "{boundary}: the killed process left part of a record"
        );
        assert_eq!(&torn[..durable.len()], &durable[..]);

        let wal = Wal::open(&data, 1).unwrap();
        assert_eq!(wal.state().term, 1, "{boundary}");
        assert_eq!(wal.state().last_index(), 1);
        assert_eq!(wal.durable_sequence(), 1);
        drop(wal);
        assert_eq!(fs::read(&wal_path).unwrap(), durable, "{boundary}");
        let copies: Vec<_> = fs::read_dir(data.join("torn"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(copies.len(), 1, "{boundary}");
        assert_eq!(fs::read(&copies[0]).unwrap(), &torn[durable.len()..]);

        // The directory is fully usable again by an ordinary process.
        let next = probe(&data, "2");
        assert!(next.status.success());
        assert!(String::from_utf8_lossy(&next.stdout).starts_with("DURABLE 2"));
    }
}

#[test]
fn real_process_crash_during_initialization_recovers() {
    let _serial = serial();
    for boundary in [
        "init_before_rename_raft.wal",
        "init_after_wal_before_identity",
        "init_before_rename_node.meta",
    ] {
        let dir = TestDir::new();
        let marker = dir.0.join("reached");
        let data = dir.0.join("node");
        crash_at(&data, &marker, boundary, "1");
        assert!(
            !data.join("node.meta").exists(),
            "{boundary}: identity is the last step of initialization"
        );
        let done = probe(&data, "1");
        assert!(
            done.status.success(),
            "{boundary}: {}",
            String::from_utf8_lossy(&done.stderr)
        );
        assert!(String::from_utf8_lossy(&done.stdout).starts_with("DURABLE 1"));
        let wal = Wal::open(&data, 1).unwrap();
        assert_eq!(wal.state().term, 1);
        assert_eq!(wal.durable_sequence(), 1);
    }
}

fn snapshot_probe(data: &Path, mode: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_snapshot_probe"))
        .arg(data)
        .args(mode)
        .output()
        .unwrap()
}

/// Same as `crash_at`, for the snapshot probe.
fn crash_snapshot_probe_at(data: &Path, marker: &Path, boundary: &str, mode: &[&str]) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_snapshot_probe"))
        .arg(data)
        .args(mode)
        .env("RAFT_WAL_FAILPOINT", boundary)
        .env("RAFT_WAL_FAILPOINT_MARKER", marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "probe exited before reaching {boundary}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(marker.exists(), "probe never reached {boundary}");
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "the install was never reported");
}

/// docs/SNAPSHOT_COMPACTION.md §7.4: a SIGKILL between any two install steps
/// leaves the old state or the new one, and a second restart changes nothing.
#[test]
fn real_process_crash_during_snapshot_install_recovers() {
    let _serial = serial();
    use raft_protocol::LogBase;
    let old = LogBase { index: 0, term: 0 };
    let new = LogBase { index: 2, term: 1 };
    for (boundary, expected) in [
        ("install_before_snapshot_rename", old),
        ("install_after_snapshot_before_wal", new),
        ("install_before_wal_rename", new),
        ("install_after_wal_rename", new),
    ] {
        let dir = TestDir::new();
        let marker = dir.0.join("reached");
        let data = dir.0.join("node");
        crash_snapshot_probe_at(&data, &marker, boundary, &[]);
        let (state, wal_bytes) = {
            let wal = Wal::open(&data, 1).unwrap();
            assert_eq!(wal.state().base(), expected, "{boundary}");
            assert_eq!(wal.state().last_index(), 3, "{boundary}: no entry is lost");
            assert_eq!(wal.snapshot().is_some(), expected == new, "{boundary}");
            (
                wal.state().clone(),
                fs::read(data.join("raft.wal")).unwrap(),
            )
        };
        assert!(!data.join("snapshot.tmp").exists() && !data.join("raft.wal.tmp").exists());
        let again =
            Wal::open(&data, 1).unwrap_or_else(|error| panic!("{boundary}: second open: {error}"));
        assert_eq!(again.state(), &state, "{boundary}");
        drop(again);
        assert_eq!(
            fs::read(data.join("raft.wal")).unwrap(),
            wal_bytes,
            "{boundary}"
        );
        // Whatever survived, the probe can finish the job.
        let done = snapshot_probe(&data, &[]);
        assert!(
            done.status.success(),
            "{boundary}: {}",
            String::from_utf8_lossy(&done.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&done.stdout).trim(), "INSTALLED 2");
    }
}

/// Lesson 11: the same four interruptions when the snapshot comes from a newer
/// term than the WAL holds (a follower that was away across an election gets
/// the whole snapshot in one message). Every one of them must leave a node
/// that starts: with the old state, or at the snapshot with at least its term.
#[test]
fn real_process_crash_during_install_from_a_newer_term_recovers() {
    let _serial = serial();
    use raft_protocol::LogBase;
    let mode = ["newer-term"];
    let old = (LogBase { index: 0, term: 0 }, 1, Some(1), 3);
    let new = (LogBase { index: 5, term: 4 }, 4, None, 5);
    for (boundary, expected) in [
        ("install_before_snapshot_rename", old),
        ("install_after_snapshot_before_wal", new),
        ("install_before_wal_rename", new),
        ("install_after_wal_rename", new),
    ] {
        let dir = TestDir::new();
        let marker = dir.0.join("reached");
        let data = dir.0.join("node");
        crash_snapshot_probe_at(&data, &marker, boundary, &mode);
        let (state, wal_bytes) = {
            let wal = Wal::open(&data, 1)
                .unwrap_or_else(|error| panic!("{boundary}: the node must start: {error}"));
            let state = wal.state();
            assert_eq!(
                (
                    state.base(),
                    state.term,
                    state.voted_for,
                    state.last_index()
                ),
                expected,
                "{boundary}"
            );
            assert_eq!(wal.snapshot().is_some(), expected == new, "{boundary}");
            (state.clone(), fs::read(data.join("raft.wal")).unwrap())
        };
        let again =
            Wal::open(&data, 1).unwrap_or_else(|error| panic!("{boundary}: second open: {error}"));
        assert_eq!(again.state(), &state, "{boundary}");
        drop(again);
        assert_eq!(
            fs::read(data.join("raft.wal")).unwrap(),
            wal_bytes,
            "{boundary}"
        );
        let done = snapshot_probe(&data, &mode);
        assert!(
            done.status.success(),
            "{boundary}: {}",
            String::from_utf8_lossy(&done.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&done.stdout).trim(), "INSTALLED 5");
    }
}

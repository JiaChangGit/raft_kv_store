"""S5 real-process tests: group commit and batches (docs/PERFORMANCE.md §10)."""
import concurrent.futures
import json
from tools.cluster import Cluster
from tools.kv_client import call, request
from tools.linearizability import check


def test_concurrent_writes_and_reads_stay_linearizable(tmp_path):
    # The checker explores at most 24 operations: 16 concurrent writes, then
    # 8 concurrent reads, all on two keys so that orders matter.
    with Cluster(tmp_path) as c:
        leader = c.wait_leader()
        with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
            writes = [pool.submit(c.invoke, request(600 + n, 1, "Put", f"k{n % 2}", str(n)), leader) for n in range(16)]
            assert all(f.result()["status"] == "OK" for f in writes)
            reads = [pool.submit(c.invoke, request(700 + n, 1, "Get", f"k{n % 2}"), leader) for n in range(8)]
            assert all(f.result()["status"] == "OK" for f in reads)
        # Every write went through a batch. Whether several shared one depends on
        # timing, so it is not asserted here; the crash test with the batch
        # failpoint below shows that they do (S5 review F5).
        assert c.status(leader)["batched_writes"] >= 16
        result = check(c.history)
        (c.directory / "checker.json").write_text(json.dumps(result, indent=2))
        assert result["status"] == "PASS", result


def test_a_batch_persisted_but_not_answered_is_not_executed_twice(tmp_path):
    with Cluster(tmp_path) as c:
        leader = c.wait_leader()
        before = {n: c.status(n)["effect_count"] for n in c.initial}
        # The leader stops right after the WAL sync of the first client batch,
        # before any reply leaves (the failpoint used since V1).
        (c.directory / f"failpoint-{leader}").write_text("after_sync_before_ack")
        payloads = [request(800 + n, 1, "Put", f"crash-{n}", str(n)) for n in range(8)]
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            first = list(pool.map(lambda p: c.invoke(p, node=leader, timeout=2), payloads))
        assert c.processes[leader].wait(timeout=5) == 91
        assert all(answer["status"] != "OK" for answer in first), first
        (c.directory / f"failpoint-{leader}").unlink()
        c.start_node(leader)
        # Retried with the same IDs and contents: each executes exactly once.
        retried = [c.invoke(p) for p in payloads]
        assert [answer["status"] for answer in retried] == ["OK"] * 8, retried
        assert all(answer["result"] is None for answer in retried), "nothing was there before"
        for n in c.initial:
            c.wait(lambda n=n: c.status(n)["effect_count"] == before[n] + 8)
        again = [c.invoke(p) for p in payloads]
        assert [a["committed_index"] for a in again] == [a["committed_index"] for a in retried]


def test_a_crash_right_after_a_batch_of_writes_executes_each_once(tmp_path):
    # S5 review F5: the test above stops at the first client batch, which may
    # hold a single write. This failpoint fires only after a batch of two or
    # more writes is durable, so the test cannot pass without such a batch.
    with Cluster(tmp_path) as c:
        leader = c.wait_leader()
        before = {n: c.status(n)["effect_count"] for n in c.initial}
        (c.directory / f"failpoint-{leader}").write_text("after_sync_of_a_batch_before_ack")
        payloads = [request(1000 + n, 1, "Put", f"batch-{n}", str(n)) for n in range(32)]
        with concurrent.futures.ThreadPoolExecutor(max_workers=32) as pool:
            first = list(pool.map(lambda p: c.invoke(p, node=leader, timeout=2), payloads))
        assert c.processes[leader].wait(timeout=5) == 91, "no batch of two or more writes formed"
        trace = (c.directory / f"node-{leader}.jsonl").read_text().splitlines()
        stop = [json.loads(line) for line in trace if '"kind":"failpoint"' in line]
        batch = {int(client) for client in stop[-1]["details"]["client_ids"]}
        assert len(batch) >= 2, stop
        # Earlier batches may have been answered; this one was durable, not answered.
        assert all(a["status"] != "OK" for p, a in zip(payloads, first) if p["client_id"] in batch), first
        (c.directory / f"failpoint-{leader}").unlink()
        c.start_node(leader)
        retried = [c.invoke(p) for p in payloads]
        assert [answer["status"] for answer in retried] == ["OK"] * 32, retried
        for n in c.initial:
            c.wait(lambda n=n: c.status(n)["effect_count"] == before[n] + 32)
        again = [c.invoke(p) for p in payloads]
        assert [a["committed_index"] for a in again] == [a["committed_index"] for a in retried]


def test_a_follower_far_behind_catches_up_from_the_log(tmp_path):
    # S5 review F2: since S5 a request carries up to 256 entries. Put the
    # follower's limit back to 32 and a follower more than 32 entries behind,
    # with no snapshot to fall back on, refuses every request and never catches
    # up. (The reviewer's probe, kept as a regression test.)
    with Cluster(tmp_path) as c:
        leader = c.wait_leader()
        lagging = next(n for n in c.initial if n != leader)
        c.kill(lagging)
        for sequence in range(1, 101):
            assert c.invoke(request(5000, sequence, "Put", f"k{sequence}", "v"))["status"] == "OK"
        leader = c.wait_leader(exclude=[lagging])
        assert c.status(leader)["snapshot_index"] == 0, "the follower must catch up from the log"
        c.start_node(lagging)
        c.wait(lambda: c.status(lagging)["last_applied"] >= c.status(leader)["last_applied"], timeout=15)


def test_tools_keep_working_when_the_map_exceeds_a_frame(tmp_path):
    # Lesson 15: about 1.2 MiB of values used to make the status unanswerable,
    # and with it Cluster.wait_leader and tools/kv_admin.py.
    with Cluster(tmp_path) as c:
        leader = c.wait_leader()
        for n in range(300):
            assert call(c.address(leader), request(900, n + 1, "Put", f"big-{n}", "v" * 4096))["status"] == "OK"
        assert c.wait_leader() == leader
        status = c.status(leader)
        assert status["kv_omitted"] is True and status["kv_bytes"] > 1024 * 1024
        assert status["role"] == "Leader" and status["last_applied"] >= 300

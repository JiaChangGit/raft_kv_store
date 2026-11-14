#!/usr/bin/env python3
"""Write throughput, latency and recovery before and after S5 (docs/PERFORMANCE.md section 9).

Builds raft-node (release) from the tag before S5 in a temporary directory and
runs the same workloads against it and against the current source: three
nodes, no fault proxies, data on the file system of artifacts/ (not tmpfs,
where fsync costs nothing). Times are recorded, never gated: they depend on
the machine. The deterministic counts (how often the leader persisted) are
what acceptance looks at.

Requests go straight to the leader with tools.kv_client.call. They must not go
through Cluster.invoke: that rewrites the whole history file on every call,
which costs more than the write being measured once a few thousand 1 KiB
values are in it (lesson 14).
"""
import concurrent.futures
import datetime
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from scripts.verify_gate import ROOT, snapshot  # noqa: E402
from tools.cluster import Cluster  # noqa: E402
from tools.kv_client import call, request  # noqa: E402

BEFORE = "s1-s4-review-verified"
CLIENTS = (1, 8, 32)
PER_CLIENT = int(os.environ.get("WRITE_BENCHMARK_PER_CLIENT", "60"))
GROWTH_WRITES = int(os.environ.get("WRITE_BENCHMARK_GROWTH", "3000"))
WINDOW = 250


def percentiles(values):
    values = sorted(values)
    pick = lambda q: values[min(len(values) - 1, int(q * len(values)))]  # noqa: E731
    return {"count": len(values), "p50": statistics.median(values), "p95": pick(.95), "p99": pick(.99)}


def trace(cluster, node):
    for line in (cluster.directory / f"node-{node}.jsonl").read_text().splitlines():
        try:
            yield json.loads(line)
        except json.JSONDecodeError:
            continue


def persists(cluster, node):
    """Completed WAL saves in the node's trace; the same rule for every version."""
    return sum(1 for row in trace(cluster, node) if row.get("kind") == "persist")


def put(cluster, leader, client, sequence, key, value):
    begin = time.perf_counter()
    answer = call(cluster.address(leader), request(client, sequence, "Put", key, value), timeout=30)
    if answer["status"] != "OK":
        raise RuntimeError(answer)
    return (time.perf_counter() - begin) * 1000


def concurrency(binary, directory):
    rows = {}
    with Cluster(directory, proxy=False, binary=binary, snapshot_every=64) as c:
        leader = c.wait_leader()
        put(c, leader, 1, 1, "warm", "x")
        for clients in CLIENTS:
            count = PER_CLIENT * (5 if clients == 1 else 1)
            before = persists(c, leader)

            def run(client):
                return [put(c, leader, client, n + 1, f"k{client % 16}", "v" * 64) for n in range(count)]

            begin = time.perf_counter()
            with concurrent.futures.ThreadPoolExecutor(max_workers=clients) as pool:
                latencies = [ms for mine in pool.map(run, range(1000 * clients, 1000 * clients + clients)) for ms in mine]
            elapsed = time.perf_counter() - begin
            rows[str(clients)] = {"writes": len(latencies), "writes_per_second": len(latencies) / elapsed,
                                  "latency_ms": percentiles(latencies),
                                  "leader_persists": persists(c, leader) - before}
    return rows


def growth(binary, directory):
    """Sequential 1 KiB writes to distinct keys under the default compaction
    policy, then the time one follower needs to start again from that data."""
    rows = []
    with Cluster(directory, proxy=False, binary=binary) as c:
        leader = c.wait_leader()
        value = "v" * 1024
        for start in range(0, GROWTH_WRITES, WINDOW):
            latencies = [put(c, leader, 77, n + 1, f"key-{n}", value) for n in range(start, start + WINDOW)]
            last = base = snapshot_bytes = 0
            for row in trace(c, leader):
                last = row.get("last_log_index", last)
                if row.get("kind") == "snapshot_persisted":
                    base, snapshot_bytes = row["details"]["index"], row["details"]["bytes"]
            rows.append({"writes": start + WINDOW, "log_entries": last - base, "snapshot_bytes": snapshot_bytes,
                         "latency_ms": percentiles(latencies)})
        follower = next(n for n in c.initial if n != leader)
        c.kill(follower)
        wal_bytes = (c.directory / f"node-{follower}" / "raft.wal").stat().st_size
        started = sum(1 for row in trace(c, follower) if row.get("kind") == "started")
        begin = time.perf_counter()
        c.start_node(follower)
        c.wait(lambda: sum(1 for row in trace(c, follower) if row.get("kind") == "started") > started, timeout=120)
        restart = {"wal_bytes": wal_bytes, "restart_to_started_ms": (time.perf_counter() - begin) * 1000}
    return rows, restart


def measure(label, binary, work):
    rows, restart = growth(binary, work / f"{label}-growth")
    return {"label": label, "binary": str(binary), "concurrency": concurrency(binary, work / f"{label}-concurrency"),
            "growth": rows, "restart": restart}


def main():
    _, fingerprint = snapshot(ROOT)
    base = ROOT / "artifacts" / "runs"
    base.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="write-benchmark-", dir=base))
    try:
        old_src = work / "old"
        old_src.mkdir()
        archive = subprocess.run(["git", "archive", BEFORE], cwd=ROOT, capture_output=True, check=True)
        subprocess.run(["tar", "-x", "-C", str(old_src)], input=archive.stdout, check=True)
        subprocess.run(["cargo", "build", "-q", "--release", "--locked", "-p", "raft-node"], cwd=old_src, check=True,
                       env={**os.environ, "CARGO_TARGET_DIR": str(work / "target-old")})
        subprocess.run(["cargo", "build", "-q", "--release", "--locked", "-p", "raft-node"], cwd=ROOT, check=True)
        only = os.environ.get("WRITE_BENCHMARK_ONLY")
        runs = []
        if only != "after":
            runs.append(measure("before", work / "target-old/release/raft-node", work))
        if only != "before":
            runs.append(measure("after", ROOT / "target/release/raft-node", work))
        if snapshot(ROOT)[1] != fingerprint:
            print("source changed while the benchmark ran", file=sys.stderr)
            return 1
        report = {"command": "python scripts/write_benchmark.py", "date": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  "source_fingerprint": fingerprint, "before_tag": BEFORE, "writes_per_run": GROWTH_WRITES,
                  "workload": "release builds, three nodes, no fault proxies, one connection per request from Python threads; "
                              "concurrency: 64-byte values with --snapshot-every 64; growth: sequential 1 KiB values to distinct keys, default compaction",
                  "environment": {"platform": platform.platform(), "cpu_count": os.cpu_count(), "python": sys.version.split()[0],
                                  "data_directory": str(base)},
                  "meaning": "Times are recorded for comparison only; not an acceptance threshold. leader_persists is a count.",
                  "runs": runs}
        out = ROOT / "artifacts" / "reports" / "write-benchmark.json"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, indent=2) + "\n")
        for run in runs:
            for clients, row in run["concurrency"].items():
                print(f"{run['label']:6} {clients:>2} clients  {row['writes_per_second']:8.1f} writes/s  p50 {row['latency_ms']['p50']:7.2f} ms  "
                      f"leader persists {row['leader_persists']} for {row['writes']} writes")
            for row in run["growth"][::max(1, len(run["growth"]) // 6)]:
                print(f"{run['label']:6} growth  writes {row['writes']:6}  log {row['log_entries']:6}  snapshot {row['snapshot_bytes']:9}  p50 {row['latency_ms']['p50']:6.2f} ms")
            print(f"{run['label']:6} restart {run['restart']}")
        print(f"report: {out.relative_to(ROOT)}")
        return 0
    finally:
        shutil.rmtree(work, ignore_errors=True)  # only the directory created above


if __name__ == "__main__":
    raise SystemExit(main())

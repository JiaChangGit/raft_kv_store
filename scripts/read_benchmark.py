#!/usr/bin/env python3
"""GET latency and WAL growth before and after S3 (docs/READ_INDEX.md section 9).

Builds raft-node from the S2 tag in a temporary directory, runs the same
sequential GET workload against it and against the current build, and records
the numbers with the source fingerprint. Numbers are recorded, never gated:
they depend on the machine. The deterministic property (a GET writes no WAL
record) is asserted by tests/integration/test_read_index.py.
"""
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
from tools.kv_client import request  # noqa: E402

BEFORE = "s2-snapshot-compaction-verified"
READS = int(os.environ.get("READ_BENCHMARK_READS", "300"))


def measure(label, binary, directory, legacy=False):
    # The S2 binary predates S4's bootstrap and membership requests.
    with Cluster(directory, proxy=False, binary=binary, legacy=legacy) as c:
        assert c.invoke(request(700, 1, "Put", "bench", "v" * 64))["status"] == "OK"
        leader = c.wait_leader()
        wal = lambda: {n: c.status(n)["wal_bytes"] for n in (1, 2, 3)}  # noqa: E731
        sequence = lambda: {n: c.status(n)["durable_sequence"] for n in (1, 2, 3)}  # noqa: E731
        before_wal, before_sequence = wal(), sequence()
        latencies = []
        for i in range(READS):
            begin = time.perf_counter()
            answer = c.invoke(request(701, i + 1, "Get", "bench"), node=leader)
            latencies.append((time.perf_counter() - begin) * 1000)
            if answer["status"] != "OK":
                raise RuntimeError(f"{label}: {answer}")
        after_wal, after_sequence = wal(), sequence()
    latencies.sort()
    pick = lambda q: latencies[min(len(latencies) - 1, int(q * len(latencies)))]  # noqa: E731
    growth = {n: after_wal[n] - before_wal[n] for n in after_wal}
    return {
        "label": label,
        "binary": str(binary),
        "reads": READS,
        "latency_ms": {"p50": statistics.median(latencies), "p95": pick(.95), "p99": pick(.99),
                       "mean": statistics.fmean(latencies), "max": latencies[-1]},
        "wal_bytes_grown": growth,
        "wal_bytes_per_get": {n: growth[n] / READS for n in growth},
        "wal_records_written": {n: after_sequence[n] - before_sequence[n] for n in after_sequence},
    }


def main():
    _, fingerprint = snapshot(ROOT)
    work = Path(tempfile.mkdtemp(prefix="read-benchmark-", dir=os.environ.get("TMPDIR")))
    try:
        run_all(work, fingerprint)
    finally:
        shutil.rmtree(work, ignore_errors=True)  # only the directory created above


def run_all(work, fingerprint):
    old_src = work / "old"
    old_src.mkdir()
    archive = subprocess.run(["git", "archive", BEFORE], cwd=ROOT, capture_output=True, check=True)
    subprocess.run(["tar", "-x", "-C", str(old_src)], input=archive.stdout, check=True)
    subprocess.run(["cargo", "build", "-q", "--locked", "-p", "raft-node"], cwd=old_src, check=True,
                   env={**os.environ, "CARGO_TARGET_DIR": str(work / "target-old")})
    subprocess.run(["cargo", "build", "-q", "--locked", "-p", "raft-node"], cwd=ROOT, check=True)
    runs = [measure(f"before ({BEFORE})", work / "target-old/debug/raft-node", work / "cluster-old", legacy=True),
            measure("after (current source)", ROOT / "target/debug/raft-node", work / "cluster-new")]
    if snapshot(ROOT)[1] != fingerprint:
        raise SystemExit("source changed during the benchmark")
    report = {
        "command": "python scripts/read_benchmark.py",
        "date": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "source_fingerprint": fingerprint,
        "reads_per_run": READS,
        "workload": "one client, sequential GETs of one 64-byte value at the leader, no fault proxies, debug builds",
        "environment": {"platform": platform.platform(), "python": platform.python_version(),
                        "cpus": os.cpu_count(), "rustc": subprocess.run(["rustc", "-V"], capture_output=True, text=True).stdout.strip()},
        "meaning": "Recorded for comparison only; not an acceptance threshold. Numbers vary by machine and load.",
        "runs": runs,
    }
    out = ROOT / "artifacts" / "reports" / "read-benchmark.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n")
    for run in runs:
        print(f"{run['label']:<44} p50 {run['latency_ms']['p50']:.2f} ms  p99 {run['latency_ms']['p99']:.2f} ms  "
              f"WAL records written per node {run['wal_records_written']}")
    print(f"report: {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

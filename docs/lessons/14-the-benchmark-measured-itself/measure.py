#!/usr/bin/env python3
"""Lesson 14 helper: the same node, the same writes, two ways of sending them.
Usage: measure.py SOURCE BINARY WORKDIR. SOURCE is the checkout whose tools/
are used (the old one shows the bug). Prints p50 latency per 500 writes."""
import statistics, sys, time
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from tools.cluster import Cluster
from tools.kv_client import call, request

binary, work = Path(sys.argv[2]), Path(sys.argv[3])
value = "v" * 1024
for way in ("Cluster.invoke", "kv_client.call"):
    with Cluster(work / way, proxy=False, binary=binary, snapshot_every=64) as c:
        leader = c.wait_leader()
        line = []
        for start in range(0, 3000, 500):
            ms = []
            for n in range(start, start + 500):
                payload = request(77, n + 1, "Put", f"key-{n}", value)
                begin = time.perf_counter()
                answer = c.invoke(payload, node=leader) if way == "Cluster.invoke" else call(c.address(leader), payload)
                ms.append((time.perf_counter() - begin) * 1000)
                assert answer["status"] == "OK", answer
            line.append(f"{statistics.median(ms):5.1f}")
        size = (c.directory / "history.json").stat().st_size if (c.directory / "history.json").exists() else 0
        print(f"   {way:15} p50 ms per 500 writes: {' '.join(line)}   history.json {size / 2**20:.1f} MiB")

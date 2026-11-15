"""Lesson 18 helper: stop a follower, commit 100 writes (or the number given
fourth) without a snapshot, start it again and wait up to 15 s for it to catch
up from the log."""
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.cluster import Cluster  # noqa: E402
from tools.kv_client import request  # noqa: E402

binary, directory, heading = sys.argv[1], Path(sys.argv[2]), sys.argv[3]
writes = int(sys.argv[4]) if len(sys.argv) > 4 else 100
print(f"\n== {heading}")
with Cluster(directory, binary=binary) as c:
    leader = c.wait_leader()
    lagging = next(n for n in c.initial if n != leader)
    c.kill(lagging)
    for sequence in range(1, writes + 1):
        assert c.invoke(request(5000, sequence, "Put", f"k{sequence}", "v"))["status"] == "OK"
    leader = c.wait_leader(exclude=[lagging])
    target = c.status(leader)["last_applied"]
    print(f"   node {lagging} stopped, {writes} writes committed (last_applied {target}, "
          f"snapshot_index {c.status(leader)['snapshot_index']}); node {lagging} started again")
    c.start_node(lagging)
    started = time.monotonic()
    try:
        c.wait(lambda: c.status(lagging)["last_applied"] >= target, timeout=15)
        print(f"   node {lagging} caught up in {time.monotonic() - started:.1f} s")
    except TimeoutError:
        behind = c.status(lagging)
        print(f"   node {lagging} NOT caught up after 15 s: role {behind['role']}, term {behind['term']}, "
              f"last_applied {behind['last_applied']}")
        for n in c.initial:
            if n != lagging:
                s = c.status(n)
                print(f"   node {n}: role {s['role']}, term {s['term']}, entries sent in AppendEntries {s['append_entries_sent']}")

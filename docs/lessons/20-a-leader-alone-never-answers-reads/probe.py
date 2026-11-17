"""Lesson 20 helper: start a cluster whose only voter is node 1, write one key,
wait until nothing else is in flight, then read it back and time the read."""
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.cluster import Cluster  # noqa: E402
from tools.kv_client import request  # noqa: E402

binary, directory, heading = sys.argv[1], Path(sys.argv[2]), sys.argv[3]
print(f"\n== {heading}")
with Cluster(directory, binary=binary, nodes=1, proxy=False) as c:
    status = c.status(1)
    print(f"   voters {status['voters']}, node 1 is {status['role']} of term {status['term']}")
    put = c.invoke(request(20, 1, "Put", "x", "A"), node=1)
    print(f"   Put x A -> {put['status']}")
    time.sleep(0.3)  # nothing else is happening when the read arrives
    started = time.monotonic()
    get = c.invoke(request(20, 2, "Get", "x"), node=1, timeout=5)
    took = time.monotonic() - started
    print(f"   Get x   -> {get['status']}, result {get.get('result')!r}, after {took:.1f} s")
    status = c.status(1)
    print(f"   node 1 status: reads_served {status['reads_served']}, reads_waiting {status['reads_waiting']}")

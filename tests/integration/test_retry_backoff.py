import json
import time
from tools.cluster import Cluster
from tools.kv_client import request


def test_unavailable_follower_has_bounded_transport_retries(tmp_path):
    with Cluster(tmp_path) as cluster:
        leader=cluster.wait_leader()
        follower=next(n for n in (1,2,3) if n!=leader)
        cluster.kill(follower)
        log=tmp_path/f"node-{leader}.jsonl"
        before=len(log.read_text().splitlines())
        start=time.monotonic()
        for seq in range(3):
            assert cluster.invoke(request(314,seq,"Put","retry","v"))["status"]=="OK"
        while time.monotonic()-start<1.1:
            time.sleep(.02)
        rows=[json.loads(line) for line in log.read_text().splitlines()[before:]]
        retries=sum(row["kind"]=="event" and row["details"].get("event")=="rpc_timeout"
                    and row["details"].get("peer")==follower for row in rows)
        (tmp_path/"retry-count.json").write_text(json.dumps({"count":retries,"window_seconds":time.monotonic()-start}))
        assert 0<retries<=12, "network failures must wait for the original retry deadline"

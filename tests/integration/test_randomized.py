import concurrent.futures
import json
import random
from tools.cluster import Cluster
from tools.kv_client import request
from tools.linearizability import check

def test_seeded_fault_history_is_linearizable(tmp_path):
    for seed in (87,231,991):
        rng=random.Random(seed)
        with Cluster(tmp_path/str(seed)) as c:
            # Three distinct clients may overlap; each client has only one outstanding request.
            with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
                futures=[pool.submit(c.invoke,request(n,1,"Put",f"k{n%2}",str(rng.randrange(100)))) for n in (101,102,103)]
                assert all(f.result()["status"]=="OK" for f in futures)
            old=c.wait_leader()
            c.proxy.isolate(old)
            new=c.wait_leader(exclude=[old])
            assert c.invoke(request(104,1,"Put","k1","after"),node=new)["status"]=="OK"
            c.invoke(request(105,1,"Put","unknown","maybe"),node=old,timeout=.3)
            c.kill(old)
            c.proxy.heal()
            c.start_node(old)
            assert c.invoke(request(106,1,"Get","k1"))["result"]=="after"
            assert c.invoke(request(105,1,"Put","unknown","maybe"))["status"]=="OK"
            result=check(c.history)
            (c.directory/"checker.json").write_text(json.dumps(result,indent=2))
            assert result["status"]=="PASS", (seed,result,c.history)

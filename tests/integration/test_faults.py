from tools.cluster import Cluster
from tools.kv_client import request

def test_partitioned_leader_cannot_commit_or_read(tmp_path):
    with Cluster(tmp_path) as c:
        assert c.invoke(request(21,1,"Put","x","old"))["status"]=="OK"
        old=c.wait_leader()
        c.proxy.isolate(old)
        new=c.wait_leader(exclude=[old])
        assert c.invoke(request(22,1,"Put","x","new"),node=new)["status"]=="OK"
        assert c.invoke(request(23,1,"Get","x"),node=old,timeout=.6)["status"]!="OK"
        assert c.invoke(request(23,2,"Put","x","bad"),node=old,timeout=.6)["status"]!="OK"
        # Also retry a cached old request against the isolated leader.
        assert c.invoke(request(21,1,"Put","x","old"),node=old,timeout=.6)["status"]!="OK"

def test_heal_recovers_and_converges(tmp_path):
    with Cluster(tmp_path) as c:
        old=c.wait_leader()
        c.proxy.isolate(old)
        c.invoke(request(31,1,"Put","x","orphan"),node=old,timeout=.3)
        new=c.wait_leader(exclude=[old])
        assert c.invoke(request(32,1,"Put","x","majority"),node=new)["status"]=="OK"
        c.proxy.heal()
        done=c.invoke(request(32,2,"Get","x"))
        assert done["status"]=="OK" and done["result"]=="majority"
        c.wait(lambda:all(c.status(n)["last_applied"]>=done["committed_index"] for n in (1,2,3)))
        assert all(c.status(n)["kv"]["x"]=="majority" for n in (1,2,3))

def test_fault_proxy_does_not_bypass_route(tmp_path):
    with Cluster(tmp_path) as c:
        for a,b,_,_ in c.routes: c.proxy.set_rule(a,b,"drop")
        # Observe only after all six rule callbacks have completed; traffic during
        # installation legitimately sees the previously active per-edge rule.
        before=len(c.proxy.events)
        response=c.invoke(request(40,1,"Put","blocked","v"),timeout=1)
        assert response["status"]!="OK"
        frames=[e for e in c.proxy.events[before:] if e["kind"]=="frame"]
        assert frames and all(e["mode"]=="drop" for e in frames)
        assert {(a,b) for a,b,_,_ in c.routes}=={(1,2),(1,3),(2,1),(2,3),(3,1),(3,2)}
        c.proxy.heal()
        assert c.invoke(request(40,1,"Put","blocked","v"))["status"]=="OK"

"""S2 with three real processes: bounded WAL, catch-up through InstallSnapshot,
and restart from snapshots. docs/SNAPSHOT_COMPACTION.md section 11."""
import json
from tools.cluster import Cluster
from tools.kv_client import request
from tools.linearizability import check

def statuses(c):
    return {n:c.status(n) for n in (1,2,3)}

def test_wal_stays_bounded_under_load(tmp_path):
    value="v"*1000
    with Cluster(tmp_path,snapshot_every=10) as c:
        for sequence in range(1,201):
            assert c.invoke(request(400,sequence,"Put",f"k{sequence%7}",value))["status"]=="OK"
        done=c.invoke(request(400,201,"Get","k0"))
        assert done["status"]=="OK" and done["result"]==value
        c.wait(lambda:all(s["last_applied"]>=done["committed_index"] for s in statuses(c).values()))
        for n,s in statuses(c).items():
            # Without compaction 200 entries of 1 KB would exceed 200 KB.
            assert s["snapshots_taken"]>=15,(n,s)
            assert s["snapshot_index"]>=done["committed_index"]-11,(n,s)
            assert s["wal_bytes"]<40_000,(n,s)
            assert s["session_count"]==1,(n,s)
        (c.directory/"statuses.json").write_text(json.dumps(statuses(c),indent=2))

def test_lagging_follower_catches_up_via_snapshot(tmp_path):
    with Cluster(tmp_path,snapshot_every=3) as c:
        leader=c.wait_leader()
        lagging=next(n for n in (1,2,3) if n!=leader)
        assert c.invoke(request(410,1,"Put","x","before"))["status"]=="OK"
        c.kill(lagging)
        for sequence in range(2,20):
            assert c.invoke(request(410,sequence,"Put",f"k{sequence}",str(sequence)))["status"]=="OK"
        leader=c.wait_leader(exclude=[lagging])
        # The entries the stopped node needs are already gone from the leader.
        assert c.status(leader)["snapshot_index"]>=12
        c.start_node(lagging)
        done=c.invoke(request(410,20,"Get","x"))
        assert done["status"]=="OK" and done["result"]=="before"
        c.wait(lambda:c.status(lagging)["last_applied"]>=done["committed_index"],timeout=15)
        caught_up=c.status(lagging)
        assert caught_up["snapshots_installed"]>=1,caught_up
        assert caught_up["kv"]==c.status(leader)["kv"]
        result=check(c.history)
        (c.directory/"checker.json").write_text(json.dumps(result,indent=2))
        assert result["status"]=="PASS",(result,c.history)

def test_full_restart_recovers_from_snapshots_and_sessions(tmp_path):
    with Cluster(tmp_path,snapshot_every=4) as c:
        last=None
        for sequence in range(1,11):
            last=request(420,sequence,"Put","x",f"v{sequence}")
            answer=c.invoke(last)
            assert answer["status"]=="OK"
        original=answer
        done=c.invoke(request(421,1,"Get","x"))
        c.wait(lambda:all(s["last_applied"]>=done["committed_index"] for s in statuses(c).values()))
        effects=c.status(1)["effect_count"]
        for n in (1,2,3): c.kill(n)
        for n in (1,2,3): c.start_node(n)
        # Each node starts from its own snapshot, not from an empty state machine.
        for n in (1,2,3):
            restarted=c.wait(lambda n=n:c.status(n))
            assert restarted["snapshot_index"]>0,restarted
            assert restarted["last_applied"]>=restarted["snapshot_index"]
        c.wait_leader()
        # The session for client 420 came back with the snapshot: the retry is
        # answered with the original result and executes nothing.
        again=c.invoke(last)
        assert again["status"]=="OK" and again["result"]==original["result"]
        assert again["committed_index"]==original["committed_index"]
        assert c.invoke(request(421,2,"Get","x"))["result"]=="v10"
        leader=c.wait_leader()
        assert c.status(leader)["effect_count"]==effects+1, "only the new Get executed"

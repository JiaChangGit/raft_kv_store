from tools.cluster import Cluster
from tools.kv_client import request

def test_retry_after_leader_crash(tmp_path):
    with Cluster(tmp_path) as c:
        r=request(11,1,"Put","x","A")
        old=c.wait_leader()
        assert c.invoke(r)["status"]=="OK"
        # Retry after the acknowledged operation's leader has crashed.
        c.kill(old)
        c.wait_leader(exclude=[old])
        again=c.invoke(r)
        assert again["status"]=="OK" and again["result"] is None
        assert c.invoke(request(11,2,"Get","x"))["result"]=="A"
        conflict=c.invoke(request(11,1,"Put","x","B"))
        assert conflict["status"]=="REQUEST_ID_REUSE"
        c.start_node(old)
        c.wait(lambda:c.status(old)["last_applied"]>=again["committed_index"])

def test_commit_before_reply_crash_has_unknown_outcome(tmp_path):
    with Cluster(tmp_path) as c:
        leader=c.wait_leader()
        marker=c.directory/f"failpoint-{leader}"
        marker.write_text("after_apply_before_reply")
        payload=request(19,1,"Put","unknown","A")
        result=c.invoke(payload,node=leader,timeout=5)
        assert result["status"]=="TIMEOUT_UNKNOWN"
        assert c.processes[leader].wait(timeout=3)==91
        marker.unlink()
        new=c.wait_leader(exclude=[leader])
        assert new!=leader
        c.start_node(leader)
        retry=c.invoke(payload)
        assert retry["status"]=="OK" and retry["result"] is None
        c.wait(lambda:c.status(new)["last_applied"]>=retry["committed_index"])
        assert c.status(new)["effect_count"]==1

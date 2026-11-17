"""S3 with three real processes. docs/READ_INDEX.md section 10."""
import json
from tools.cluster import Cluster
from tools.kv_client import request
from tools.linearizability import check

def durable(c):
    return {n:(c.status(n)["durable_sequence"],c.status(n)["wal_bytes"]) for n in (1,2,3)}

def test_reads_do_not_write_the_wal(tmp_path):
    with Cluster(tmp_path) as c:
        done=c.invoke(request(500,1,"Put","x","stored"))
        assert done["status"]=="OK"
        leader=c.wait_leader()
        c.wait(lambda:all(c.status(n)["last_applied"]>=done["committed_index"] for n in (1,2,3)))
        term=c.status(leader)["term"]
        before=durable(c)
        for sequence in range(2,102):
            answer=c.invoke(request(500,sequence,"Get","x"),node=leader)
            assert answer["status"]=="OK" and answer["result"]=="stored",answer
        after=durable(c)
        status=c.status(leader)
        (c.directory/"durable.json").write_text(json.dumps({"before":before,"after":after,"leader":status},indent=2))
        assert status["term"]==term, "no election happened during the reads"
        assert after==before, "100 GETs wrote nothing to any node's WAL"
        assert status["reads_served"]>=100
        assert status["effect_count"]==1, "GETs execute nothing in the state machine"

def test_new_leader_reads_the_latest_acknowledged_write(tmp_path):
    with Cluster(tmp_path) as c:
        assert c.invoke(request(510,1,"Put","x","old"))["status"]=="OK"
        old=c.wait_leader()
        assert c.invoke(request(510,2,"Put","x","new"),node=old)["status"]=="OK"
        c.kill(old)
        new=c.wait_leader(exclude=[old])
        # Ask the new leader right away: it must not answer from a commit index
        # that has not yet caught up with the old leader's (READ_INDEX.md G4).
        answer=c.invoke(request(511,1,"Get","x"))
        assert answer["status"]=="OK" and answer["result"]=="new",answer
        c.start_node(old)
        assert c.invoke(request(511,2,"Get","x"))["result"]=="new"
        result=check(c.history)
        (c.directory/"checker.json").write_text(json.dumps(result,indent=2))
        assert result["status"]=="PASS",(result,c.history)

def test_a_leader_left_with_a_minority_does_not_answer_reads(tmp_path):
    """Five nodes. The leader keeps one follower and loses the other three, who
    elect a new leader and accept a write. The one follower still acknowledges
    the old leader in its term, so the old leader keeps getting replies: the
    only thing between a client and the old value is that one follower plus
    itself is not a majority of five (READ_INDEX.md G3, lesson 12). A leader
    that is cut off from everybody gets no replies at all and proves less."""
    with Cluster(tmp_path,nodes=5) as c:
        assert c.invoke(request(520,1,"Put","x","old"))["status"]=="OK"
        old=c.wait_leader()
        friend=next(n for n in c.ids if n!=old)
        minority={old,friend}
        for a,b,_,_ in c.routes:
            if (a in minority)!=(b in minority): c.proxy.set_rule(a,b,"drop")
        new=c.wait_leader(exclude=minority)
        assert c.invoke(request(521,1,"Put","x","new"),node=new)["status"]=="OK"
        before=c.status(old)
        assert before["role"]=="Leader" and c.status(friend)["term"]==before["term"], "the old leader still has a follower"
        answer=c.invoke(request(522,1,"Get","x"),node=old,timeout=1.0)
        after=c.status(old)
        (c.directory/"old-leader.json").write_text(json.dumps({"before":before,"answer":answer,"after":after},indent=2))
        assert answer["status"]!="OK",answer
        assert after["role"]=="Leader" and after["reads_served"]==before["reads_served"]
        c.proxy.heal()
        done=c.invoke(request(522,2,"Get","x"))
        assert done["status"]=="OK" and done["result"]=="new",done
        result=check(c.history)
        (c.directory/"checker.json").write_text(json.dumps(result,indent=2))
        assert result["status"]=="PASS",(result,c.history)

def test_a_single_node_cluster_answers_reads_at_once(tmp_path):
    """Right after bootstrap the cluster has one voter. That leader is its own
    majority, so no reply from anybody will ever finish a read: it has to
    answer when the read arrives (lesson 20). Before the fix every GET here
    ran into the node's three-second deadline and came back TIMEOUT_UNKNOWN."""
    import time
    with Cluster(tmp_path,nodes=1,proxy=False) as c:
        assert c.status(1)["voters"]==[1]
        assert c.invoke(request(530,1,"Put","x","alone"),node=1)["status"]=="OK"
        # Let the write settle so that nothing but the read itself can answer it.
        time.sleep(0.3)
        started=time.monotonic()
        answer=c.invoke(request(530,2,"Get","x"),node=1,timeout=5)
        elapsed=time.monotonic()-started
        assert answer["status"]=="OK" and answer["result"]=="alone",answer
        assert elapsed<1.0,f"the read waited {elapsed:.2f}s"
        assert c.status(1)["reads_waiting"]==0

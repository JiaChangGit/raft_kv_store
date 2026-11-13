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

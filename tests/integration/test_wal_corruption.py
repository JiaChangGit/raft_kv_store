"""A node whose WAL record header is damaged must refuse to start and change nothing;
the other two nodes keep serving. Offsets follow docs/STORAGE_FORMAT_V2.md."""
import json
import struct
from tools.cluster import Cluster
from tools.kv_client import request
from tools.linearizability import check

FILE_HEADER=80
LENGTH_FIELD=8

def test_corrupt_header_node_refuses_start_and_cluster_continues(tmp_path):
    with Cluster(tmp_path) as c:
        assert c.invoke(request(301,1,"Put","k","before"))["status"]=="OK"
        leader=c.wait_leader()
        victim=next(n for n in (1,2,3) if n!=leader)
        c.wait(lambda:c.status(victim)["durable_index"]>=2)
        c.kill(victim)

        wal=tmp_path/f"node-{victim}"/"raft.wal"
        original=wal.read_bytes()
        assert original[FILE_HEADER:FILE_HEADER+4]==b"RFTR"
        # The corruption reproduced against format 1: the first record now claims
        # to be longer than the whole file. There it was cut off as an unfinished tail.
        length=struct.unpack_from("<I",original,FILE_HEADER+LENGTH_FIELD)[0]
        damaged=bytearray(original)
        struct.pack_into("<I",damaged,FILE_HEADER+LENGTH_FIELD,length+65536)
        assert FILE_HEADER+24+length+65536+4>len(damaged)
        wal.write_bytes(damaged)

        trace=tmp_path/f"node-{victim}.jsonl"
        trace_before=trace.read_bytes()
        refused=c.start_node(victim)
        assert refused.wait(timeout=5)!=0
        assert wal.read_bytes()==bytes(damaged), "a refused WAL must not be modified"
        assert not (wal.parent/"torn").exists()
        assert "header checksum" in (tmp_path/f"node-{victim}.stderr").read_text()
        assert trace.read_bytes()==trace_before, "the refused node never ran its actor"

        # Two of three are still a majority.
        assert c.invoke(request(301,2,"Put","k","during"))["status"]=="OK"
        assert c.invoke(request(302,1,"Get","k"))["result"]=="during"

        # Putting the original bytes back shows the refusal lost nothing.
        wal.write_bytes(original)
        c.start_node(victim)
        done=c.invoke(request(302,2,"Get","k"))
        assert done["status"]=="OK" and done["result"]=="during"
        c.wait(lambda:c.status(victim)["last_applied"]>=done["committed_index"])
        assert c.status(victim)["kv"]["k"]=="during"

        result=check(c.history)
        (c.directory/"checker.json").write_text(json.dumps(result,indent=2))
        assert result["status"]=="PASS",(result,c.history)

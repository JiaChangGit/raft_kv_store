import json
import socket
import struct
import time
from tools.cluster import Cluster
from tools.kv_client import call,request,read_frame,write_frame

def test_sync_failure_cannot_send_granted_vote(tmp_path):
    c=Cluster(tmp_path)
    try:
        c.proxy.start()
        c.start_node(1,env={"RAFT_SYNC_FAILURE":"1"})
        c.wait(lambda:c.status(1))
        port=next(port for a,b,port,_ in c.routes if a==2 and b==1)
        vote={"version":1,"cluster_id":"raft-kv-v1","from":2,"to":1,"rpc_id":991,"body":{"RequestVote":{"term":7,"candidate_id":2,"last_log_index":0,"last_log_term":0}}}
        response=None
        try: response=call(f"127.0.0.1:{port}",vote,timeout=2)
        except (EOFError,OSError): pass
        assert response is None or not response["body"]["VoteResponse"]["granted"]
        assert c.processes[1].wait(timeout=3)!=0
        trace=[json.loads(line) for line in (tmp_path/"node-1.jsonl").read_text().splitlines()]
        assert any(line["kind"]=="storage_failure" for line in trace)
    finally: c.close()

def test_real_peer_accepts_multiple_fragmented_frames(tmp_path):
    with Cluster(tmp_path) as c:
        target=next(n for n in (1,2,3) if n!=c.wait_leader())
        source=next(n for n in (1,2,3) if n!=target)
        port=next(port for a,b,port,_ in c.routes if a==source and b==target)
        with socket.create_connection(("127.0.0.1",port),timeout=2) as sock:
            sock.settimeout(2)
            # Stale valid requests avoid perturbing the real leader term.
            for serial in (888,889):
                message={"version":1,"cluster_id":"raft-kv-v1","from":source,"to":target,"rpc_id":serial,"body":{"RequestVote":{"term":0,"candidate_id":source,"last_log_index":0,"last_log_term":0}}}
                payload=json.dumps(message).encode()
                packet=struct.pack("!I",len(payload))+payload
                for i in range(0,len(packet),3): sock.sendall(packet[i:i+3])
                response=read_frame(sock)
                assert response["rpc_id"]==serial
                assert response["body"]["VoteResponse"]["granted"] is False

def test_all_nodes_restart_relearn_commit_before_serving(tmp_path):
    with Cluster(tmp_path) as c:
        assert c.invoke(request(200,1,"Put","restart","retained"))["status"]=="OK"
        for n in (1,2,3): c.kill(n)
        for n in (1,2,3): c.start_node(n)
        c.wait_leader()
        assert c.invoke(request(200,2,"Get","restart"))["result"]=="retained"

def test_client_wire_version_bounds_and_full_u128(tmp_path):
    with Cluster(tmp_path) as c:
        leader=c.wait_leader()
        for payload in [dict(request(1,0,"Get","x"),version=2),
                        {"client_id":1,"sequence":0,"operation":{"Get":{"key":"x"}}},
                        dict(request(1,0,"Get","x"),unexpected=True),
                        request(1,0,"Put","x","v"*4097),request(1,0,"Get","界"*86)]:
            assert call(c.address(leader),payload)["status"]=="INVALID_ARGUMENT"
        assert c.invoke(request(2**128-1,0,"Put","界"*85,"max-id"))["status"]=="OK"
        assert c.invoke(request(2**128-1,1,"Get","界"*85))["result"]=="max-id"

import os
from tools.cluster import Cluster

def test_three_process_election_and_restart(tmp_path):
    with Cluster(tmp_path) as c:
        pids=[p.pid for p in c.processes.values()]
        assert len(set(pids))==3
        assert all(os.path.exists(f"/proc/{pid}") for pid in pids)
        old=c.wait_leader()
        term=c.status(old)["term"]
        c.kill(old)
        new=c.wait_leader(exclude=[old])
        assert new!=old and c.status(new)["term"]>term
        c.start_node(old)
        c.wait(lambda:c.status(old)["term"]>=c.status(new)["term"])
        assert c.processes[old].pid not in pids

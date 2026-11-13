"""S4 real-process tests: membership changes (docs/MEMBERSHIP.md §15)."""
import concurrent.futures
import json
import subprocess
import sys
import threading
import time
from tools.cluster import Cluster, ROOT
from tools.kv_client import request
from tools.linearizability import check


def test_grow_to_five_and_back_to_three_stays_linearizable(tmp_path):
    # The checker explores at most 24 operations: three clients, seven each.
    per_client = 7
    with Cluster(tmp_path, nodes=3, capacity=5) as c:
        for n in (4, 5):
            c.start_node(n)
        stop = threading.Event()

        def workload(client):
            for sequence in range(1, per_client + 1):
                if stop.is_set():
                    return
                key = f"k{client % 2}"
                op = request(client, sequence, "Put", key, str(sequence)) if sequence % 2 else request(client, sequence, "Get", key)
                c.invoke(op, timeout=5)
                time.sleep(.2)

        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            clients = [pool.submit(workload, client) for client in (401, 402, 403)]
            began = time.monotonic()
            try:
                for kind, node, voters in [("Add", 4, [1, 2, 3, 4]), ("Add", 5, [1, 2, 3, 4, 5]),
                                           ("Remove", 4, [1, 2, 3, 5]), ("Remove", 5, [1, 2, 3])]:
                    answer = c.change(kind, node)
                    assert answer["status"] == "OK", (kind, node, answer)
                    assert answer["result"] == ",".join(map(str, voters))
            finally:
                ended = time.monotonic()
            for future in clients:
                future.result()
        assert c.voters() == [1, 2, 3]
        overlapping = [h for h in c.history if h["invocation_time"] < ended and h["response_time"] > began]
        assert len(overlapping) >= 3, "operations ran while the configuration changed"
        result = check(c.history)
        (c.directory / "checker.json").write_text(json.dumps(result, indent=2))
        assert result["status"] == "PASS", result


def test_removing_the_leader_elects_another_and_it_stops_campaigning(tmp_path):
    with Cluster(tmp_path) as c:
        assert c.invoke(request(411, 1, "Put", "x", "before"))["status"] == "OK"
        old = c.wait_leader()
        rest = [n for n in (1, 2, 3) if n != old]
        answer = c.change("Remove", old)
        assert answer["status"] == "OK" and answer["result"] == ",".join(map(str, rest)), answer
        new = c.wait_leader(exclude=[old])
        assert new in rest
        assert c.invoke(request(411, 2, "Put", "x", "after"))["status"] == "OK"
        # It committed its own removal, so it knows it was removed (§9.2).
        before = c.status(old)
        assert (before["member"], before["config_committed"], before["role"]) == (False, True, "Follower")
        time.sleep(2)
        assert c.status(old)["term"] == before["term"], "it started no election"


def test_a_new_node_catches_up_from_a_snapshot(tmp_path):
    with Cluster(tmp_path, capacity=4, snapshot_every=2) as c:
        for i in range(10):
            assert c.invoke(request(421, i + 1, "Put", f"k{i}", "v"))["status"] == "OK"
        leader = c.wait_leader()
        c.wait(lambda: c.status(leader)["snapshot_index"] > 1)
        c.start_node(4)
        assert c.change("Add", 4)["status"] == "OK"
        c.wait(lambda: c.status(4)["snapshots_installed"] >= 1 and c.status(4)["kv"].get("k9") == "v")
        assert c.status(4)["member"] is True


def test_adding_an_unreachable_node_fails_and_changes_nothing(tmp_path):
    # Node 4 is in the address book but never started: a port nobody listens on.
    with Cluster(tmp_path, capacity=4) as c:
        start = time.monotonic()
        answer = c.change("Add", 4)
        took = time.monotonic() - start
        assert answer["status"] == "CATCH_UP_FAILED", answer
        assert 9 < took < 16, took
        assert c.voters() == [1, 2, 3]
        assert c.invoke(request(431, 1, "Put", "x", "fine"))["status"] == "OK"


def test_a_removed_node_that_keeps_running_does_not_disturb(tmp_path):
    with Cluster(tmp_path) as c:
        leader = c.wait_leader()
        victim = next(n for n in (1, 2, 3) if n != leader)
        assert c.change("Remove", victim)["status"] == "OK"
        term = c.status(leader)["term"]
        time.sleep(3)
        # It never received its removal, so it keeps campaigning (§4.2.3)...
        assert c.status(victim)["term"] > term, "the removed node did try to disrupt"
        # ...but the leader and its follower ignore it (§9.4).
        assert c.wait_leader() == leader and c.status(leader)["term"] == term
        assert c.invoke(request(441, 1, "Put", "x", "undisturbed"))["status"] == "OK"


def test_replacing_a_failed_node(tmp_path):
    with Cluster(tmp_path, capacity=4) as c:
        assert c.invoke(request(451, 1, "Put", "x", "one"))["status"] == "OK"
        leader = c.wait_leader()
        failed = next(n for n in (1, 2, 3) if n != leader)
        c.kill(failed)
        c.start_node(4)
        # Add before remove (dissertation §4.4): one failure is tolerated throughout.
        assert c.change("Add", 4)["status"] == "OK"
        assert c.change("Remove", failed)["status"] == "OK"
        assert c.voters() == sorted(n for n in (1, 2, 3, 4) if n != failed)
        assert c.invoke(request(451, 2, "Put", "x", "two"))["status"] == "OK"
        c.wait(lambda: c.status(4)["kv"].get("x") == "two")


def test_five_nodes_survive_an_isolated_leader_through_the_proxies(tmp_path):
    with Cluster(tmp_path, nodes=5) as c:
        assert len(c.routes) == 20 and c.voters() == [1, 2, 3, 4, 5]
        assert c.invoke(request(461, 1, "Put", "x", "five"))["status"] == "OK"
        old = c.wait_leader()
        c.proxy.isolate(old)
        new = c.wait_leader(exclude=[old])
        assert c.invoke(request(461, 2, "Put", "x", "four"), node=new)["status"] == "OK"
        assert c.invoke(request(462, 1, "Put", "y", "lost"), node=old, timeout=.6)["status"] != "OK"
        c.proxy.heal()
        c.wait(lambda: c.status(old)["role"] == "Follower" and c.status(old)["kv"].get("x") == "four")
        frames = {(e["source"], e["target"]) for e in c.proxy.events if e["kind"] == "frame"}
        assert all((new, n) in frames for n in (1, 2, 3, 4, 5) if n != new), frames


def test_bootstrap_is_ignored_on_a_directory_with_data(tmp_path):
    with Cluster(tmp_path) as c:
        assert c.invoke(request(471, 1, "Put", "x", "kept"))["status"] == "OK"
        log = c.directory / "node-1.jsonl"
        assert '"outcome":"bootstrapped"' in log.read_text()
        c.kill(1)
        c.start_node(1, bootstrap=True)
        c.wait(lambda: c.status(1)["voters"] == [1, 2, 3] and c.status(1)["kv"].get("x") == "kept")
        assert '"outcome":"ignored_existing_data"' in log.read_text()


def test_admin_tool_adds_removes_and_warns(tmp_path):
    with Cluster(tmp_path, capacity=4) as c:
        c.start_node(4)

        def admin(*args):
            done = subprocess.run([sys.executable, str(ROOT / "tools/kv_admin.py"), "--config", str(c.config), *args],
                                  capture_output=True, text=True, timeout=90)
            return done.returncode, done.stdout, done.stderr

        code, out, _ = admin("status")
        assert code == 0 and json.loads(out)["voters"] == [1, 2, 3]
        code, out, _ = admin("add", "4")
        assert code == 0 and json.loads(out)["result"] == "1,2,3,4"
        code, out, err = admin("remove", "4")
        assert code == 0 and json.loads(out)["result"] == "1,2,3" and "warning" not in err
        leader = c.wait_leader()
        follower = next(n for n in (1, 2, 3) if n != leader)
        code, out, err = admin("remove", str(follower))
        assert code == 0 and "cannot tolerate a failure" in err

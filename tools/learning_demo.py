#!/usr/bin/env python3
"""實跑隔離的三節點 PUT/GET、Leader 故障與恢復；保存實際輸出。"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

from tools.cluster import Cluster, ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path,
                        help="尚不存在的輸出目錄；內含測試資料與執行紀錄")
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    events = []
    summary = {"started_utc": datetime.now(timezone.utc).isoformat(),
               "status": "INCOMPLETE", "events": events,
               "limits": ["單機 WSL，短時操作，不是完整安全性證明",
                          "節點各自的 JSONL 只有檔案內順序，不能據此重建跨程序的精確時序",
                          "為防止測試繞過 proxy，示範使用六條方向性本機 proxy 路徑"]}

    def record(kind, **data):
        item = {"step": len(events) + 1, "kind": kind, **data}
        events.append(item)
        print(json.dumps(item, ensure_ascii=False), flush=True)
        (output / "result.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")

    try:
        with Cluster(output / "cluster") as cluster:
            initial_pids = {str(node): proc.pid for node, proc in cluster.processes.items()}
            leader = cluster.wait_leader()
            before = cluster.status(leader)
            record("cluster_started", pids=initial_pids, leader=leader, status=before)

            def client(sequence, operation, value=None):
                argv = [sys.executable, "-m", "tools.kv_client"]
                for node in (1, 2, 3):
                    argv += ["--address", cluster.address(node)]
                argv += ["--client-id", "701", "--sequence", str(sequence), operation, "lesson-x"]
                if value is not None:
                    argv.append(value)
                started = time.monotonic()
                run = subprocess.run(argv, cwd=ROOT, text=True, capture_output=True, timeout=20)
                try:
                    result = json.loads(run.stdout)
                except json.JSONDecodeError:
                    record("client_invalid_output", argv=argv, exit_code=run.returncode,
                           stdout=run.stdout, stderr=run.stderr)
                    raise
                record("client", argv=argv, exit_code=run.returncode, stdout=run.stdout.strip(),
                       stderr=run.stderr, elapsed_seconds=time.monotonic() - started, result=result)
                if run.returncode or result["status"] != "OK":
                    raise RuntimeError(f"client 失敗：{result}")
                return result

            put = client(0, "Put", "A")
            get = client(1, "Get")
            if put["result"] is not None or get["result"] != "A":
                raise AssertionError("PUT 的舊值或 GET 的值不符預期")
            log = output / "cluster" / f"node-{leader}.jsonl"
            rows = [json.loads(line) for line in log.read_text().splitlines()]
            positions = {
                "propose": next(i + 1 for i, row in enumerate(rows)
                                if row["kind"] == "event" and row["details"].get("event") == "propose"
                                and row["details"].get("client_id") == "701" and row["details"].get("sequence") == 0),
                "persist": next(i + 1 for i, row in enumerate(rows)
                                if row["kind"] == "persist" and row["last_log_index"] == put["committed_index"]),
                "apply": next(i + 1 for i, row in enumerate(rows)
                              if row["kind"] == "apply" and row["details"].get("index") == put["committed_index"]),
            }
            if not positions["propose"] < positions["persist"] < positions["apply"]:
                raise AssertionError(f"單節點 trace 順序不符：{positions}")
            record("put_trace", node=leader, path=str(log.relative_to(output)), lines=positions,
                   committed_index=put["committed_index"])

            cluster.kill(leader)
            rc = cluster.processes[leader].returncode
            if rc != -9:
                raise AssertionError(f"Leader 非 SIGKILL 結束：{rc}")
            record("leader_killed", node=leader, pid=initial_pids[str(leader)], returncode=rc)
            new_leader = cluster.wait_leader(exclude=[leader])
            after = cluster.status(new_leader)
            if after["term"] <= before["term"]:
                raise AssertionError("新 Leader term 未增加")
            record("new_leader", node=new_leader, status=after)
            after_get = client(2, "Get")
            if after_get["result"] != "A":
                raise AssertionError("換 Leader 後讀取值不符")

            restarted = cluster.start_node(leader)
            cluster.wait(lambda: all(cluster.status(n)["last_applied"] >= after_get["committed_index"]
                                     for n in (1, 2, 3)))
            states = {str(n): cluster.status(n) for n in (1, 2, 3)}
            if any(state["kv"].get("lesson-x") != "A" for state in states.values()):
                raise AssertionError("重啟後 debug state 未收斂")
            record("restarted_and_caught_up", node=leader, new_pid=restarted.pid,
                   previous_pid=initial_pids[str(leader)], nodes=states)
        summary["status"] = "PASS"
    except Exception as error:
        summary["status"] = "FAIL"
        summary["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        summary["finished_utc"] = datetime.now(timezone.utc).isoformat()
        summary["source_sha256"] = {
            str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [ROOT / "tools/learning_demo.py", ROOT / "tools/cluster.py",
                         ROOT / "tools/kv_client.py", ROOT / "crates/raft-node/src/main.rs",
                         ROOT / "crates/raft-core/src/lib.rs"]}
        (output / "result.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    print(f"結果：{output / 'result.json'}")


if __name__ == "__main__":
    main()

"""The hands-on demo (docs/HANDS_ON.md) end to end. Until 2026-11-17 nothing ran
it, and it had been failing since S5 changed the trace: the node now records a
proposed batch, not each client's write, and the demo looked for the latter."""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def test_the_hands_on_demo_runs_to_the_end(tmp_path):
    output = tmp_path / "demo"
    run = subprocess.run([sys.executable, "-m", "tools.learning_demo", "--output-dir", str(output)],
                         cwd=ROOT, capture_output=True, text=True, timeout=180)
    result = json.loads((output / "result.json").read_text())
    assert run.returncode == 0 and result["status"] == "PASS", (result.get("error"), run.stderr[-2000:])
    kinds = [event["kind"] for event in result["events"]]
    assert kinds[-4:] == ["leader_killed", "new_leader", "client", "restarted_and_caught_up"], kinds

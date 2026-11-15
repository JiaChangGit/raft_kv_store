"""Lesson 17 helper: a report shaped like the real write-benchmark.json, checked
by the S5 checker (e5f09c2) and by the current one against the phase 14 rule."""
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
work = Path(sys.argv[1])


def load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


old = load(work / "scripts/verify_gate.py", "old_gate")  # its ROOT is the work directory
new = load(ROOT / "scripts/verify_gate.py", "new_gate")  # its ROOT is the project
rules = json.loads((work / "final.json").read_text())["reports"]["14"]
rule = next(r for r in rules if r["path"].endswith("write-benchmark.json"))
print("phase 14 rule:", json.dumps({key: rule[key] for key in ("required_nonempty", "required_positive_integer")}))
report = work / "artifacts/reports/write-benchmark.json"  # inside both checkers' ROOT
report.parent.mkdir(parents=True)
shaped = {"command": "python scripts/write_benchmark.py", "source_fingerprint": "abc", "writes_per_run": 3000,
          "environment": {"cpu_count": 16}, "runs": [{"label": "before"}, {"label": "after"}]}
for label, gate in (("S5 checker (e5f09c2)", old), ("current checker", new)):
    print(f"\n== {label}")
    for name, runs in (("runs holds two entries", shaped["runs"]), ("runs is empty", [])):
        report.write_text(json.dumps({**shaped, "runs": runs}))
        try:
            gate.validate_report(report, rule, "abc")
            print(f"   {name}: accepted")
        except ValueError as error:
            print(f"   {name}: REFUSED ({str(error).replace(str(report), 'write-benchmark.json')})")
print("\n== RESULT")
print("The S5 checker refuses every report, a good one too: the rule could never be met. The current one refuses only the empty list.")

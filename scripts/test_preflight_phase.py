"""The preflight applies a phase's report rules to the reports on disk (S5 review F7)."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent))
spec = importlib.util.spec_from_file_location("preflight_phase", Path(__file__).with_name("preflight_phase.py"))
preflight = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preflight)


class ReportPreflightTests(unittest.TestCase):
    def setUp(self):
        # Under the project root (validate_report names reports relative to it),
        # in artifacts/, which the source fingerprint leaves out.
        parent = preflight.ROOT / "artifacts/audit/preflight-unit"
        parent.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=parent)
        self.addCleanup(self.temporary.cleanup)
        self.path = Path(self.temporary.name).relative_to(preflight.ROOT) / "write-benchmark.json"
        self.contract = {"reports": {"14": [{"path": self.path.as_posix(), "required_nonempty": ["command", "runs"],
                                             "required_positive_integer": ["writes_per_run"],
                                             "source_fingerprint_must_match": True}]}}
        self.report = {"command": "python scripts/write_benchmark.py", "source_fingerprint": "old",
                       "writes_per_run": 3000, "runs": [{"label": "before"}, {"label": "after"}]}

    def write(self, report):
        (preflight.ROOT / self.path).write_text(json.dumps(report))

    def test_a_rule_the_report_breaks_is_a_problem(self):
        self.write({**self.report, "runs": []})
        problems, _ = preflight.check_reports(self.contract, "14", "old")
        self.assertEqual(len(problems), 1)
        self.assertIn("runs must be nonempty", problems[0])

    def test_a_list_valued_runs_passes_and_another_source_is_only_a_note(self):
        self.write(self.report)
        problems, notes = preflight.check_reports(self.contract, "14", "current")
        self.assertEqual(problems, [])
        self.assertEqual(len(notes), 1)
        self.assertIn("regenerate", notes[0])

    def test_a_missing_report_is_a_note(self):
        problems, notes = preflight.check_reports(self.contract, "14", "current")
        self.assertEqual(problems, [])
        self.assertIn("missing", notes[0])

    def test_earlier_phases_rules_apply_and_later_ones_do_not(self):
        # The checker applies every phase's report rules up to the one accepted;
        # some phases give a single rule instead of a list.
        self.write({**self.report, "runs": []})
        contract = {"reports": {"07": self.contract["reports"]["14"][0], "15": [{"path": "nowhere.json"}]}}
        problems, notes = preflight.check_reports(contract, "14", "old")
        self.assertEqual(len(problems), 1)
        self.assertTrue(problems[0].startswith("P07 "))
        self.assertEqual(notes, [])


if __name__ == "__main__":
    unittest.main()

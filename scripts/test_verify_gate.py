"""Small negative tests for the acceptance parser, independent of Cargo."""
import importlib.util
from pathlib import Path
import unittest
import json
import shutil
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location("verify_gate", Path(__file__).with_name("verify_gate.py"))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class DiscoveryTests(unittest.TestCase):
    def test_missing_and_ambiguous_names_fail(self):
        for listing in ("0 tests, 0 benchmarks\n", "a::required: test\nb::required: test\n"):
            with self.assertRaises(ValueError):
                gate.discover_rust(listing, ["required"])

    def test_exact_leaf_not_substring(self):
        self.assertEqual(gate.discover_rust("m::required: test\nm::required_extra: test\n", ["required"]), {"required": "m::required"})

    def test_zero_or_ignored_is_not_success(self):
        for output in ("test result: ok. 0 passed; 0 failed; 0 ignored;", "test m::required ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored;"):
            with self.assertRaises(ValueError):
                gate.require_rust_execution(output, "m::required")

    def test_actual_exact_execution_is_accepted(self):
        output = "test m::required ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\ntest result: ok. 0 passed; 0 failed; 0 ignored;"
        self.assertEqual(gate.require_rust_execution(output, "m::required")["passed"], 1)


class EvidenceBoundaryTests(unittest.TestCase):
    def setUp(self):
        parent = gate.ROOT / "artifacts/audit/gate-unit"
        parent.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=parent)
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def test_report_rejects_wrong_types_and_inconsistent_counts(self):
        report = self.directory / "report.json"
        contract = json.loads(gate.CONTRACT.read_text())
        bad_benchmark = {"command": 7, "run_count": 1, "metrics": "wrong", "environment": ["wrong"]}
        report.write_text(json.dumps(bad_benchmark))
        with self.assertRaises(ValueError):
            gate.validate_report(report, contract["reports"]["08"])
        bad_simulation = {"command": "run", "seed_count": 10, "event_count": 100,
                          "pass_count": 11, "failures": []}
        report.write_text(json.dumps(bad_simulation))
        with self.assertRaises(ValueError):
            gate.validate_report(report, contract["reports"]["07"])

    def test_snapshot_rejects_file_and_directory_symlinks(self):
        target = self.directory / "target"
        target.mkdir()
        (target / "source.rs").write_text("initial")
        for directory_link in (False, True):
            root = self.directory / str(directory_link)
            root.mkdir()
            try:
                (root / "linked").symlink_to(target if directory_link else target / "source.rs",
                                            target_is_directory=directory_link)
            except OSError as error:
                self.skipTest(f"platform cannot create source symlink: {error}")
            with self.assertRaises(ValueError, msg=str(directory_link)):
                gate.snapshot(root)

    def test_snapshot_error_replaces_previous_passing_latest_receipt(self):
        root = self.directory / "workspace"
        (root / "scripts").mkdir(parents=True)
        (root / "acceptance").mkdir()
        (root / "artifacts/acceptance-v1").mkdir(parents=True)
        shutil.copy2(Path(gate.__file__), root / "scripts/verify_gate.py")
        shutil.copy2(gate.CONTRACT, root / "acceptance/v1.json")
        previous = root / "artifacts/acceptance-v1/phase-00.last-attempt.json"
        previous.write_text(json.dumps({"status": "PASS"}))
        payload = self.directory / "payload.rs"
        payload.write_text("outside workspace snapshot")
        try:
            (root / "linked.rs").symlink_to(payload)
        except OSError as error:
            self.skipTest(f"platform cannot create source symlink: {error}")
        run = subprocess.run([sys.executable, "-B", "scripts/verify_gate.py", "00"], cwd=root,
                             capture_output=True, timeout=10)
        latest = json.loads(previous.read_text())
        self.assertEqual(run.returncode, 1, run.stderr)
        self.assertEqual(latest["status"], "FAIL")
        self.assertIn("symlink", latest["error"])
        self.assertTrue((root / latest["evidence_directory"] / "result.json").exists())


if __name__ == "__main__":
    unittest.main()

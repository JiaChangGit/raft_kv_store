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

    def test_report_must_come_from_the_accepted_source_and_name_detected_cases(self):
        report = self.directory / "mutations.json"
        schema = {"required_nonempty": ["command"], "required_positive_integer": ["case_count", "detected_count"],
                  "required_empty_array": ["undetected"], "source_fingerprint_must_match": True,
                  "required_detected_cases": ["first", "second"]}
        good = {"command": "run", "case_count": 2, "detected_count": 2, "undetected": [], "source_fingerprint": "abc",
                "cases": [{"case": "first", "detected": True}, {"case": "second", "detected": True}]}
        report.write_text(json.dumps(good))
        self.assertTrue(gate.validate_report(report, schema, "abc")["schema_valid"])
        for fingerprint in ("other", None):
            with self.assertRaises(ValueError):
                gate.validate_report(report, schema, fingerprint)
        for broken in ({**good, "cases": [{"case": "first", "detected": True}]},
                       {**good, "cases": [{"case": "first", "detected": True}, {"case": "second", "detected": False}]},
                       {**good, "cases": "first second"},
                       {**good, "undetected": ["second"]},
                       {key: value for key, value in good.items() if key != "source_fingerprint"}):
            report.write_text(json.dumps(broken))
            with self.assertRaises(ValueError):
                gate.validate_report(report, schema, "abc")

    def test_nonempty_keys_have_the_type_the_report_uses(self):
        # S5 review F7: the phase 14 contract asked for a nonempty `runs` list, which the
        # checker read as a string, so phase 14 could never pass.
        report = self.directory / "typed.json"
        schema = {"required_nonempty": ["command", "environment", "runs"]}
        good = {"command": "run", "environment": {"cpu_count": 1}, "runs": [{"label": "before"}]}
        report.write_text(json.dumps(good))
        self.assertTrue(gate.validate_report(report, schema)["schema_valid"])
        for key, bad in (("runs", []), ("runs", "before"), ("runs", {}), ("environment", {}), ("environment", ["x"]),
                         ("command", " "), ("command", ["run"])):
            report.write_text(json.dumps({**good, key: bad}))
            with self.assertRaises(ValueError, msg=f"{key}={bad!r}"):
                gate.validate_report(report, schema)

    def test_the_phase_14_write_benchmark_schema_accepts_a_report_shaped_like_the_real_one(self):
        contract = json.loads(gate.CONTRACT.read_text())
        schema = next(s for s in contract["reports"]["14"] if s["path"].endswith("write-benchmark.json"))
        report = self.directory / "write-benchmark.json"
        shaped = {"command": "python scripts/write_benchmark.py", "source_fingerprint": "abc", "writes_per_run": 3000,
                  "environment": {"cpu_count": 16}, "runs": [{"label": "before"}, {"label": "after"}]}
        report.write_text(json.dumps(shaped))
        self.assertTrue(gate.validate_report(report, schema, "abc")["schema_valid"])
        report.write_text(json.dumps({**shaped, "runs": []}))
        with self.assertRaises(ValueError):
            gate.validate_report(report, schema, "abc")

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
        (root / gate.RECEIPTS).mkdir(parents=True)
        shutil.copy2(Path(gate.__file__), root / "scripts/verify_gate.py")
        shutil.copy2(gate.CONTRACT, root / "acceptance" / gate.CONTRACT.name)
        previous = root / gate.RECEIPTS / "phase-00.last-attempt.json"
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


class ContractContinuityTests(unittest.TestCase):
    """Every property accepted by the historical v1 contract must stay named in the final one."""

    def setUp(self):
        self.final = json.loads(gate.CONTRACT.read_text())
        self.v1 = json.loads((gate.ROOT / "acceptance/v1.json").read_text())

    def test_every_v1_test_is_mapped_to_existing_final_tests(self):
        final_tests = {name: phase for phase, body in self.final["phases"].items()
                       for name in body["rust_tests"] + body["pytest_nodes"]}
        for body in self.v1["phases"].values():
            for name in body["rust_tests"] + body["pytest_nodes"]:
                entry = self.final["v1_mapping"].get(name)
                self.assertIsNotNone(entry, f"v1 test dropped without mapping: {name}")
                self.assertTrue(entry["final_tests"], name)
                self.assertTrue(entry["note"].strip(), name)
                for target in entry["final_tests"]:
                    self.assertIn(target, final_tests, f"{name} maps to a test not in any final phase")

    def test_v1_reports_and_required_files_are_not_dropped(self):
        final_files = {name for body in self.final["phases"].values() for name in body["required_files"]}
        waived = self.final.get("v1_required_files_replaced", {})
        for body in self.v1["phases"].values():
            for name in body["required_files"]:
                self.assertTrue(name in final_files or waived.get(name, "").strip(), name)
        for phase, schema in self.v1["reports"].items():
            self.assertEqual(self.final["reports"].get(phase), schema)

    def test_replaced_tests_name_replacements_that_still_run(self):
        final_tests = {name for body in self.final["phases"].values()
                       for name in body["rust_tests"] + body["pytest_nodes"]}
        for name, entry in self.final.get("replaced_tests", {}).items():
            self.assertNotIn(name, final_tests, f"{name} is replaced but still listed")
            self.assertTrue(entry["note"].strip(), name)
            for target in entry["replaced_by"]:
                self.assertIn(target, final_tests, f"{name} is replaced by a test no phase runs")

    def test_modified_tests_still_run_in_their_phase(self):
        for name, entry in self.final.get("modified_tests", {}).items():
            self.assertTrue(entry["note"].strip(), name)
            phase = self.final["phases"].get(entry["phase"])
            if phase is not None:
                self.assertIn(name, phase["rust_tests"] + phase["pytest_nodes"], name)

    def test_host_configuration_is_outside_fingerprint_but_sources_are_not(self):
        self.assertTrue({".claude", ".agents"} <= gate.EXCLUDED)
        self.assertFalse({"docs", "scripts", "crates", "tests", "tools", "acceptance"} & gate.EXCLUDED)


if __name__ == "__main__":
    unittest.main()

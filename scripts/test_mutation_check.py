"""The mutation report must not mistake output layout or build errors for results."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("mutation_check", Path(__file__).with_name("mutation_check.py"))
mutation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mutation)


class ClassifyTests(unittest.TestCase):
    def test_named_failure_is_detected(self):
        out = "running 1 test\ntest tests::guarded ... FAILED\n\nfailures:\n"
        self.assertEqual(mutation.classify("guarded", out, ""), "detected")

    def test_failure_split_by_trace_output_is_still_detected(self):
        # Observed on 2026-11-13: the node's trace landed inside libtest's lines.
        out = ('test tests::guarded ... {"kind":"rpc_send","to":2}\n'
               "thread 'tests::guarded' (3217) panicked at crates/raft-node/src/main.rs:822:14:\n"
               'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out{"kind":"rpc_send"}\n')
        self.assertEqual(mutation.classify("guarded", out, ""), "detected")
        listed = "failures:\n    tests::guarded\n\ntest result: FAILED\n"
        self.assertEqual(mutation.classify("guarded", listed, ""), "detected")

    def test_other_test_failing_is_not_a_detection(self):
        out = ("test tests::guarded ... ok\ntest tests::guarded_extra ... FAILED\n"
               "thread 'tests::guarded_extra' panicked at x.rs:1:1:\nfailures:\n    tests::guarded_extra\n")
        self.assertEqual(mutation.classify("guarded", out, ""), "survived")

    def test_zero_tests_run_is_not_a_survivor(self):
        out = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n"
        self.assertEqual(mutation.classify("guarded", out, ""), "test_not_run")
        passed = "running 1 test\ntest tests::guarded ... ok\n"
        self.assertEqual(mutation.classify("guarded", passed, ""), "survived")

    def test_build_error_is_reported_as_such(self):
        err = "error[E0061]: this function takes 4 arguments\nerror: could not compile `raft-storage` (lib)\n"
        self.assertEqual(mutation.classify("guarded", "", err), "build_error")

    def test_every_case_anchor_is_unique_in_the_current_source(self):
        for name, file, original, mutated, *_ in mutation.CASES:
            text = (mutation.ROOT / file).read_text()
            self.assertEqual(text.count(original), 1, name)
            self.assertNotEqual(original, mutated, name)


class IsolationTests(unittest.TestCase):
    def test_each_copy_builds_in_its_own_target_directory(self):
        # Lesson 06: a shared CARGO_TARGET_DIR let one mutant's crate leak into the next.
        seen = []

        class Done:
            returncode, stdout, stderr = 0, b"", b""

        def fake_run(command, cwd, env, capture_output, timeout):
            seen.append((Path(cwd), Path(env["CARGO_TARGET_DIR"])))
            return Done()

        import tempfile
        original = mutation.subprocess.run
        mutation.subprocess.run = fake_run
        try:
            with tempfile.TemporaryDirectory() as tmp:
                for name in ("a", "b"):
                    work = Path(tmp) / name
                    work.mkdir()
                    mutation.cargo_test(work, "some_test", "raft-core")
        finally:
            mutation.subprocess.run = original
        (cwd_a, target_a), (cwd_b, target_b) = seen
        self.assertNotEqual(target_a, target_b)
        self.assertEqual(target_a.parent, cwd_a)
        self.assertEqual(target_b.parent, cwd_b)


if __name__ == "__main__":
    unittest.main()

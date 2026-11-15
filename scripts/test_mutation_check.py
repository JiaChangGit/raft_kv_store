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


class JudgeTests(unittest.TestCase):
    """A case killed for time or output did not fail by itself (S5 review F6)."""

    def test_killed_runs_are_never_detections(self):
        # The output may even contain a "FAILED" line from before the hang.
        out = "test tests::guarded ... FAILED\nthread 'tests::guarded' panicked at x.rs:1:1:\n"
        self.assertEqual(mutation.judge("guarded", 124, out, ""), ("timeout", False))
        self.assertEqual(mutation.judge("guarded", 125, out, ""), ("output_cap", False))

    def test_ordinary_failure_is_still_a_detection(self):
        out = "test tests::guarded ... FAILED\n\nfailures:\n    tests::guarded\n"
        self.assertEqual(mutation.judge("guarded", 101, out, ""), ("detected", True))
        self.assertEqual(mutation.judge("guarded", 0, "test tests::guarded ... ok\n", ""), ("survived", False))


class CappedRunTests(unittest.TestCase):
    """capped_run.py keeps memory bounded and leaves no process behind (S5 review F6)."""

    def run_capped(self, timeout, cap, *command):
        import subprocess
        import sys
        import time
        script = Path(__file__).with_name("capped_run.py")
        started = time.monotonic()
        done = subprocess.run([sys.executable, str(script), "--timeout", str(timeout), "--cap", str(cap), "--", *command],
                              capture_output=True, timeout=60)
        return done, time.monotonic() - started

    def test_output_and_exit_status_pass_through(self):
        import sys
        done, _ = self.run_capped(30, 1 << 20, sys.executable, "-c",
                                  "import sys; print('out'); print('err', file=sys.stderr); sys.exit(3)")
        self.assertEqual(done.returncode, 3)
        self.assertEqual(done.stdout.strip(), b"out")
        self.assertEqual(done.stderr.strip(), b"err")

    def test_a_flood_is_cut_at_the_cap_and_only_its_ends_are_kept(self):
        import sys
        flood = "import sys\nwhile True:\n    sys.stdout.write('x' * 65536)\n    sys.stdout.flush()\n"
        done, seconds = self.run_capped(30, 24 << 20, sys.executable, "-c", flood)
        self.assertEqual(done.returncode, 125)
        self.assertLess(seconds, 25)
        # Kept: the first 4 MiB and the last 4 MiB, not the 24 MiB or more that went by.
        self.assertLess(len(done.stdout), 9 << 20)
        self.assertIn(b"bytes omitted", done.stdout)
        self.assertIn(b"output cap", done.stderr)

    def test_the_time_limit_kills_the_command_and_what_it_started(self):
        import os
        import sys
        import tempfile
        import time
        with tempfile.TemporaryDirectory() as tmp:
            pid_file = Path(tmp) / "grandchild.pid"
            parent = ("import subprocess, sys, time\n"
                      f"child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'])\n"
                      f"open({str(pid_file)!r}, 'w').write(str(child.pid))\n"
                      "time.sleep(120)\n")
            done, seconds = self.run_capped(2, 1 << 20, sys.executable, "-c", parent)
            self.assertEqual(done.returncode, 124)
            self.assertLess(seconds, 20)
            grandchild = int(pid_file.read_text())
            for _ in range(50):
                try:
                    os.kill(grandchild, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.1)
            else:
                self.fail("the grandchild outlived capped_run.py")


if __name__ == "__main__":
    unittest.main()

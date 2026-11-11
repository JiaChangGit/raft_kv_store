#!/usr/bin/env python3
"""Alternative acceptance v1, not a reconstruction of the missing original gate."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "acceptance/v1.json"
EXCLUDED = {".git", ".agf", ".venv", "target", "artifacts", "__pycache__", ".pytest_cache"}


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot(root: Path) -> tuple[dict[str, str], str]:
    files = {}
    for base, directories, names in os.walk(root):
        directories[:] = sorted(d for d in directories if d not in EXCLUDED)
        for name in directories:
            if (Path(base) / name).is_symlink():
                raise ValueError(f"Source directory symlink is not captured: {Path(base) / name}")
        for name in sorted(names):
            path = Path(base) / name
            if "Zone.Identifier" in name or name.endswith(".pyc"):
                continue
            if path.is_symlink():
                raise ValueError(f"Source file symlink is not captured: {path}")
            files[path.relative_to(root).as_posix()] = sha(path)
    return files, hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()


def discover_rust(output: str, required: list[str]) -> dict[str, str]:
    discovered = re.findall(r"^([^\r\n]+): test\s*$", output, re.MULTILINE)
    selected = {}
    for leaf in required:
        matches = [name for name in discovered if name.rsplit("::", 1)[-1] == leaf]
        if len(matches) != 1:
            raise ValueError(f"Rust test {leaf!r} must be discovered exactly once; got {matches}")
        selected[leaf] = matches[0]
    return selected


def require_rust_execution(output: str, full_name: str) -> dict[str, int]:
    # libtest may report many zero-test harnesses under --workspace. Only the
    # selected test's explicit success is evidence that the filter ran it.
    if len(re.findall(r"^test " + re.escape(full_name) + r" \.\.\. ok\s*$", output, re.MULTILINE)) != 1:
        raise ValueError(f"Selected Rust test did not run exactly once and pass: {full_name}")
    totals = {"passed": 0, "failed": 0, "ignored": 0}
    for passed, failed, ignored in re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored;", output):
        totals["passed"] += int(passed)
        totals["failed"] += int(failed)
        totals["ignored"] += int(ignored)
    if totals != {"passed": 1, "failed": 0, "ignored": 0}:
        raise ValueError(f"Unexpected selected-test execution counts: {totals}")
    return totals


def validate_report(path: Path, schema: dict) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("Report must be an object")
    for key in schema.get("required_nonempty", []):
        expected = dict if key in {"metrics", "environment"} else str
        if not isinstance(data.get(key), expected) or not data[key] or (expected is str and not data[key].strip()):
            raise ValueError(f"Report {path}: {key} must be nonempty")
    for key in schema.get("required_positive_integer", []):
        value = data.get(key)
        if type(value) is not int or value <= 0:
            raise ValueError(f"Report {path}: {key} must be a positive integer")
    for key in schema.get("required_empty_array", []):
        if not isinstance(data.get(key), list) or data[key] != []:
            raise ValueError(f"Report {path}: {key} must be []")
    if "seed_count" in data and (data.get("pass_count") != data["seed_count"] or data.get("event_count",0) < data["seed_count"]):
        raise ValueError("Simulation report counts are inconsistent")
    return {"path": path.relative_to(ROOT).as_posix(), "sha256": sha(path), "schema_valid": True}


def atomic_json(path: Path, value: dict) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    temporary.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=[f"{n:02}" for n in range(9)])
    parser.add_argument("--timeout", type=int, default=1200, help="Per-command timeout in seconds")
    args = parser.parse_args()
    contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
    contract_hash, checker_hash = sha(CONTRACT), sha(Path(__file__))
    started = datetime.now(timezone.utc).isoformat()
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    out = ROOT / "artifacts/acceptance-v1" / f"P{args.phase}-{run_id}"
    out.mkdir(parents=True, exist_ok=False)
    receipts = out.parent
    result = {"acceptance": contract["identity"], "phase": args.phase,
              "original_gate": "UNAVAILABLE", "status": "FAIL", "started_at": started,
              "contract_sha256": contract_hash, "checker_sha256": checker_hash,
              "source_fingerprint": None, "source_files": {},
              "commands": [], "rust_tests": {}, "pytest_nodes": [], "reports": [],
              "limits": contract["limits"], "evidence_directory": out.relative_to(ROOT).as_posix()}

    def run(argv: list[str], label: str) -> str:
        index = len(result["commands"])
        wall_start = time.monotonic()
        entry = {"argv": argv, "cwd": str(ROOT), "label": label}
        result["commands"].append(entry)
        try:
            done = subprocess.run(argv, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  timeout=args.timeout, check=False)
            stdout, stderr = done.stdout, done.stderr
            code = done.returncode
        except subprocess.TimeoutExpired as exc:
            stdout, stderr, code = exc.stdout or b"", exc.stderr or b"", 124
        except OSError as exc:
            stdout, stderr, code = b"", str(exc).encode(), 127
        stdout_path, stderr_path = out / f"{index:02}-stdout.txt", out / f"{index:02}-stderr.txt"
        stdout_path.write_bytes(stdout)
        stderr_path.write_bytes(stderr)
        entry.update({"exit_code": code, "elapsed_seconds": time.monotonic() - wall_start,
                      "stdout": stdout_path.relative_to(ROOT).as_posix(),
                      "stderr": stderr_path.relative_to(ROOT).as_posix(),
                      "stdout_sha256": sha(stdout_path), "stderr_sha256": sha(stderr_path),
                      "capture_complete": code != 124})
        print(f"[{label}] exit={code}", flush=True)
        output = stdout.decode("utf-8", errors="replace") + "\n" + stderr.decode("utf-8", errors="replace")
        if code:
            print(output[-10000:], flush=True)
            raise ValueError(f"{label} failed with exit {code}; full output saved")
        return output

    try:
        state, fingerprint = snapshot(ROOT)
        result.update(source_fingerprint=fingerprint, source_files=state)
        if args.timeout <= 0:
            raise ValueError("--timeout must be positive")
        manifest=json.loads((ROOT / contract["source_manifest"]).read_text(encoding="utf-8"))
        for name, expected in manifest["originals"].items():
            if sha(ROOT / name) != expected:
                raise ValueError(f"Original supplied material changed: {name}")
        result["original_materials_verified"] = len(manifest["originals"])
        if args.phase != "00":
            previous = f"{int(args.phase)-1:02}"
            previous_path = receipts / f"phase-{previous}.last-attempt.json"
            previous_data = json.loads(previous_path.read_text(encoding="utf-8"))
            if previous_data.get("status") != "PASS" or previous_data.get("contract_sha256") != contract_hash or previous_data.get("checker_sha256") != checker_hash:
                raise ValueError(f"Previous phase {previous} lacks a passing receipt for this acceptance/checker version")
            result["previous_receipt"] = {"path": previous_path.relative_to(ROOT).as_posix(), "sha256": sha(previous_path)}
        phases = [contract["phases"][f"{n:02}"] for n in range(int(args.phase) + 1)]
        rust_tests = [name for phase in phases for name in phase["rust_tests"]]
        pytest_nodes = [name for phase in phases for name in phase["pytest_nodes"]]
        for phase in phases:
            for name in phase["required_files"]:
                path = ROOT / name
                if not path.is_file() or path.stat().st_size == 0:
                    raise ValueError(f"Required nonempty file missing: {name}")
        listing = run(["cargo", "test", "--workspace", "--", "--list"], "Rust discovery")
        selected = discover_rust(listing, rust_tests)
        for argv in contract["common_commands"]:
            run(argv, " ".join(argv))
        for leaf, full in selected.items():
            output = run(["cargo", "test", "--workspace", full, "--", "--exact", "--nocapture"], leaf)
            result["rust_tests"][leaf] = {"discovered_name": full, **require_rust_execution(output, full)}
        if pytest_nodes:
            run(["cargo", "build", "--workspace", "--bins"], "Build real process executables")
            collection = run([sys.executable, "-m", "pytest", "--collect-only", "-q", *pytest_nodes], "pytest discovery")
            for node in pytest_nodes:
                if node not in collection.splitlines():
                    raise ValueError(f"pytest node not collected exactly: {node}")
                junit = out / f"pytest-{len(result['pytest_nodes']):02}.xml"
                run([sys.executable, "-m", "pytest", "-q", node, f"--junitxml={junit}"], node)
                import xml.etree.ElementTree as ET
                suites = ET.parse(junit).getroot()
                cases = list(suites.iter("testcase"))
                if len(cases) != 1 or any(case.find(tag) is not None for case in cases for tag in ("failure", "error", "skipped")):
                    raise ValueError(f"pytest node must execute exactly one unskipped passing case: {node}")
                result["pytest_nodes"].append({"node": node, "passed": 1, "junit": junit.relative_to(ROOT).as_posix(), "sha256": sha(junit)})
        for phase, schema in contract["reports"].items():
            if int(phase) <= int(args.phase):
                result["reports"].append(validate_report(ROOT / schema["path"], schema))
        _, after = snapshot(ROOT)
        if after != fingerprint:
            raise ValueError("Source changed during acceptance; preserve diagnostics and rerun after changes settle")
        result["status"] = "PASS"
        result["meaning"] = "Alternative acceptance v1 passed on this source; human assertion/protocol review remains separate."
    except (ValueError, OSError, KeyError, json.JSONDecodeError) as exc:
        result["error"] = str(exc)
    finally:
        result["finished_at"] = datetime.now(timezone.utc).isoformat()
        atomic_json(out / "result.json", result)
        atomic_json(receipts / f"phase-{args.phase}.last-attempt.json", result)
        if result["status"] == "PASS":
            atomic_json(receipts / f"phase-{args.phase}.json", result)
    print(json.dumps({key: value for key, value in result.items() if key not in {"source_files", "commands"}}, indent=2, ensure_ascii=False))
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())

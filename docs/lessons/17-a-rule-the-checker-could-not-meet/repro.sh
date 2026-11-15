#!/usr/bin/env bash
# Lesson 17: the phase 14 contract required write-benchmark.json's "runs" to be
# nonempty, but the S5 checker (e5f09c2) read every such key except metrics and
# environment as a string, so a list never counted as nonempty. One well-formed
# report, checked by both checkers against that rule. Takes a second.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
mkdir -p "$ROOT/artifacts/runs"
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson17.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/scripts"
git -C "$ROOT" show e5f09c2:scripts/verify_gate.py > "$WORK/scripts/verify_gate.py"
git -C "$ROOT" show e5f09c2:acceptance/final.json > "$WORK/final.json"
"$ROOT/.venv/bin/python" "$LESSON/probe.py" "$WORK"

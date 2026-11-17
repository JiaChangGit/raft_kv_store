#!/usr/bin/env bash
# Lesson 20: a cluster with a single voter (right after --bootstrap, before any
# `kv_admin.py add`) answers writes but never answers a GET; the client gets
# TIMEOUT_UNKNOWN after three seconds. Builds the version before the fix in a
# temporary directory and compares it with the current source. About a minute.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
BEFORE=s9-public-verified
mkdir -p "$ROOT/artifacts/runs"
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson20.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

mkdir "$WORK/before"
git -C "$ROOT" archive "$BEFORE" crates config Cargo.toml Cargo.lock rust-toolchain.toml | tar -x -C "$WORK/before"
(cd "$WORK/before" && CARGO_TARGET_DIR="$WORK/target-before" cargo build -q --locked -p raft-node)
(cd "$ROOT" && cargo build -q --locked -p raft-node)

"$ROOT/.venv/bin/python" "$LESSON/probe.py" "$WORK/target-before/debug/raft-node" "$WORK/cluster-before" \
  "$BEFORE (before the fix): one voter"
"$ROOT/.venv/bin/python" "$LESSON/probe.py" "$ROOT/target/debug/raft-node" "$WORK/cluster-now" \
  "current source: one voter"

printf '\n== RESULT\n'
echo "With one voter the old leader never finishes a read; the fixed one answers at once."

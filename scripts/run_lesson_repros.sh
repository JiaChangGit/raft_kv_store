#!/usr/bin/env bash
# Run every lesson's repro.sh and compare the RESULT section of its output with
# the one saved in expected-output.txt (timings are masked). check_lessons.py
# only checks that the files exist; this script checks that they still work.
# Lessons 06 and 13 had stopped working for two stages before anyone noticed.
#
#   bash scripts/run_lesson_repros.sh            # all lessons, about 10 minutes
#   bash scripts/run_lesson_repros.sh 06 13      # only these
#
# Output of each run goes to artifacts/runs/lesson-repros/<lesson>.out.
set -uo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/artifacts/runs/lesson-repros"
mkdir -p "$OUT"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cd "$ROOT"

result_section() {  # the lines from "== RESULT" on, without timings or blank lines
  sed -n '/^== RESULT/,$p' "$1" | grep -v '^[[:space:]]*$' | sed -E 's/[0-9]+(\.[0-9]+)? ?(ms|s)\b/N\2/g'
}

failed=0
for dir in docs/lessons/[0-9][0-9]-*/; do
  name=$(basename "$dir")
  if [ $# -gt 0 ] && [[ ! " $* " == *" ${name:0:2} "* ]]; then continue; fi
  start=$(date +%s)
  timeout 1500 bash "$dir/repro.sh" 2>&1 | head -c 400000 > "$OUT/$name.out"
  code=${PIPESTATUS[0]}
  took=$(( $(date +%s) - start ))
  if [ "$code" -ne 0 ]; then
    echo "FAIL  $name: exit $code after ${took}s (see $OUT/$name.out)"; failed=$((failed + 1))
  elif [ "$(result_section "$OUT/$name.out")" != "$(result_section "$dir/expected-output.txt")" ]; then
    echo "DIFF  $name: RESULT differs from expected-output.txt (see $OUT/$name.out)"; failed=$((failed + 1))
  else
    echo "ok    $name (${took}s)"
  fi
done
[ "$failed" -eq 0 ] && echo "all lesson reproductions match" || echo "$failed lesson(s) need attention"
exit $(( failed > 0 ))

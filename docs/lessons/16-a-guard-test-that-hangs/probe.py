"""Lesson 16 helper. `plant DIR` puts the mistake into a source copy; `run WORK
NAME:LABEL ...` runs the guard test in each copy under scripts/capped_run.py."""
import os
from pathlib import Path
import re
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts"))
from capped_run import run  # noqa: E402  the project's own time and output limit

TEST = "saturated_outbound_pool_does_not_starve_status"
# The mutation case saturated_pool_spin: a peer RPC's deadline is "now", so it
# is already over the moment it is sent.
GOOD = "                            Instant::now() + PEER_TIMEOUT,\n                        );"
BAD = "                            Instant::now(),\n                        );"
TIME_LIMIT, OUTPUT_CAP = 60, 256 << 20


def plant(directory: Path) -> None:
    path = directory / "crates/raft-node/src/main.rs"
    text = path.read_text()
    assert text.count(GOOD) == 1, "the mutation anchor moved"
    path.write_text(text.replace(GOOD, BAD))


def printed(rendered: bytes) -> int:
    """Bytes the command wrote, from what capped_run kept and its 'omitted' note."""
    note = re.search(rb"\n\[capped_run: (\d+) bytes omitted\]\n", rendered)
    return len(rendered) - len(note.group(0)) + int(note.group(1)) if note else len(rendered)


def run_all(work: Path, pairs: list[str]) -> None:
    for pair in pairs:
        name, label = pair.split(":", 1)
        env = dict(os.environ, CARGO_TARGET_DIR=str(work / f"target-{name}"))
        os.environ.update(env)
        started = time.monotonic()
        os.chdir(work / name)
        code, out, err, why = run(["cargo", "test", "-q", "--locked", "-p", "raft-node", TEST], TIME_LIMIT, OUTPUT_CAP)
        seconds = time.monotonic() - started
        size = printed(out) + printed(err)
        text = (out + err).decode(errors="replace")
        print(f"\n== {label}, with the mistake planted")
        if why:
            print(f"   KILLED after {seconds:.1f} s ({why}); the test had neither failed nor passed")
        elif code == 0:
            print(f"   the test PASSED after {seconds:.1f} s: the mistake went unnoticed")
        else:
            message = re.search(r"panicked at [^\n]*\n([^\n]+)", text)
            print(f"   the test FAILED after {seconds:.1f} s: {message.group(1).strip() if message else 'exit ' + str(code)}")
        print(f"   output: {size / 1e6:.1f} MB ({size / 1e6 / seconds:.0f} MB/s)")
    print("\n== RESULT")
    print("S4 and the current test fail within seconds. S5 before the fix hangs and floods its output.")


if __name__ == "__main__":
    if sys.argv[1] == "plant":
        plant(Path(sys.argv[2]))
    else:
        run_all(Path(sys.argv[2]), sys.argv[3:])

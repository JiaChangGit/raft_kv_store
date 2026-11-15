#!/usr/bin/env python3
"""Run a command in its own process group with a time limit and an output cap.

    capped_run.py --timeout SECONDS --cap BYTES -- COMMAND [ARG ...]

Used by mutation_check.py. A planted mistake can leave a test spinning for ever while it
prints some 50 MB per second (docs/INDEPENDENT_REVIEW_S5.md F6); capturing that in memory
filled an 8.6 GB machine within minutes, three times. Here the parent keeps only the first
and the last HEAD/TAIL bytes of each stream and never holds more. Past the cap or the time
limit the whole process group is killed, so a test binary started by cargo cannot outlive
its parent and keep printing.

Exit status: the command's own; 124 when the time limit was hit; 125 when the cap was hit.
"""
import argparse
import os
import signal
import subprocess
import sys
import threading
import time

HEAD = 4 << 20
TAIL = 4 << 20
CHUNK = 1 << 16


class Kept:
    """The start and the end of a stream, and how many bytes went by in all."""

    def __init__(self):
        self.head = bytearray()
        self.tail = bytearray()
        self.total = 0

    def add(self, chunk: bytes) -> None:
        self.total += len(chunk)
        room = HEAD - len(self.head)
        if room > 0:
            self.head += chunk[:room]
            chunk = chunk[room:]
        if chunk:
            self.tail += chunk
            if len(self.tail) > 2 * TAIL:
                del self.tail[: len(self.tail) - TAIL]

    def render(self) -> bytes:
        tail = bytes(self.tail[-TAIL:])
        dropped = self.total - len(self.head) - len(tail)
        middle = f"\n[capped_run: {dropped} bytes omitted]\n".encode() if dropped else b""
        return bytes(self.head) + middle + tail


def pump(stream, kept: Kept, lock: threading.Lock) -> None:
    fd = stream.fileno()
    while True:
        try:
            chunk = os.read(fd, CHUNK)
        except OSError:
            return
        if not chunk:
            return
        with lock:
            kept.add(chunk)


def run(command: list[str], timeout: float, cap: int) -> tuple[int, bytes, bytes, str]:
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               stdin=subprocess.DEVNULL, start_new_session=True)
    out, err, lock = Kept(), Kept(), threading.Lock()
    readers = [threading.Thread(target=pump, args=(process.stdout, out, lock), daemon=True),
               threading.Thread(target=pump, args=(process.stderr, err, lock), daemon=True)]
    for reader in readers:
        reader.start()
    started = time.monotonic()
    code, why = None, ""
    while code is None:
        try:
            code = process.wait(timeout=0.05)
        except subprocess.TimeoutExpired:
            pass
        with lock:
            seen = out.total + err.total
        if code is None and time.monotonic() - started > timeout:
            code, why = 124, f"time limit of {timeout:g} s reached"
        elif code is None and seen > cap:
            code, why = 125, f"output cap of {cap} bytes reached"
        if why:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
    # Whatever the command left behind in its group goes too; then the pipes reach end of file.
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    for reader in readers:
        reader.join(timeout=5)
    with lock:
        return code, out.render(), err.render(), why


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--cap", type=int, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("no command")
    code, out, err, why = run(command, args.timeout, args.cap)
    if why:
        err += f"\n[capped_run: killed the process group: {why}]\n".encode()
    sys.stdout.buffer.write(out)
    sys.stdout.buffer.flush()
    sys.stderr.buffer.write(err)
    sys.stderr.buffer.flush()
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    raise SystemExit(main())

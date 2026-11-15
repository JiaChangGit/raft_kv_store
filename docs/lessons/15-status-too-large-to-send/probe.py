#!/usr/bin/env python3
"""Lesson 15 helper: write about 1.2 MiB of values, then ask for the status.
Usage: probe.py BINARY WORKDIR LABEL"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tools.cluster import Cluster
from tools.kv_client import call, request

binary, work, label = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
with Cluster(work / label, proxy=False, binary=binary) as c:
    leader = c.wait_leader()
    for n in range(300):
        answer = call(c.address(leader), request(5, n + 1, "Put", f"key-{n}", "v" * 4096))
        assert answer["status"] == "OK", answer
    print(f"   {label}: 300 values of 4 KiB written (about 1.2 MiB of data)")
    try:
        status = call(c.address(leader), {"status": True}, timeout=2)
        print(f"   status: answered, role {status['role']}, kv_omitted {status.get('kv_omitted')}, kv_bytes {status.get('kv_bytes')}")
    except (OSError, EOFError, ValueError) as error:
        print(f"   status: NO ANSWER ({type(error).__name__}: {error})")
    try:
        c.wait_leader(timeout=3)
        print("   Cluster.wait_leader: found the leader")
    except TimeoutError:
        print("   Cluster.wait_leader: TIMED OUT (it asks every node for its status)")

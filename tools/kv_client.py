#!/usr/bin/env python3
"""Bounded framed client. A retry always retains the same client/sequence/content."""
import argparse
import json
import socket
import struct
import time

MAX_FRAME = 1024 * 1024

def recv_exact(sock, count):
    chunks = bytearray()
    while len(chunks) < count:
        part = sock.recv(count - len(chunks))
        if not part:
            raise EOFError("incomplete frame")
        chunks.extend(part)
    return bytes(chunks)

def read_frame(sock):
    length = struct.unpack("!I", recv_exact(sock, 4))[0]
    if length > MAX_FRAME:
        raise ValueError("oversized frame")
    return json.loads(recv_exact(sock, length))

def write_frame(sock, obj):
    data = json.dumps(obj, separators=(",", ":"), ensure_ascii=False).encode()
    if len(data) > MAX_FRAME:
        raise ValueError("oversized frame")
    sock.sendall(struct.pack("!I", len(data)) + data)

def call(address, request, timeout=4.0):
    host, port = address.rsplit(":", 1)
    with socket.create_connection((host, int(port)), timeout=timeout) as sock:
        sock.settimeout(timeout)
        write_frame(sock, request)
        return read_frame(sock)

def request(client, sequence, operation, key, value=None):
    body = {"key": key}
    if operation == "Put":
        body["value"] = value
    return {"version":1,"client_id": client, "sequence": sequence, "operation": {operation: body}}

def retry(addresses, payload, timeout=10.0):
    deadline = time.monotonic() + timeout
    last = {"status": "TIMEOUT_UNKNOWN", "result": None}
    while time.monotonic() < deadline:
        for address in addresses:
            try:
                last = call(address, payload, timeout=min(1.0, max(.05, deadline-time.monotonic())))
                if last["status"] in ("OK", "REQUEST_ID_REUSE", "INVALID_ARGUMENT", "STORAGE_ERROR"):
                    return last
            except (OSError, EOFError, ValueError):
                pass
        time.sleep(.025)
    return {**last, "status": "TIMEOUT_UNKNOWN"}

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--address", action="append", required=True)
    p.add_argument("--client-id", type=int, default=1)
    p.add_argument("--sequence", type=int, required=True)
    p.add_argument("operation", choices=["Put", "Get", "Delete"])
    p.add_argument("key")
    p.add_argument("value", nargs="?")
    a = p.parse_args()
    result = retry(a.address, request(a.client_id,a.sequence,a.operation,a.key,a.value))
    print(json.dumps(result, ensure_ascii=False))
    return 0 if result["status"] == "OK" else 1

if __name__ == "__main__":
    raise SystemExit(main())

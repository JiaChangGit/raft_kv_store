import json
import socket
import struct
import threading
import time

from tools.cluster import allocate_ports
from tools.fault_proxy import FaultProxy
from tools.kv_client import read_frame, write_frame


def test_reorder_flush_preserves_in_progress_fragmented_frame():
    backend_port, proxy_port = allocate_ports(2)
    listener = socket.socket()
    listener.bind(("127.0.0.1", backend_port))
    listener.listen()
    backend_received = []

    def backend():
        connection, _ = listener.accept()
        with connection:
            connection.settimeout(2)
            try:
                while True:
                    item = read_frame(connection)
                    backend_received.append(item["rpc_id"])
                    write_frame(connection, {**item, "from": 2, "to": 1})
            except (OSError, EOFError):
                pass

    thread = threading.Thread(target=backend, daemon=True)
    thread.start()
    proxy = FaultProxy([(1, 2, proxy_port, backend_port)]).start()
    try:
        proxy.set_rule(1, 2, "reorder")
        with socket.create_connection(("127.0.0.1", proxy_port), timeout=1) as connection:
            connection.settimeout(2)
            write_frame(connection, {"from": 1, "to": 2, "rpc_id": 1})
            payload = json.dumps({"from": 1, "to": 2, "rpc_id": 2}).encode()
            packet = struct.pack("!I", len(payload)) + payload
            connection.sendall(packet[:10])  # header consumed; body still incomplete
            time.sleep(.15)  # held first frame flushes while this read is pending
            connection.sendall(packet[10:])
            assert read_frame(connection)["rpc_id"] == 1
            assert read_frame(connection)["rpc_id"] == 2
            assert backend_received == [1, 2]
    finally:
        proxy.close()
        listener.close()
        thread.join(2)

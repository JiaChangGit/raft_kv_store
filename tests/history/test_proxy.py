import asyncio
import socket
import threading
from tools.cluster import allocate_ports
from tools.fault_proxy import FaultProxy
from tools.kv_client import read_frame,write_frame

def test_complete_frame_fault_modes():
    backend_port,listen_port=allocate_ports(2)
    listener=socket.socket()
    listener.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
    listener.bind(("127.0.0.1",backend_port))
    listener.listen()
    stop=threading.Event()
    def echo():
        while not stop.is_set():
            try:
                conn,_=listener.accept()
            except OSError: return
            def session(conn):
                with conn:
                    try:
                        while True:
                            obj=read_frame(conn)
                            write_frame(conn,{**obj,"from":2,"to":1})
                    except (OSError,EOFError): pass
            threading.Thread(target=session,args=(conn,),daemon=True).start()
    thread=threading.Thread(target=echo,daemon=True)
    thread.start()
    proxy=FaultProxy([(1,2,listen_port,backend_port)]).start()
    try:
        with socket.create_connection(("127.0.0.1",listen_port),timeout=1) as conn:
            conn.settimeout(1)
            proxy.set_rule(1,2,"reorder")
            write_frame(conn,{"from":1,"to":2,"rpc_id":1})
            write_frame(conn,{"from":1,"to":2,"rpc_id":2})
            assert [read_frame(conn)["rpc_id"],read_frame(conn)["rpc_id"]]==[2,1]
            proxy.set_rule(1,2,"duplicate")
            write_frame(conn,{"from":1,"to":2,"rpc_id":3})
            assert [read_frame(conn)["rpc_id"],read_frame(conn)["rpc_id"]]==[3,3]
            proxy.set_rule(1,2,"delay",delay=.02)
            write_frame(conn,{"from":1,"to":2,"rpc_id":4})
            assert read_frame(conn)["rpc_id"]==4
            proxy.set_rule(1,2,"drop")
            write_frame(conn,{"from":1,"to":2,"rpc_id":5})
            conn.settimeout(.1)
            try:
                read_frame(conn)
                raise AssertionError("drop forwarded frame")
            except socket.timeout: pass
            proxy.set_rule(1,2,"allow")
            write_frame(conn,{"from":1,"to":2,"rpc_id":6})
            conn.settimeout(1)
            assert read_frame(conn)["rpc_id"]==6
    finally:
        proxy.close()
        stop.set()
        listener.close()

#!/usr/bin/env python3
"""Temporary real Linux process cluster. All fault-test routes use proxies."""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import threading
import time
from tools.kv_client import call,request,retry
from tools.fault_proxy import FaultProxy

ROOT=Path(__file__).resolve().parents[1]

def allocate_ports(count):
    sockets=[]
    try:
        for _ in range(count):
            s=socket.socket()
            s.bind(("127.0.0.1",0))
            sockets.append(s)
        return [s.getsockname()[1] for s in sockets]
    finally:
        for s in sockets:
            s.close()

class Cluster:
    def __init__(self,directory,proxy=True,snapshot_every=None):
        self.directory=Path(directory)
        self.snapshot_every=snapshot_every
        self.directory.mkdir(parents=True,exist_ok=True)
        ports=allocate_ports(12)
        self.peer={n:ports[n-1] for n in (1,2,3)}
        self.client={n:ports[n+2] for n in (1,2,3)}
        self.routes=[(a,b,ports[6+i],self.peer[b]) for i,(a,b) in enumerate(( (a,b) for a in (1,2,3) for b in (1,2,3) if a!=b ))]
        self.proxy=FaultProxy(self.routes,self.directory/"faults.jsonl") if proxy else None
        self.processes={}
        self.files=[]
        self.history=[]
        self.history_lock=threading.Lock()
        self.config=self.directory/"cluster.toml"
        text="cluster_id = 'raft-kv-v1'\n"
        for n in (1,2,3):
            text+=f"\n[[nodes]]\nid = {n}\npeer_addr = '127.0.0.1:{self.peer[n]}'\nclient_addr = '127.0.0.1:{self.client[n]}'\n"
        if proxy:
            for a,b,port,_ in self.routes:
                text+=f"\n[[routes]]\nfrom = {a}\nto = {b}\naddress = '127.0.0.1:{port}'\n"
        self.config.write_text(text)

    def address(self,n):
        return f"127.0.0.1:{self.client[n]}"

    def start_node(self,n,env=None):
        if n in self.processes and self.processes[n].poll() is None:
            raise RuntimeError("already running")
        out=(self.directory/f"node-{n}.jsonl").open("ab",buffering=0)
        err=(self.directory/f"node-{n}.stderr").open("ab",buffering=0)
        self.files.extend([out,err])
        merged=dict(os.environ)
        merged.pop("RAFT_FAILPOINT",None)
        merged["RAFT_FAILPOINT_FILE"]=str(self.directory/f"failpoint-{n}")
        if env:
            merged.update(env)
        argv=[str(ROOT/"target/debug/raft-node"),"--id",str(n),"--config",str(self.config),"--data-dir",str(self.directory/f"node-{n}")]
        if self.snapshot_every:
            argv+=["--snapshot-every",str(self.snapshot_every)]
        proc=subprocess.Popen(argv,stdout=out,stderr=err,env=merged)
        self.processes[n]=proc
        return proc

    def start(self):
        if self.proxy:
            self.proxy.start()
        for n in (1,2,3):
            self.start_node(n)
        pids=[p.pid for p in self.processes.values()]
        assert len(set(pids))==3
        (self.directory/"pids.json").write_text(json.dumps(pids))
        self.wait_leader()
        return self

    def status(self,n):
        return call(self.address(n),{"status":True},timeout=.5)

    def wait(self,predicate,timeout=10):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            try:
                value=predicate()
                if value:
                    return value
            except (OSError,EOFError,ValueError,KeyError):
                pass
            time.sleep(.025)
        states={}
        for n in (1,2,3):
            try: states[n]=self.status(n)
            except (OSError,EOFError,ValueError): states[n]="unavailable"
        raise TimeoutError(f"cluster condition not met: {states}, logs={self.directory}")

    def wait_leader(self,exclude=()):
        def find():
            candidates=[]
            for n,p in self.processes.items():
                if n in exclude or p.poll() is not None:
                    continue
                try:
                    s=self.status(n)
                    if s["role"]=="Leader": candidates.append((s["term"],n))
                except (OSError,EOFError,ValueError): pass
            return max(candidates)[1] if candidates else None
        return self.wait(find)

    def kill(self,n):
        p=self.processes[n]
        if p.poll() is None:
            p.send_signal(signal.SIGKILL)
            p.wait(timeout=3)

    def invoke(self,payload,node=None,timeout=10):
        start=time.monotonic()
        if node:
            try: response=call(self.address(node),payload,timeout=timeout)
            except (OSError,EOFError,ValueError): response={"status":"TIMEOUT_UNKNOWN","result":None}
        else:
            response=retry([self.address(n) for n in (1,2,3)],payload,timeout)
        with self.history_lock:
            self.history.append({**payload,"invocation_time":start,"response_time":time.monotonic(),"status":response["status"],"result":response.get("result")})
            (self.directory/"history.json").write_text(json.dumps(self.history,indent=2))
        return response

    def close(self):
        for n in list(self.processes):
            self.kill(n)
        if self.proxy:
            self.proxy.close()
        for f in self.files:
            f.close()

    def __enter__(self):
        try:
            return self.start()
        except BaseException:
            self.close()
            raise
    def __exit__(self,*_): self.close()

#!/usr/bin/env python3
"""Temporary real Linux process cluster. All fault-test routes use proxies.

Since S4 (docs/MEMBERSHIP.md §6) a cluster is created the way an operator
would: node 1 bootstraps alone, the other nodes start empty, and they are
added one at a time through the membership request on the client port.
`legacy=True` starts a pre-S4 binary (fixed nodes 1, 2, 3) for comparisons.
"""
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

def membership(kind,node):
    """A membership request (docs/MEMBERSHIP.md §10); kind is "Add" or "Remove"."""
    return {"version":1,"membership":{kind:node}}

class Cluster:
    def __init__(self,directory,proxy=True,snapshot_every=None,binary=None,nodes=3,capacity=None,legacy=False):
        self.directory=Path(directory)
        self.snapshot_every=snapshot_every
        # Another build of raft-node, e.g. an older tag for a before/after measurement.
        self.binary=Path(binary) if binary else ROOT/"target/debug/raft-node"
        self.legacy=legacy
        capacity=capacity or nodes
        if legacy and (nodes,capacity)!=(3,3):
            raise ValueError("a pre-S4 binary only knows nodes 1, 2 and 3")
        if not 1<=nodes<=capacity<=7:
            raise ValueError("need 1 <= nodes <= capacity <= 7")
        # The address book lists `capacity` nodes; the first `nodes` form the cluster.
        self.ids=tuple(range(1,capacity+1))
        self.initial=tuple(range(1,nodes+1))
        self.directory.mkdir(parents=True,exist_ok=True)
        pairs=[(a,b) for a in self.ids for b in self.ids if a!=b]
        ports=allocate_ports(2*capacity+len(pairs))
        self.peer={n:ports[n-1] for n in self.ids}
        self.client={n:ports[capacity+n-1] for n in self.ids}
        self.routes=[(a,b,ports[2*capacity+i],self.peer[b]) for i,(a,b) in enumerate(pairs)]
        self.proxy=FaultProxy(self.routes,self.directory/"faults.jsonl") if proxy else None
        self.processes={}
        self.files=[]
        self.history=[]
        self.history_lock=threading.Lock()
        self.config=self.directory/"cluster.toml"
        text="cluster_id = 'raft-kv-v1'\n"
        for n in self.ids:
            text+=f"\n[[nodes]]\nid = {n}\npeer_addr = '127.0.0.1:{self.peer[n]}'\nclient_addr = '127.0.0.1:{self.client[n]}'\n"
        if proxy:
            for a,b,port,_ in self.routes:
                text+=f"\n[[routes]]\nfrom = {a}\nto = {b}\naddress = '127.0.0.1:{port}'\n"
        self.config.write_text(text)

    def address(self,n):
        return f"127.0.0.1:{self.client[n]}"

    def start_node(self,n,env=None,bootstrap=False):
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
        argv=[str(self.binary),"--id",str(n),"--config",str(self.config),"--data-dir",str(self.directory/f"node-{n}")]
        if bootstrap:
            argv.append("--bootstrap")
        if self.snapshot_every:
            argv+=["--snapshot-every",str(self.snapshot_every)]
        proc=subprocess.Popen(argv,stdout=out,stderr=err,env=merged)
        self.processes[n]=proc
        return proc

    def start(self):
        if self.proxy:
            self.proxy.start()
        if self.legacy:
            for n in self.initial:
                self.start_node(n)
        else:
            # Dissertation §4.4: bootstrap one node, then add the others.
            self.start_node(1,bootstrap=True)
            for n in self.initial[1:]:
                self.start_node(n)
        pids=[p.pid for p in self.processes.values()]
        assert len(set(pids))==len(self.initial)
        (self.directory/"pids.json").write_text(json.dumps(pids))
        self.wait_leader()
        if not self.legacy:
            for n in self.initial[1:]:
                answer=self.change("Add",n)
                if answer["status"]!="OK":
                    raise RuntimeError(f"could not add node {n}: {answer}")
            expected=list(self.initial)
            self.wait(lambda:all(self.status(n)["voters"]==expected for n in self.initial))
        return self

    def status(self,n):
        return call(self.address(n),{"status":True},timeout=.5)

    def running(self):
        return [n for n,p in self.processes.items() if p.poll() is None]

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
        for n in self.ids:
            try: states[n]=self.status(n)
            except (OSError,EOFError,ValueError): states[n]="unavailable"
        raise TimeoutError(f"cluster condition not met: {states}, logs={self.directory}")

    def wait_leader(self,exclude=(),timeout=10):
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
        return self.wait(find,timeout)

    def change(self,kind,node,timeout=40):
        """Send a membership request to the leader until it gets a definite answer.

        CHANGE_IN_PROGRESS and NOT_LEADER are retried; TIMEOUT_UNKNOWN is resolved
        by asking again, which answers OK at once if the change took effect.
        """
        deadline=time.monotonic()+timeout
        answer={"status":"TIMEOUT_UNKNOWN","result":None}
        while time.monotonic()<deadline:
            try:
                leader=self.wait_leader(timeout=max(.1,deadline-time.monotonic()))
                answer=call(self.address(leader),membership(kind,node),timeout=17)
            except (OSError,EOFError,ValueError,TimeoutError):
                answer={"status":"TIMEOUT_UNKNOWN","result":None}
            if answer["status"] not in ("NOT_LEADER","CHANGE_IN_PROGRESS","TIMEOUT_UNKNOWN"):
                return answer
            time.sleep(.1)
        return answer

    def voters(self):
        """The configuration as the current leader sees it."""
        return self.status(self.wait_leader())["voters"]

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
            response=retry([self.address(n) for n in self.ids],payload,timeout)
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

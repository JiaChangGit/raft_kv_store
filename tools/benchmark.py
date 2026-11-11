#!/usr/bin/env python3
"""Measured single-machine process benchmark; never disables durability."""
import argparse
import concurrent.futures
import datetime
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import time
from tools.cluster import Cluster,ROOT
from tools.kv_client import request

def percentiles(values):
    if not values: raise ValueError("no measurement samples")
    values=sorted(values)
    return {"count":len(values),"p50":statistics.median(values),"p95":values[min(len(values)-1,int(.95*len(values)))],"p99":values[min(len(values)-1,int(.99*len(values)))]}

def resources(cluster):
    rss=0
    cpu=0
    for p in cluster.processes.values():
        if p.poll() is None:
            try:
                fields=Path(f"/proc/{p.pid}/stat").read_text().split()
                cpu+=(int(fields[13])+int(fields[14]))/os.sysconf("SC_CLK_TCK")
                rss+=int(fields[23])*os.sysconf("SC_PAGE_SIZE")
            except FileNotFoundError: pass
    wal=sum(p.stat().st_size for p in cluster.directory.glob("node-*/*.wal"))
    return {"rss_bytes":rss,"process_cpu_seconds":cpu,"wal_bytes":wal}

def run(directory,samples=12):
    if samples<1: raise ValueError("samples must be positive")
    directory=Path(directory)
    directory.mkdir(parents=True,exist_ok=True)
    metrics={}
    raw=[]
    with Cluster(directory/"cluster") as c:
        assert c.invoke(request(900,0,"Put","warmup","x"))["status"]=="OK"
        def timed(payload):
            begin=time.perf_counter()
            response=c.invoke(payload)
            elapsed=(time.perf_counter()-begin)*1000
            if response["status"]!="OK": raise RuntimeError(response)
            name=next(iter(payload["operation"]))
            raw.append({"operation":name,"latency_ms":elapsed,"client_id":payload["client_id"],"sequence":payload["sequence"]})
            return name,elapsed
        def workload(name,clients,count):
            before=resources(c)
            start=len(raw)
            begin=time.perf_counter()
            def client_run(client):
                for seq in range(count):
                    timed(request(client,seq*2,"Put",f"bench-{client}","v"*64))
                    timed(request(client,seq*2+1,"Get",f"bench-{client}"))
            if clients==1: client_run(1000+len(metrics)*100)
            else:
                with concurrent.futures.ThreadPoolExecutor(max_workers=clients) as pool:
                    list(pool.map(client_run,range(2000+len(metrics)*100,2000+len(metrics)*100+clients)))
            elapsed=time.perf_counter()-begin
            selected=raw[start:]
            after=resources(c)
            metrics[name]={"client_count":clients,"duration_seconds":elapsed,"operation_count":len(selected),"ops_per_second":len(selected)/elapsed,"Put_latency_ms":percentiles([x["latency_ms"] for x in selected if x["operation"]=="Put"]),"Get_latency_ms":percentiles([x["latency_ms"] for x in selected if x["operation"]=="Get"]),"resources_before":before,"resources_after":after,"cpu_seconds_delta":after["process_cpu_seconds"]-before["process_cpu_seconds"],"wal_growth_bytes":after["wal_bytes"]-before["wal_bytes"]}
        workload("single_client",1,samples)
        workload("eight_clients",8,samples)
        leader=c.wait_leader()
        follower=next(n for n in (1,2,3) if n!=leader)
        c.proxy.isolate(follower)
        workload("one_follower_isolated",1,samples)
        target=c.status(leader)["commit_index"]
        begin=time.perf_counter()
        c.proxy.heal()
        c.wait(lambda:c.status(follower)["last_applied"]>=target)
        metrics["follower_catchup_ms"]=(time.perf_counter()-begin)*1000
        leader=c.wait_leader()
        begin=time.perf_counter()
        c.kill(leader)
        c.wait_leader(exclude=[leader])
        timed(request(9999,1,"Put","failover","confirmed"))
        metrics["leader_failover_to_success_ms"]=(time.perf_counter()-begin)*1000
        target=c.status(c.wait_leader(exclude=[leader]))["commit_index"]
        begin=time.perf_counter()
        c.start_node(leader)
        c.wait(lambda:c.status(leader)["last_applied"]>=target)
        metrics["restarted_follower_catchup_ms"]=(time.perf_counter()-begin)*1000
        # Read measured persist events after actors have flushed their trace.
        sync=[]
        fsync=[]
        for path in c.directory.glob("node-*.jsonl"):
            for line in path.read_text().splitlines():
                try: row=json.loads(line)
                except json.JSONDecodeError: continue
                if row.get("kind")=="persist" and "sync_us" in row:
                    sync.append(row["sync_us"])
                    fsync.append(row["details"]["fsync_us"])
        metrics["wal_transaction_save_us"]=percentiles(sync)
        metrics["wal_fsync_us"]=percentiles(fsync)
        measurements=resources(c)
    rustc=shutil.which("rustc") or str(Path.home()/".cargo/bin/rustc")
    version=subprocess.check_output([rustc,"-Vv"],text=True).strip()
    report={"command":" ".join(sys.argv),"date":datetime.datetime.now(datetime.timezone.utc).isoformat(),"run_count":1,"metrics":metrics,"environment":{"platform":platform.platform(),"python":sys.version,"rustc":version,"cpu_count":os.cpu_count(),"visible_ram_bytes":os.sysconf("SC_PAGE_SIZE")*os.sysconf("SC_PHYS_PAGES"),"payload_bytes":64,"samples_per_client":samples,"warmup_operations":1,"wal_sync_policy":"every transaction sync_all; no bypass","final_resources":measurements},"limits":["One WSL2 machine; not a multi-host network prediction.","Small finite sample; tail percentiles are descriptive only.","wal_transaction_save_us includes encoding/write/sync, not syscall-only fsync.","Three Raft processes; proxy/client CPU excluded from process CPU and RSS."],"raw_samples":raw}
    (directory/"raw-samples.json").write_text(json.dumps(raw,indent=2))
    return report

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--directory",type=Path,required=True)
    p.add_argument("--output",type=Path,required=True)
    p.add_argument("--samples",type=int,default=12)
    a=p.parse_args()
    report=run(a.directory,a.samples)
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"output":str(a.output),"run_count":report["run_count"],"metrics":list(report["metrics"])}))

if __name__=="__main__": main()

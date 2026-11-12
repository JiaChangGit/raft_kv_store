#!/usr/bin/env python3
"""Independent bounded sequential KV/session oracle; UNKNOWN is never a failed PUT."""
import argparse
import json
import math
from pathlib import Path

def apply(model,sessions,operation):
    name,body=next(iter(operation["operation"].items()))
    if len(body["key"].encode("utf-8"))>256 or (name=="Put" and len(body["value"].encode("utf-8"))>4096):
        return model,sessions,("INVALID_ARGUMENT",None)
    # One remembered request per client: only its latest sequence can be answered
    # again; anything older is rejected without effect (docs/CLIENT_CONTRACT.md).
    client,sequence=operation["client_id"],operation["sequence"]
    command=json.dumps(operation["operation"],sort_keys=True,separators=(",",":"))
    if client in sessions:
        latest,old_command,result=sessions[client]
        if sequence==latest:
            return model,sessions,("OK",result) if old_command==command else ("REQUEST_ID_REUSE",None)
        if sequence<latest:
            return model,sessions,("INVALID_ARGUMENT",None)
    kv=dict(model)
    k=body["key"]
    value=kv.get(k)
    if name=="Put": kv[k]=body["value"]
    elif name=="Delete": kv.pop(k,None)
    elif name!="Get": raise ValueError("unsupported operation")
    table=dict(sessions)
    table[client]=(sequence,command,value)
    return kv,table,("OK",value)

def check(history,max_states=200000):
    if not isinstance(history,list): raise ValueError("history must be a list")
    allowed={"OK","REQUEST_ID_REUSE","INVALID_ARGUMENT","TIMEOUT_UNKNOWN","STORAGE_ERROR","NOT_LEADER"}
    uncertain={"TIMEOUT_UNKNOWN","STORAGE_ERROR"}
    for h in history:
        if not isinstance(h,dict): raise ValueError("history operation must be an object")
        if not isinstance(h.get("status"),str) or h["status"] not in allowed:
            raise ValueError("unknown status")
        for field,bits in (("client_id",128),("sequence",64)):
            if type(h.get(field)) is not int or not 0<=h[field]<2**bits:
                raise ValueError(f"invalid {field}")
        for field in ("invocation_time","response_time"):
            if type(h.get(field)) not in (int,float) or (type(h[field]) is float and not math.isfinite(h[field])):
                raise ValueError(f"invalid time field {field}")
        if h["response_time"]<h["invocation_time"]: raise ValueError("reversed time interval")
        operation=h.get("operation")
        if not isinstance(operation,dict) or len(operation)!=1:
            raise ValueError("operation must have one variant")
        name,body=next(iter(operation.items()))
        expected={"key","value"} if name=="Put" else {"key"}
        if name not in {"Put","Get","Delete"} or not isinstance(body,dict) or set(body)!=expected or any(not isinstance(v,str) for v in body.values()):
            raise ValueError("invalid operation body")
        if h.get("result") is not None and not isinstance(h["result"],str): raise ValueError("invalid result")
    operations=[h for h in history if h["status"]!="NOT_LEADER"]
    if not operations:
        return {"status":"INCONCLUSIVE","reason":"no effect-bearing operations; empty histories are mathematically linearizable but provide no execution evidence","states":0,"operation_count":0}
    for h in operations:
        if h["response_time"]<h["invocation_time"]:
            raise ValueError("reversed interval")
    n=len(operations)
    if n>24:
        return {"status":"INCONCLUSIVE","reason":"history exceeds 24 operation bound","states":0}
    predecessors=[]
    for item in operations:
        bits=0
        for j,other in enumerate(operations):
            if other["status"] not in uncertain and other["response_time"]<item["invocation_time"]:
                bits|=1<<j
        predecessors.append(bits)
    visited=set()
    count=0
    exhausted=False
    def search(remaining,kv,sessions):
        nonlocal count,exhausted
        if not remaining: return True
        count+=1
        if count>max_states:
            exhausted=True
            return False
        identity=(remaining,tuple(sorted(kv.items())),tuple(sorted((k,v) for k,v in sessions.items())))
        if identity in visited: return False
        visited.add(identity)
        for i,item in enumerate(operations):
            if not (remaining>>i)&1: continue
            rest=remaining&~(1<<i)
            # An unknown outcome may be discarded without claiming it was aborted.
            if item["status"] in uncertain and search(rest,kv,sessions): return True
            if predecessors[i]&remaining: continue
            next_kv,next_sessions,result=apply(kv,sessions,item)
            if item["status"] in uncertain or result==(item["status"],item.get("result")):
                if search(rest,next_kv,next_sessions): return True
        return False
    passed=search((1<<n)-1,{}, {})
    return {"status":"PASS" if passed else "INCONCLUSIVE" if exhausted else "FAIL","states":count,"operation_count":n,"search_limit":max_states}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("history",type=Path)
    p.add_argument("--output",type=Path)
    a=p.parse_args()
    result=check(json.loads(a.history.read_text()))
    text=json.dumps(result,indent=2)
    if a.output: a.output.write_text(text+"\n")
    print(text)
    return 0 if result["status"]=="PASS" else 1

if __name__=="__main__": raise SystemExit(main())

#!/usr/bin/env python3
"""Directional complete-frame fault proxy. Never reorders raw TCP bytes."""
import asyncio
import json
import struct
import threading
import time
from pathlib import Path

MAX_FRAME = 1024 * 1024

class FaultProxy:
    def __init__(self, endpoints, trace=None):
        # (from,to,listen_port,destination_port), all local test routes.
        self.endpoints = endpoints
        self.rules = {}
        self.events = []
        self.trace = Path(trace) if trace else None
        self.loop = None
        self.servers = []
        self.thread = None
        self.ready = threading.Event()
        self.connections = set()
        self.sequence = 0
        self.error = None

    def record(self, **event):
        self.sequence += 1
        event.update(sequence=self.sequence, time=time.monotonic())
        self.events.append(event)
        if self.trace:
            with self.trace.open("a", encoding="utf-8") as out:
                out.write(json.dumps(event)+"\n")

    def set_rule(self, source, target, mode="allow", delay=.03):
        if mode not in {"allow","drop","delay","duplicate","reorder"}:
            raise ValueError(mode)
        def change():
            self.rules[(source,target)] = (mode,delay)
            self.record(kind="rule",source=source,target=target,mode=mode,delay=delay)
        if self.loop:
            done=threading.Event()
            def applied():
                change()
                done.set()
            self.loop.call_soon_threadsafe(applied)
            if not done.wait(2):
                raise TimeoutError("proxy rule not applied")
        else:
            change()

    def isolate(self, node):
        for other in (1,2,3):
            if other != node:
                self.set_rule(node,other,"drop")
                self.set_rule(other,node,"drop")

    def heal(self):
        for source,target,_,_ in self.endpoints:
            self.set_rule(source,target,"allow")

    async def frame(self, reader):
        header = await reader.readexactly(4)
        length = struct.unpack("!I",header)[0]
        if length > MAX_FRAME:
            raise ValueError("oversized proxy frame")
        body = await reader.readexactly(length)
        obj = json.loads(body)
        return header+body,obj

    async def pump(self, reader, writer, default_source, default_target):
        held = None
        pending = asyncio.create_task(self.frame(reader))
        async def flush_held():
            nonlocal held
            if held:
                old_frame,old_source,old_target=held
                if self.rules.get((old_source,old_target),("allow",0))[0]!="drop":
                    writer.write(old_frame)
                    await writer.drain()
                held=None
        try:
            while True:
                try:
                    # Keep the same partial-frame read alive when only the held
                    # frame's flush timer expires. Cancellation would lose bytes.
                    frame,obj = await asyncio.wait_for(asyncio.shield(pending), .04 if held else 30)
                except asyncio.TimeoutError:
                    if held:
                        await flush_held()
                        continue
                    return
                except asyncio.IncompleteReadError:
                    await flush_held()
                    return
                pending = asyncio.create_task(self.frame(reader))
                source,target = obj.get("from",default_source),obj.get("to",default_target)
                mode,delay = self.rules.get((source,target),("allow",0))
                self.record(kind="frame",source=source,target=target,mode=mode,rpc_id=obj.get("rpc_id"))
                if mode == "drop":
                    continue
                if mode == "delay":
                    await asyncio.sleep(delay)
                    if self.rules.get((source,target),("allow",0))[0] == "drop":
                        continue
                if mode == "reorder":
                    if held is None:
                        held=(frame,source,target)
                        continue
                    writer.write(frame)
                    await flush_held()
                else:
                    await flush_held()
                    writer.write(frame)
                    if mode == "duplicate":
                        writer.write(frame)
                await writer.drain()
        finally:
            pending.cancel()
            await asyncio.gather(pending, return_exceptions=True)

    async def handle(self, reader, writer, source, target, port):
        backend=None
        tasks=[]
        self.connections.add(writer)
        try:
            r2,backend = await asyncio.open_connection("127.0.0.1",port)
            self.connections.add(backend)
            tasks=[asyncio.create_task(self.pump(reader,backend,source,target)),asyncio.create_task(self.pump(r2,writer,target,source))]
            await asyncio.wait(tasks,return_when=asyncio.FIRST_COMPLETED)
        except (OSError,EOFError,ValueError,asyncio.IncompleteReadError):
            pass
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            for stream in (writer,backend):
                if stream:
                    stream.close()
                    self.connections.discard(stream)

    async def boot(self):
        for source,target,listen,dest in self.endpoints:
            server=await asyncio.start_server(lambda r,w,s=source,t=target,d=dest:self.handle(r,w,s,t,d),"127.0.0.1",listen)
            self.servers.append(server)

    def start(self):
        def run():
            self.loop=asyncio.new_event_loop()
            asyncio.set_event_loop(self.loop)
            try:
                self.loop.run_until_complete(self.boot())
                self.ready.set()
                self.loop.run_forever()
            except BaseException as error:
                self.error=error
                self.ready.set()
            finally:
                pending=asyncio.all_tasks(self.loop)
                for task in pending:
                    task.cancel()
                self.loop.run_until_complete(asyncio.gather(*pending,return_exceptions=True))
                self.loop.close()
        self.thread=threading.Thread(target=run,daemon=True)
        self.thread.start()
        if not self.ready.wait(3):
            raise TimeoutError("proxy startup")
        if self.error:
            raise self.error
        return self

    def close(self):
        if self.loop and self.loop.is_running():
            def stop():
                for server in self.servers:
                    server.close()
                for connection in list(self.connections):
                    connection.close()
                self.loop.stop()
            self.loop.call_soon_threadsafe(stop)
        if self.thread:
            self.thread.join(3)

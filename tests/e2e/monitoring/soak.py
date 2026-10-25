#!/usr/bin/env python3
"""Bounded, real two-process HTTP/SSE/WebSocket monitoring soak (stdlib only)."""
import argparse
import asyncio
import base64
from collections import Counter
import contextlib
import datetime as dt
import hashlib
import json
from pathlib import Path
import platform
import secrets
import struct
import subprocess
import time
import urllib.parse
import uuid


MAX_RESPONSE = 8 * 1024 * 1024


class ProtocolError(Exception):
    """Intentionally has no response body, URL, credential, or payload attached."""


async def headers(reader):
    raw = await reader.readuntil(b"\r\n\r\n")
    lines = raw.decode("latin1").split("\r\n")
    fields = dict(line.split(":", 1) for line in lines[1:] if ":" in line)
    return lines[0], {k.lower(): v.strip() for k, v in fields.items()}


class Response:
    def __init__(self, reader, writer, status, fields):
        self.reader, self.writer, self.status, self.fields = reader, writer, status, fields

    async def chunks(self):
        if "chunked" in self.fields.get("transfer-encoding", "").lower():
            while True:
                size = int((await self.reader.readline()).split(b";", 1)[0], 16)
                if size > MAX_RESPONSE:
                    raise ProtocolError("chunk_bound")
                if not size:
                    while await self.reader.readline() != b"\r\n":
                        if self.reader.at_eof():
                            raise ProtocolError("truncated_trailer")
                    break
                yield await self.reader.readexactly(size)
                if await self.reader.readexactly(2) != b"\r\n":
                    raise ProtocolError("invalid_chunk")
        elif "content-length" in self.fields:
            remaining = int(self.fields["content-length"])
            while remaining:
                chunk = await self.reader.readexactly(min(remaining, 65536))
                remaining -= len(chunk)
                yield chunk
        else:
            while chunk := await self.reader.read(65536):
                yield chunk

    async def body(self):
        result = bytearray()
        async for chunk in self.chunks():
            result.extend(chunk)
            if len(result) > MAX_RESPONSE:
                raise ProtocolError("response_bound")
        return bytes(result)

    async def close(self):
        self.writer.close()
        with contextlib.suppress(OSError):
            await self.writer.wait_closed()


async def request(base, path, token, method="GET", body=None, extra=None):
    target = urllib.parse.urlsplit(base)
    reader, writer = await asyncio.open_connection(target.hostname, target.port, limit=MAX_RESPONSE)
    try:
        data = b"" if body is None else json.dumps(body).encode()
        fields = {"Host": target.netloc, "Authorization": "Bearer " + token,
                  "Connection": "close", **(extra or {})}
        if body is not None:
            fields.update({"Content-Type": "application/json", "Content-Length": str(len(data))})
        writer.write((f"{method} {path} HTTP/1.1\r\n" + "".join(
            f"{k}: {v}\r\n" for k, v in fields.items()) + "\r\n").encode() + data)
        await writer.drain()
        first, fields = await headers(reader)
        return Response(reader, writer, int(first.split()[1]), fields)
    except BaseException:
        writer.close()
        raise


async def frame_read(reader):
    first, second = await reader.readexactly(2)
    length = second & 127
    if length == 126:
        length = struct.unpack("!H", await reader.readexactly(2))[0]
    elif length == 127:
        length = struct.unpack("!Q", await reader.readexactly(8))[0]
    if length > 65536 or not first & 128:
        raise ProtocolError("frame_bound_or_fragment")
    mask = await reader.readexactly(4) if second & 128 else None
    payload = await reader.readexactly(length)
    if mask:
        payload = bytes(value ^ mask[i % 4] for i, value in enumerate(payload))
    return first & 15, payload


async def frame_write(writer, opcode, payload=b"", masked=False):
    mask = secrets.token_bytes(4) if masked else b""
    length = len(payload)
    prefix = bytes([128 | opcode, (128 if masked else 0) | (length if length < 126 else 126)])
    if length >= 126:
        prefix += struct.pack("!H", length)
    writer.write(prefix + mask + (bytes(v ^ mask[i % 4] for i, v in enumerate(payload)) if mask else payload))
    await writer.drain()


async def websocket(base, path, token, request_id):
    nonce = base64.b64encode(secrets.token_bytes(16)).decode()
    response = await request(base, path, token, extra={
        "Connection": "Upgrade", "Upgrade": "websocket", "Sec-WebSocket-Version": "13",
        "Sec-WebSocket-Key": nonce, "x-request-id": request_id})
    accept = base64.b64encode(hashlib.sha1((nonce + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
    if response.status != 101 or response.fields.get("sec-websocket-accept") != accept:
        await response.close()
        raise ProtocolError("websocket_handshake")
    return response


async def probe(socket, sequence):
    payload = f"synthetic-probe-{sequence}".encode()
    await frame_write(socket.writer, 1, payload, masked=True)
    opcode, echoed = await asyncio.wait_for(frame_read(socket.reader), 5)
    if opcode != 1 or echoed != payload:
        raise ProtocolError("websocket_echo")


def admission_denied(status, body):
    if status != 503:
        return False
    try:
        return json.loads(body).get("error", {}).get("code") == "gateway_overloaded"
    except (ValueError, AttributeError):
        return False


async def close_socket(socket):
    try:
        await frame_write(socket.writer, 8, struct.pack("!H", 1000), masked=True)
        await asyncio.wait_for(frame_read(socket.reader), 3)
    except (OSError, EOFError, ValueError, asyncio.TimeoutError, ProtocolError):
        pass
    finally:
        await socket.close()


async def first_event(response):
    pending = bytearray()
    async for chunk in response.chunks():
        pending.extend(chunk)
        if len(pending) > MAX_RESPONSE:
            raise ProtocolError("observer_frame_bound")
        if b"\n\n" in pending:
            block = pending.split(b"\n\n", 1)[0]
            line = next((line[6:] for line in block.splitlines() if line.startswith(b"data: ")), None)
            if line:
                return json.loads(line)
    raise ProtocolError("observer_event_missing")


class Upstream:
    def __init__(self, credential, delay):
        self.credential, self.delay = credential, delay
        self.counts = Counter()

    async def handle(self, reader, writer):
        try:
            first, fields = await headers(reader)
            path = urllib.parse.urlsplit(first.split()[1]).path
            size = int(fields.get("content-length", 0))
            if size > 1024 * 1024:
                raise ProtocolError("request_bound")
            if size:
                await reader.readexactly(size)
            if fields.get("authorization") != "Bearer " + self.credential:
                self.counts["credential_translation_failures"] += 1
                writer.write(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
            elif fields.get("upgrade", "").lower() == "websocket":
                self.counts["websocket"] += 1
                accept = base64.b64encode(hashlib.sha1((fields["sec-websocket-key"] +
                    "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
                writer.write(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                    "Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + "\r\n\r\n").encode())
                await writer.drain()
                while True:
                    opcode, payload = await frame_read(reader)
                    await frame_write(writer, 10 if opcode == 9 else opcode, payload)
                    if opcode == 8:
                        break
            elif path in {"/sse", "/sse-interrupted"}:
                self.counts[path[1:]] += 1
                writer.write(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n"
                             b"Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
                for index in range(4):
                    event = b'data: {"choices":[{"delta":{"content":"synthetic"}}]}\n\n'
                    writer.write(f"{len(event):x}\r\n".encode() + event + b"\r\n")
                    await writer.drain()
                    await asyncio.sleep(.3)
                    if path == "/sse-interrupted":
                        # Deliberately truncate the HTTP body after a real 200 and SSE event.
                        writer.transport.abort()
                        return
                event = b"data: [DONE]\n\n"
                writer.write(f"{len(event):x}\r\n".encode() + event + b"\r\n0\r\n\r\n")
            else:
                self.counts["http"] += 1
                await asyncio.sleep(self.delay)
                body = b'{"ok":true}'
                writer.write(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
                             + str(len(body)).encode() + b"\r\nConnection: close\r\n\r\n" + body)
            await writer.drain()
        except (OSError, EOFError, ValueError, KeyError, ProtocolError):
            self.counts["connection_ended"] += 1
        finally:
            writer.close()
            with contextlib.suppress(OSError):
                await writer.wait_closed()


class Soak:
    def __init__(self, args, token):
        self.args, self.token = args, token
        self.run_id = uuid.uuid4().hex[:12]
        self.name = "monitoring-soak-" + self.run_id
        self.credential = secrets.token_urlsafe(32)
        self.canary = "body-query-canary-" + self.run_id
        self.secrets = [token, self.credential, self.canary]
        self.stop = asyncio.Event()
        self.counts = [Counter(), Counter()]
        self.failures = Counter()
        self.socket_ids = [f"soak-{self.run_id}-quiet-{i}" for i in range(2)]
        self.last_sse = [{}, {}]
        self.last_denial = [None, None]
        self.observer = [Counter(), Counter()]
        self.metrics = [{"rss_start": None, "rss_peak": None, "capacity_peak": 0},
                        {"rss_start": None, "rss_peak": None, "capacity_peak": 0}]
        self.progress = [0, 0]
        self.sample_lock = asyncio.Lock()
        self.checkpoints = []
        self.samples = 0
        self.fleet = Counter()
        self.started = time.monotonic()
        self.output = Path(args.output)
        self.output.mkdir(parents=True, exist_ok=False)
        self.evidence = (self.output / "observations.ndjson").open("w")
        (self.output / "harness.py").write_bytes(Path(__file__).read_bytes())

    def check_private(self, data):
        encoded = json.dumps(data)
        if any(secret in encoded for secret in self.secrets):
            raise ProtocolError("sensitive_observer_content")

    async def admin(self, index, path, method="GET", body=None):
        response = None
        try:
            async with asyncio.timeout(20):
                response = await request(self.args.control[index], "/admin-ui/admin/" + path,
                                         self.token, method, body)
                raw = await response.body()
                if response.status not in {200, 201, 204}:
                    raise ProtocolError("admin_status_" + str(response.status))
                return json.loads(raw) if raw else None
        finally:
            if response:
                await response.close()

    async def fixtures(self, port):
        self.project = await self.admin(0, "projects", "POST", {"name": self.name})
        self.layer = await self.admin(0, "policy-layers", "POST", {
            "kind": "project", "scope_id": self.project["id"], "policy": {}, "guardrail_policy": {}})
        await self.admin(0, "services", "POST", {
            "name": self.name, "project_id": self.project["id"],
            "upstream_base_url": f"http://127.0.0.1:{port}", "credential": self.credential,
            "allowed_methods": ["GET", "POST"], "timeout_ms": 600000, "cost_mode": "none",
            "access": {"websocket": {"idle_timeout_ms": 600000, "max_connections": 2,
                "max_connections_per_key": 2, "max_frame_bytes": 65536,
                "max_session_duration_seconds": 86400}}})
        created = await self.admin(0, "keys", "POST", {
            "name": self.name, "owner_type": "project", "project_id": self.project["id"],
            "service_names": [self.name], "policy": {"allowed_routes": ["/services/*"],
                "allowed_providers": ["internal-service"], "allowed_services": [self.name],
                "allow_streaming": True, "allow_tools": True, "rpm_limit": 1000000}})
        self.key, self.key_id = created["raw_key"], created["key"]["id"]
        self.secrets.append(self.key)
        self.prefix = "/services/" + self.name
        await asyncio.sleep(2)  # Allow the other process's normal registry refresh.

    async def http_worker(self, index, worker):
        sequence = 0
        period = 2 * self.args.workers / self.args.http_rate
        while not self.stop.is_set():
            began = time.monotonic()
            response = None
            request_id = f"soak-{self.run_id}-http-{index}-{worker}-{sequence}"
            try:
                async with asyncio.timeout(10):
                    response = await request(self.args.proxy[index], self.prefix + "/http?canary=" + self.canary,
                        self.key, "POST", {"synthetic": self.canary},
                        {"x-request-id": request_id})
                    body = await response.body()
                    if admission_denied(response.status, body):
                        self.counts[index]["http_admission_denied"] += 1
                        self.last_denial[index] = request_id
                    elif response.status != 200 or json.loads(body) != {"ok": True}:
                        self.counts[index]["http_status_" + str(response.status)] += 1
                        raise ProtocolError("http_outcome")
                    else:
                        self.counts[index]["http_success"] += 1
            except (OSError, EOFError, ValueError, asyncio.TimeoutError, ProtocolError) as error:
                self.counts[index]["http_failure"] += 1
                self.counts[index]["http_error_" + type(error).__name__] += 1
            finally:
                if response:
                    await response.close()
            sequence += 1
            await asyncio.sleep(max(0, period - (time.monotonic() - began)))

    async def sse_worker(self, index):
        sequence = 0
        while not self.stop.is_set():
            interrupted = sequence % 4 == 3
            kind = "interrupted" if interrupted else "success"
            request_id = f"soak-{self.run_id}-sse-{index}-{sequence}"
            response, data, transport_failed, denied = None, bytearray(), False, False
            try:
                async with asyncio.timeout(10):
                    response = await request(self.args.proxy[index], self.prefix +
                        ("/sse-interrupted" if interrupted else "/sse"), self.key, "POST",
                        {"stream": True, "synthetic": self.canary}, {"x-request-id": request_id})
                    if response.status != 200:
                        denied = admission_denied(response.status, await response.body())
                        if not denied:
                            self.counts[index]["sse_status_" + str(response.status)] += 1
                    else:
                        async for chunk in response.chunks():
                            data.extend(chunk)
                            if len(data) > 65536:
                                raise ProtocolError("sse_bound")
            except (OSError, EOFError, ValueError, asyncio.TimeoutError, ProtocolError) as error:
                transport_failed = True
                self.counts[index]["sse_transport_" + type(error).__name__] += 1
            finally:
                if response:
                    await response.close()
            valid = response and response.status == 200 and b"data: " in data
            valid = valid and ((b"[DONE]" not in data) if interrupted else
                               (b"[DONE]" in data and not transport_failed))
            if denied:
                self.counts[index]["sse_admission_denied"] += 1
                self.last_denial[index] = request_id
            elif valid:
                self.counts[index]["sse_" + kind] += 1
                self.last_sse[index][kind] = request_id
            else:
                self.counts[index]["sse_failure"] += 1
            sequence += 1
            await asyncio.sleep(self.args.sse_interval)

    async def journal(self, index):
        cursor = None
        while not self.stop.is_set():
            response = None
            try:
                response = await asyncio.wait_for(request(self.args.control[index],
                    "/admin-ui/admin/traffic/live?snapshot=true&project_id=" + self.project["id"],
                    self.token, extra={"Last-Event-ID": cursor} if cursor else None), 10)
                if response.status != 200 or "no-store" not in response.fields.get("cache-control", ""):
                    raise ProtocolError("observer_headers")
                self.observer[index]["connections"] += 1
                pending = bytearray()
                iterator = response.chunks().__aiter__()
                while not self.stop.is_set():
                    chunk = await asyncio.wait_for(anext(iterator), 12)
                    pending.extend(chunk)
                    if len(pending) > MAX_RESPONSE:
                        raise ProtocolError("observer_frame_bound")
                    while b"\n\n" in pending:
                        block, rest = pending.split(b"\n\n", 1)
                        pending = bytearray(rest)
                        line = next((line[6:] for line in block.splitlines() if line.startswith(b"data: ")), None)
                        if line is None:
                            continue
                        batch = json.loads(line)
                        self.check_private(batch)
                        self.observer[index]["batches"] += 1
                        self.observer[index]["gaps"] += bool(batch.get("gap"))
                        self.observer[index]["rows"] += len(batch.get("rows", []))
                        cursor = batch["cursor"]
                        if batch["instance_id"] != self.instances[index]:
                            raise ProtocolError("observer_instance_changed")
                        for row in batch.get("rows", []):
                            if len(row.get("timeline", [])) > 32 or len(row.get("upstream_timings", [])) > 32:
                                raise ProtocolError("observer_metadata_bound")
            except StopAsyncIteration:
                self.observer[index]["lease_ends"] += 1
            except (OSError, EOFError, ValueError, KeyError, asyncio.TimeoutError, ProtocolError) as error:
                self.observer[index]["errors"] += 1
                self.observer[index]["error_" + (str(error) if isinstance(error, ProtocolError) else type(error).__name__)] += 1
            finally:
                if response:
                    await response.close()
            await asyncio.sleep(.1)

    async def sample(self, phase):
        async with self.sample_lock:
            return await self._sample(phase)

    async def _sample(self, phase):
        statuses = await asyncio.gather(*(self.admin(i, "gateway/status?instance_id=all&window_seconds=60") for i in range(2)))
        selected = await asyncio.gather(*(self.admin(0, "traffic/cluster?instance_id=" + instance
            + "&project_id=" + self.project["id"] + "&limit=200") for instance in self.instances))
        # Scope the two publication reads before the fleet byte budget. Unrelated
        # integration-test publishers may be stale or saturate an all-fleet read.
        traffic = {"scope": "selected_instances", "sampled_at": selected[0]["sampled_at"],
            "partial": any(value["partial"] for value in selected),
            "unavailable": any(value["unavailable"] for value in selected),
            "observed_instances": sum(value["observed_instances"] for value in selected),
            "instances": [row for value in selected for row in value["instances"]],
            "rows": [row for value in selected for row in value["rows"]],
            "counts": {protocol: sum(value["counts"][protocol] or 0 for value in selected)
                if all(value["counts"][protocol] is not None for value in selected) else None
                for protocol in ("http", "websocket")},
            "matching": sum(value["matching"] for value in selected)
                if all(value["matching"] is not None for value in selected) else None,
            "has_more": any(value["has_more"] for value in selected),
            "truncated": any(value["truncated"] for value in selected)}
        self.check_private([statuses, traffic])
        wanted = set(self.instances)
        for status in statuses:
            fresh = {row["instance_id"] for row in status["instances"] if not row["stale"]}
            if not wanted.issubset(fresh):
                self.failures["status_coverage"] += 1
        fleet = [{"instance_id": row["instance_id"], "stale": row["stale"],
                  "selected": row["instance_id"] in wanted} for row in statuses[0]["instances"]]
        self.fleet["partial_samples"] += bool(statuses[0]["partial"])
        self.fleet["unavailable_samples"] += bool(statuses[0]["unavailable"])
        self.fleet["extra_publisher_peak"] = max(self.fleet["extra_publisher_peak"], sum(not row["selected"] for row in fleet))
        self.fleet["stale_publisher_peak"] = max(self.fleet["stale_publisher_peak"], sum(row["stale"] for row in fleet))
        if traffic["partial"] or traffic["unavailable"] or traffic["observed_instances"] != 2:
            self.failures["traffic_coverage"] += 1
        for value in traffic["instances"]:
            for protocol in ("http", "websocket"):
                coverage = value["summary"][protocol]
                if coverage["retained"] > coverage["record_limit"] or coverage["retained_bytes"] > coverage["byte_limit"]:
                    self.failures["active_memory_bound"] += 1
        rows = {row["request_id"]: row for row in traffic["rows"]}
        if phase == "load":
            for index, request_id in enumerate(self.socket_ids):
                row = rows.get(request_id)
                if not row or row["instance_id"] != self.instances[index]:
                    self.failures["quiet_socket_missing"] += 1
                else:
                    self.progress[index] = row["diagnostics"]["websocket"]["client_frames"]
        projected = []
        for index, instance in enumerate(self.instances):
            local = next((row for row in statuses[index]["instances"] if row["instance_id"] == instance), None)
            if local is None or local["runtime"] is None:
                raise ProtocolError("selected_status_missing")
            runtime = local["runtime"]
            capacity = runtime["body_capacity"]
            if capacity and (capacity["used_requests"] > capacity["max_requests"] or capacity["used_bytes"] > capacity["max_bytes"]):
                self.failures["body_capacity_bound"] += 1
            self.metrics[index]["capacity_peak"] = max(self.metrics[index]["capacity_peak"], (capacity or {}).get("used_requests", 0))
            rss = runtime["process"]["rss_bytes"]
            if rss is None and self.args.gateway_pid:
                result = subprocess.run(["ps", "-o", "rss=", "-p", str(self.args.gateway_pid[index])], capture_output=True, text=True)
                rss = int(result.stdout.strip()) * 1024 if result.stdout.strip().isdigit() else None
            if rss is not None:
                metrics = self.metrics[index]
                metrics["rss_start"] = rss if metrics["rss_start"] is None else metrics["rss_start"]
                metrics["rss_peak"] = max(metrics["rss_peak"] or 0, rss)
                if rss - metrics["rss_start"] > self.args.max_rss_growth_mib * 1024 * 1024:
                    self.failures["rss_growth_bound"] += 1
            projected.append({"instance_id": instance, "runtime": runtime, "native_rss_bytes": rss})
        self.samples += 1
        self.evidence.write(json.dumps({"elapsed_seconds": round(time.monotonic() - self.started, 3),
            "phase": phase, "status": projected,
            "fleet": {"partial": statuses[0]["partial"], "unavailable": statuses[0]["unavailable"], "publishers": fleet},
            "traffic": {k: traffic[k] for k in ("scope", "sampled_at", "partial", "unavailable", "counts", "observed_instances", "instances", "matching", "has_more", "truncated")},
            "quiet_sockets": [{"request_id": request_id, "present": request_id in rows,
                "websocket": rows.get(request_id, {}).get("diagnostics", {}).get("websocket")}
                for request_id in self.socket_ids]}) + "\n")
        self.evidence.flush()
        return projected, traffic

    async def poll(self):
        while not self.stop.is_set():
            try:
                await self.sample("load")
            except (OSError, EOFError, ValueError, KeyError, StopIteration, asyncio.TimeoutError, ProtocolError):
                self.failures["observer_sample"] += 1
            await asyncio.sleep(self.args.poll_seconds)

    async def terminal(self, request_id):
        query = urllib.parse.urlencode({"request_id": request_id, "time_basis": "completed", "limit": 5})
        for _ in range(20):
            history = await self.admin(0, "traffic/history?" + query)
            self.check_private(history)
            rows = history if isinstance(history, list) else history.get("rows", [])
            found = next((row for row in rows if row["request_id"] == request_id), None)
            if found:
                return found
            await asyncio.sleep(.5)
        raise ProtocolError("terminal_record_missing")

    async def renew_sockets(self, sockets, terminal):
        # Keep HTTP/SSE load and journal observers running. Serialize sampling
        # across this explicit make-before-break transition, then validate the
        # replacements through publication. This is not failure reconnection.
        async with self.sample_lock:
            for index, previous in enumerate(sockets):
                request_id = f"soak-{self.run_id}-renewed-{index}-{self.counts[index]['socket_renewals'] + 1}"
                replacement = None
                try:
                    replacement = await asyncio.wait_for(websocket(
                        self.args.proxy[index], self.prefix + "/ws", self.key, request_id), 10)
                    await probe(replacement, 0)
                    await close_socket(previous)
                    row = await self.terminal(self.socket_ids[index])
                    self.check_private(row)
                    diagnostics = row["diagnostics"]
                    if (not row.get("completed_at")
                            or diagnostics.get("instance_id") != self.instances[index]
                            or diagnostics.get("outcome") != "completed"
                            or not diagnostics.get("websocket", {}).get("client_frames")):
                        raise ProtocolError("renewed_socket_terminal_invalid")
                    terminal.append({"kind": "websocket_renewed", "request_id": self.socket_ids[index],
                        "client_status": row.get("client_status"), "completed_at": row["completed_at"],
                        "diagnostics": diagnostics})
                    sockets[index] = replacement
                    replacement = None
                    self.socket_ids[index] = request_id
                    self.progress[index] = 0
                    self.counts[index]["socket_renewals"] += 1
                finally:
                    if replacement is not None:
                        await close_socket(replacement)
            await asyncio.sleep(2)
            await self._sample("load")

    async def run(self):
        upstream = Upstream(self.credential, self.args.http_delay_ms / 1000)
        server = await asyncio.start_server(upstream.handle, "127.0.0.1", 0, limit=MAX_RESPONSE)
        sockets, tasks, terminal = [], [], []
        report = {"started_at": dt.datetime.now(dt.timezone.utc).isoformat(), "run_id": self.run_id,
            "head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
            "dirty_candidate": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True)),
            "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "platform": platform.platform(), "architecture": platform.machine(),
            "requested_seconds": self.args.seconds, "http_rate_total": self.args.http_rate,
            "workers_per_instance": self.args.workers, "rss_growth_limit_mib": self.args.max_rss_growth_mib,
            "socket_renew_seconds": self.args.socket_renew_seconds,
            "gateway_pids": self.args.gateway_pid,
            "concurrent_load_note": self.args.concurrent_load_note,
            "limitations": ["Synthetic loopback upstream; no real provider inference or ingress/TLS.",
                "RSS growth and registry bounds are finite observations, not a leak proof or production capacity.",
                "No database/Redis outage injection; no direct injection of internal post-cancel callbacks.",
                "A short run is not a 24-hour pass. Native macOS process/cgroup metrics may be unavailable."]}
        try:
            await self.fixtures(server.sockets[0].getsockname()[1])
            self.instances = []
            for index in range(2):
                # The process-local journal identifies the control/proxy pair without Redis guesses.
                response = await request(self.args.control[index], "/admin-ui/admin/traffic/live", self.token)
                try:
                    batch = await asyncio.wait_for(first_event(response), 12)
                    self.check_private(batch)
                    self.instances.append(batch["instance_id"])
                finally:
                    await response.close()
            if len(set(self.instances)) != 2:
                raise ProtocolError("two_distinct_instances_required")
            report["instances"] = self.instances
            report["fixture_project_id"] = self.project["id"]
            report["fixture_service"] = self.name
            for index in range(2):
                socket = await asyncio.wait_for(websocket(self.args.proxy[index], self.prefix + "/ws", self.key, self.socket_ids[index]), 10)
                sockets.append(socket)
                await probe(socket, 0)
            await asyncio.sleep(2)
            await self.sample("load")
            tasks = [asyncio.create_task(self.http_worker(i, worker)) for i in range(2) for worker in range(self.args.workers)]
            tasks += [asyncio.create_task(self.sse_worker(i)) for i in range(2)]
            tasks += [asyncio.create_task(self.journal(i)) for i in range(2)]
            tasks += [asyncio.create_task(self.poll())]
            load_start = time.monotonic()
            next_probe = load_start + min(self.args.quiet_seconds, self.args.seconds * .75)
            next_renewal = load_start + self.args.socket_renew_seconds
            while time.monotonic() - load_start < self.args.seconds:
                await asyncio.sleep(min(1, max(.01, load_start + self.args.seconds - time.monotonic())))
                if time.monotonic() >= next_renewal:
                    await self.renew_sockets(sockets, terminal)
                    next_renewal = time.monotonic() + self.args.socket_renew_seconds
                if time.monotonic() >= next_probe:
                    before = list(self.progress)
                    checkpoint = {"elapsed_load_seconds": round(time.monotonic() - load_start, 3),
                        "http_successes_before": [counts["http_success"] for counts in self.counts],
                        "client_frames_before": before}
                    for index, socket in enumerate(sockets):
                        await probe(socket, self.counts[index]["http_success"])
                        self.counts[index]["late_probes"] += 1
                    await asyncio.sleep(max(2, self.args.poll_seconds * 2))
                    await self.sample("load")
                    if any(after <= previous for after, previous in zip(self.progress, before)):
                        self.failures["late_progress_missing"] += 1
                    checkpoint["client_frames_after"] = list(self.progress)
                    self.checkpoints.append(checkpoint)
                    next_probe = time.monotonic() + self.args.quiet_seconds
            report["load_elapsed_seconds"] = round(time.monotonic() - load_start, 3)
            self.stop.set()
            done, pending = await asyncio.wait(tasks, timeout=15)
            for task in pending:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            if pending or any(task.exception() for task in done if not task.cancelled()):
                self.failures["worker_shutdown"] += 1
            for index, socket in enumerate(sockets):
                await close_socket(socket)
                # Opening both allowed slots proves closed sessions release admission.
                recovered = []
                try:
                    for slot in range(2):
                        reopened = await websocket(self.args.proxy[index], self.prefix + "/ws", self.key,
                            f"soak-{self.run_id}-recovered-{index}-{slot}")
                        recovered.append(reopened)
                        await probe(reopened, slot)
                    self.counts[index]["socket_slots_recovered"] = len(recovered)
                finally:
                    for reopened in recovered:
                        await close_socket(reopened)
            sockets.clear()
            await asyncio.sleep(3)
            final, active = await self.sample("recovery")
            if active["matching"] != 0 or active["rows"]:
                self.failures["active_did_not_drain"] += 1
            for value in final:
                capacity = value["runtime"]["body_capacity"]
                if capacity and (capacity["used_requests"] or capacity["used_bytes"]):
                    self.failures["body_capacity_did_not_drain"] += 1
                if any(value["runtime"]["active"].values()):
                    self.failures["runtime_active_did_not_drain"] += 1
            for index in range(2):
                records = {"websocket": self.socket_ids[index], **self.last_sse[index]}
                if self.last_denial[index]:
                    records["admission_denial"] = self.last_denial[index]
                for kind, request_id in records.items():
                    row = await self.terminal(request_id)
                    diagnostics = row["diagnostics"]
                    if not row.get("completed_at") or diagnostics.get("instance_id") != self.instances[index]:
                        self.failures["terminal_identity_or_completion"] += 1
                    expected = "completed" if kind == "success" else "stream_interrupted" if kind == "interrupted" else None
                    if expected and diagnostics.get("outcome") != expected:
                        self.failures["terminal_sse_outcome"] += 1
                    if kind in {"success", "interrupted"} and row.get("client_status") != 200:
                        self.failures["terminal_sse_status"] += 1
                    if kind == "admission_denial" and (row.get("client_status") != 503
                            or diagnostics.get("failure_code") != "gateway_overloaded"
                            or diagnostics.get("outcome") != "failed"):
                        self.failures["terminal_admission_denial"] += 1
                    if kind == "websocket" and not diagnostics.get("websocket", {}).get("client_frames"):
                        self.failures["terminal_websocket_progress"] += 1
                    if kind == "websocket" and diagnostics.get("outcome") != "completed":
                        self.failures["terminal_websocket_outcome"] += 1
                    terminal.append({"kind": kind, "request_id": request_id,
                        "client_status": row.get("client_status"),
                        "completed_at": row.get("completed_at"), "diagnostics": diagnostics})
                counts = self.counts[index]
                if counts["http_success"] < self.args.min_http_per_instance:
                    self.failures["insufficient_http_churn"] += 1
                if any(counts[key] == 0 for key in ("late_probes", "sse_success", "sse_interrupted")):
                    self.failures["protocol_coverage"] += 1
                if counts["http_failure"] or counts["sse_failure"] or self.observer[index]["errors"]:
                    self.failures["request_or_journal_errors"] += 1
                if not self.observer[index]["lease_ends"] or self.observer[index]["connections"] < 2:
                    self.failures["observer_lease_renewal_not_exercised"] += 1
        except asyncio.CancelledError:
            self.failures["run_cancelled"] += 1
            raise
        except Exception as error:
            # Only fixed harness codes are safe; never serialize transport exceptions.
            self.failures["run_" + (str(error) if isinstance(error, ProtocolError) else type(error).__name__)] += 1
        finally:
            self.stop.set()
            for task in tasks:
                if not task.done():
                    task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            for socket in sockets:
                await close_socket(socket)
            # Preserve diagnostic history; disable only this run's credential and service.
            for path in ([f"keys/{self.key_id}/disable"] if hasattr(self, "key_id") else []) + (
                    [f"services/{self.name}/disable"] if hasattr(self, "project") else []):
                try:
                    await self.admin(0, path, "POST")
                except (OSError, EOFError, ValueError, asyncio.TimeoutError, ProtocolError):
                    self.failures["fixture_cleanup"] += 1
            server.close()
            await server.wait_closed()
            self.evidence.close()
            report.update({"elapsed_seconds": round(time.monotonic() - self.started, 3),
                "counts": [dict(value) for value in self.counts], "observer": [dict(value) for value in self.observer],
                "metrics": self.metrics, "samples": self.samples, "terminal_records": terminal,
                "probe_checkpoints": self.checkpoints,
                "fleet_observations": dict(self.fleet),
                "upstream": dict(upstream.counts), "failures": dict(self.failures),
                "passed": not self.failures, "completed_24_hours": report.get("load_elapsed_seconds", 0) >= 86400})
            if upstream.counts["credential_translation_failures"]:
                report["passed"] = False
                report["failures"]["credential_translation"] = upstream.counts["credential_translation_failures"]
            self.check_private(report)
            (self.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"passed": report["passed"], "elapsed_seconds": report["elapsed_seconds"],
            "http_successes": [counts["http_success"] for counts in self.counts],
            "failures": report["failures"], "report": str(self.output / "report.json")}))
        return 0 if report["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-fixtures", action="store_true")
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--proxy", nargs=2, default=["http://127.0.0.1:58080", "http://127.0.0.1:58180"])
    parser.add_argument("--control", nargs=2, default=["http://127.0.0.1:58081", "http://127.0.0.1:58181"])
    parser.add_argument("--seconds", type=int, default=300)
    parser.add_argument("--http-rate", type=float, default=20)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--quiet-seconds", type=int, default=240)
    parser.add_argument("--socket-renew-seconds", type=int, default=82800,
                        help="explicit make-before-break renewal before the 24-hour session cap (default: 23 hours)")
    parser.add_argument("--poll-seconds", type=float, default=2)
    parser.add_argument("--sse-interval", type=float, default=2)
    parser.add_argument("--http-delay-ms", type=int, default=50)
    parser.add_argument("--min-http-per-instance", type=int, default=1000)
    parser.add_argument("--max-rss-growth-mib", type=int, default=128)
    parser.add_argument("--gateway-pid", nargs=2, type=int)
    parser.add_argument("--concurrent-load-note", default="No concurrent load declared")
    parser.add_argument("--output", default="output/monitoring/soak-" + dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    args = parser.parse_args()
    if not args.allow_fixtures:
        parser.error("--allow-fixtures is required for disposable gateways")
    for value in args.proxy + args.control:
        target = urllib.parse.urlsplit(value)
        if target.scheme != "http" or target.hostname not in {"127.0.0.1", "localhost", "::1"} or not target.port or target.username or target.password or target.path or target.query or target.fragment:
            parser.error("all targets must be plain loopback HTTP origins")
    if len(set(args.proxy)) != 2 or len(set(args.control)) != 2:
        parser.error("two distinct proxy/control pairs are required")
    if not (30 <= args.seconds <= 86400 and 1 <= args.http_rate <= 200 and 1 <= args.workers <= 16
            and 5 <= args.quiet_seconds <= 300 and .5 <= args.poll_seconds <= 10
            and 60 <= args.socket_renew_seconds <= 82800
            and .1 <= args.sse_interval <= 30 and 0 <= args.http_delay_ms <= 1000
            and 0 <= args.min_http_per_instance <= 10000000 and 1 <= args.max_rss_growth_mib <= 4096):
        parser.error("duration, load, or observation settings outside bounded test limits")
    token_path = Path(args.token_file)
    if not token_path.is_file() or token_path.stat().st_mode & 0o077:
        parser.error("token file must be private (chmod 600)")
    output = Path(args.output).resolve()
    root = Path.cwd().resolve()
    if not any(output.is_relative_to(root / name) for name in ("output", "target")):
        parser.error("evidence must be inside this checkout's output/ or target/")
    if output.exists():
        parser.error("output directory already exists; use a fresh evidence directory")
    token = token_path.read_text().strip()
    if not token or any(character in token for character in "\r\n"):
        parser.error("token file is empty or malformed")
    raise SystemExit(asyncio.run(Soak(args, token).run()))


if __name__ == "__main__":
    main()

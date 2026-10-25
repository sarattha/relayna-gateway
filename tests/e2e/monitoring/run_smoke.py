#!/usr/bin/env python3
"""Run the bounded CI protocol check with entirely disposable local fixtures."""
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import urllib.error
import urllib.parse
import urllib.request

import scale


class SmokeError(Exception):
    """Fixed diagnostic codes only; transport exceptions can contain secrets."""


def redact(text, private_values):
    for value in private_values:
        if value:
            text = text.replace(value, "[REDACTED]")
    text = re.sub(r"\b(?:rk_live|op_live)_[A-Za-z0-9_=-]{16,}\b", "[REDACTED]", text)
    text = re.sub(r"(?i)Bearer\s+[A-Za-z0-9._~+/=-]+", "Bearer [REDACTED]", text)
    return re.sub(r'\?[^\s"\\]+', "?[REDACTED]", text)


def private_file(path, text):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        stream.write(text)


def loopback_ports(count):
    reservations = [socket.socket() for _ in range(count)]
    try:
        for reservation in reservations:
            reservation.bind(("127.0.0.1", 0))
        return [reservation.getsockname()[1] for reservation in reservations]
    finally:
        for reservation in reservations:
            reservation.close()


def validate_report(report, seconds):
    if not report.get("passed") or report.get("failures"):
        raise SmokeError("protocol_check_failed")
    if report.get("requested_seconds") != seconds or report.get("load_elapsed_seconds", 0) < seconds:
        raise SmokeError("continuous_interval_incomplete")
    counts = report.get("counts", [])
    if len(counts) != 2:
        raise SmokeError("two_instance_coverage_missing")
    if seconds > 330 and not all(row.get("socket_renewals", 0) for row in counts):
        raise SmokeError("socket_renewal_not_exercised")


class Fixtures:
    def __init__(self, binary, output, temporary):
        self.binary, self.output, self.temporary = binary, output, temporary
        self.run_id = secrets.token_hex(6)
        self.operator = "op_live_" + secrets.token_urlsafe(36)
        self.password, self.upstream_key = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
        self.private = [self.operator, self.password, self.upstream_key]
        self.containers, self.gateways, self.loggers = [], [], []
        self.images = {}
        self.cleanup_errors = []
        self.token_file = temporary / "operator-token"
        private_file(self.token_file, self.operator)

    def docker(self, *arguments, input_text=None):
        result = subprocess.run(["docker", *arguments], input=input_text, text=True,
                                capture_output=True, timeout=90)
        if result.returncode:
            with (self.output / "fixture-errors.log").open("a") as stream:
                stream.write(redact(result.stderr, self.private))
            raise SmokeError("docker_fixture_failed")
        return result.stdout.strip()

    def create_dependencies(self):
        self.postgres = "relayna-monitoring-smoke-postgres-" + self.run_id
        redis = "relayna-monitoring-smoke-redis-" + self.run_id
        for name, port, image, extra in [
            (self.postgres, 5432, "postgres:16-alpine", ["-e", "POSTGRES_USER=relayna_test",
                "-e", "POSTGRES_DB=relayna_monitoring", "-e", "POSTGRES_PASSWORD=" + self.password]),
            (redis, 6379, "redis:7-alpine", []),
        ]:
            self.containers.append(name)
            command = ["run", "--rm", "--detach", "--name", name,
                       "--publish", f"127.0.0.1::{port}", *extra, image]
            if port == 6379:
                command += ["redis-server", "--save", "", "--appendonly", "no"]
            self.docker(*command)
            self.images[name] = self.docker("inspect", "--format", "{{.Image}}", name)
        for _ in range(120):
            result = subprocess.run(["docker", "exec", self.postgres, "pg_isready", "-U",
                                     "relayna_test", "-d", "relayna_monitoring"],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if result.returncode == 0:
                break
            time.sleep(.25)
        else:
            raise SmokeError("postgres_not_ready")
        pg_port = self.docker("port", self.postgres, "5432/tcp").rsplit(":", 1)[1]
        redis_port = self.docker("port", redis, "6379/tcp").rsplit(":", 1)[1]
        self.database_url = f"postgres://relayna_test:{self.password}@127.0.0.1:{pg_port}/relayna_monitoring"
        self.redis_url = "redis://127.0.0.1:" + redis_port

    def log_output(self, process, path):
        with path.open("w") as stream:
            for line in process.stdout:
                stream.write(redact(line, self.private))
                stream.flush()

    def start_gateways(self):
        ports = loopback_ports(4)
        self.proxies = [f"http://127.0.0.1:{port}" for port in ports[:2]]
        self.controls = [f"http://127.0.0.1:{port}" for port in ports[2:]]
        environment = {key: os.environ[key] for key in ("PATH", "LANG", "LC_ALL", "TMPDIR", "TZ")
                       if key in os.environ}
        environment.update(DATABASE_URL=self.database_url, REDIS_URL=self.redis_url,
                           LITELLM_BASE_URL="http://127.0.0.1:9", LITELLM_SERVICE_KEY=self.upstream_key,
                           GATEWAY_ADMIN_TOKEN=self.operator, LOG_LEVEL="error",
                           GATEWAY_MAX_BUFFERED_REQUESTS="8",
                           GATEWAY_MAX_INFLIGHT_BUFFER_BYTES=str(512 * 1024 * 1024))
        for index in range(2):
            current = {**environment, "GATEWAY_BIND_ADDR": self.proxies[index].removeprefix("http://"),
                       "GATEWAY_CONTROL_BIND_ADDR": self.controls[index].removeprefix("http://")}
            process = subprocess.Popen([str(self.binary)], env=current, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, text=True, errors="replace")
            self.gateways.append(process)
            logger = threading.Thread(target=self.log_output,
                                      args=(process, self.output / f"gateway-{index}.log"), daemon=True)
            logger.start()
            self.loggers.append(logger)
        for _ in range(120):
            self.check_processes()
            try:
                for control in self.controls:
                    with urllib.request.urlopen(control + "/admin-ui/readyz", timeout=1) as response:
                        if response.status != 200:
                            raise SmokeError("gateway_not_ready")
                return
            except (OSError, SmokeError):
                time.sleep(.5)
        raise SmokeError("gateway_not_ready")

    def check_processes(self):
        if any(process.poll() is not None for process in self.gateways):
            raise SmokeError("gateway_exited")

    def neutral_policy(self):
        request = urllib.request.Request(self.controls[0] + "/admin-ui/admin/policy-layers",
            data=json.dumps({"kind": "global", "policy": {}, "guardrail_policy": {}}).encode(),
            headers={"Authorization": "Bearer " + self.operator, "Content-Type": "application/json"},
            method="POST")
        with urllib.request.urlopen(request, timeout=20) as response:
            if response.status not in {200, 201}:
                raise SmokeError("fixture_policy_failed")

    def usage_probe(self):
        now = dt.datetime.now(dt.timezone.utc)
        query = urllib.parse.urlencode({"from": (now - dt.timedelta(days=1)).isoformat(),
                                       "to": now.isoformat(), "limit": 20})
        rows = []
        for endpoint in ("summary", "events"):
            path = "/admin-ui/admin/usage/" + endpoint + "?" + query
            if endpoint == "events":
                path += "&cursor=start"
            result = scale.fetch(self.controls[0], path, self.operator)
            rows.append({"endpoint": endpoint, **result})
        return rows

    def close(self):
        for process in self.gateways:
            if process.poll() is None:
                # Traffic is already drained and terminals verified. Pingora's
                # SIGINT exits promptly; SIGTERM waits its production grace period.
                process.send_signal(signal.SIGINT)
        for process in self.gateways:
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
                self.cleanup_errors.append("gateway_shutdown_timeout")
        for logger in self.loggers:
            logger.join(timeout=5)
        for name in reversed(self.containers):
            try:
                result = subprocess.run(["docker", "stop", "--time", "10", name], text=True,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
                failed = result.returncode != 0
            except (OSError, subprocess.TimeoutExpired):
                failed = True
            if failed:
                self.cleanup_errors.append("owned_container_cleanup_failed")


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--binary", required=True)
    result.add_argument("--output", required=True)
    result.add_argument("--seconds", type=int, default=600,
                        help="600 for the CI gate; shorter local diagnostics are allowed, maximum 600")
    result.add_argument("--history-rows", type=int, default=100_000)
    return result


def main(argv=None):
    args = parser().parse_args(argv)
    if not 30 <= args.seconds <= 600 or not 1_000 <= args.history_rows <= 1_000_000:
        parser().error("CI checks allow 30–600 seconds and 1,000–1,000,000 historical rows; use soak.py for intentional longer runs")
    root, binary, output = Path.cwd().resolve(), Path(args.binary).resolve(), Path(args.output).resolve()
    if not binary.is_file() or not any(output.is_relative_to(root / name) for name in ("output", "target")):
        parser().error("a built binary and fresh evidence directory under output/ or target/ are required")
    if output.exists():
        parser().error("refusing to overwrite existing evidence")
    output.mkdir(parents=True)
    report = {"started_at": dt.datetime.now(dt.timezone.utc).isoformat(),
              "requested_seconds": args.seconds, "history_rows": args.history_rows,
              "head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
              "working_tree_dirty": bool(subprocess.check_output(
                  ["git", "status", "--porcelain"], text=True).strip()),
              "host": platform.platform(), "architecture": platform.machine(),
              "passed": False, "failures": [], "usage_probes": []}
    for name in ("run_smoke.py", "scale.py"):
        shutil.copy2(Path(__file__).with_name(name), output / name)
    interrupted, stop_usage = threading.Event(), threading.Event()
    child, fixtures, usage_thread = None, None, None
    previous_handlers = {}

    def interrupt(_signum, _frame):
        interrupted.set()
        if child is not None and child.poll() is None:
            child.send_signal(signal.SIGINT)

    for signum in (signal.SIGINT, signal.SIGTERM):
        previous_handlers[signum] = signal.signal(signum, interrupt)
    try:
        with tempfile.TemporaryDirectory(prefix="relayna-monitoring-smoke-private-") as temporary:
            immutable = Path(temporary) / "gateway-api"
            shutil.copy2(binary, immutable)
            immutable.chmod(0o700)
            report["binary_sha256"] = hashlib.sha256(immutable.read_bytes()).hexdigest()
            fixtures = Fixtures(immutable, output, Path(temporary))
            try:
                fixtures.create_dependencies()
                fixtures.start_gateways()
                fixtures.neutral_policy()
                scale.seed(SimpleNamespace(container=fixtures.postgres), args.history_rows)
                if interrupted.is_set():
                    raise SmokeError("continuous_check_interrupted_or_timed_out")

                def usage_reads():
                    while not stop_usage.is_set():
                        try:
                            rows = fixtures.usage_probe()
                            report["usage_probes"].extend(rows)
                        except Exception:
                            report["usage_probes"].append({"error": "usage_probe_failed"})
                        stop_usage.wait(30)

                usage_thread = threading.Thread(target=usage_reads, daemon=True)
                usage_thread.start()
                command = [sys.executable, str(Path(__file__).with_name("soak.py")), "--allow-fixtures",
                           "--token-file", str(fixtures.token_file), "--proxy", *fixtures.proxies,
                           "--control", *fixtures.controls, "--gateway-pid",
                           *(str(process.pid) for process in fixtures.gateways),
                           "--seconds", str(args.seconds), "--http-rate", "8", "--workers", "2",
                           "--quiet-seconds", str(min(240, max(5, args.seconds // 3))),
                           "--socket-renew-seconds", "330", "--min-http-per-instance",
                           str(args.seconds), "--concurrent-load-note",
                           "One bounded all-project Usage summary/events reader every 30 seconds",
                           "--output", str(output / "protocol")]
                with (output / "protocol.log").open("w") as log:
                    child = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
                    deadline = time.monotonic() + args.seconds + 180
                    while child.poll() is None:
                        fixtures.check_processes()
                        if interrupted.is_set() or time.monotonic() > deadline:
                            raise SmokeError("continuous_check_interrupted_or_timed_out")
                        time.sleep(.5)
                    if child.returncode:
                        raise SmokeError("protocol_check_failed")
                protocol = json.loads((output / "protocol/report.json").read_text())
                validate_report(protocol, args.seconds)
                report["load_elapsed_seconds"] = protocol["load_elapsed_seconds"]
                report["gateway_pids"] = [process.pid for process in fixtures.gateways]
                report["passed"] = True
            finally:
                report["owned_containers"] = list(fixtures.containers)
                report["dependency_images"] = dict(fixtures.images)
                report["gateway_pids"] = [process.pid for process in fixtures.gateways]
                stop_usage.set()
                if usage_thread:
                    usage_thread.join(timeout=75)
                    if usage_thread.is_alive():
                        report["failures"].append("usage_reader_shutdown_timeout")
                if child is not None and child.poll() is None:
                    child.send_signal(signal.SIGINT)
                    try:
                        child.wait(timeout=20)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=10)
                fixtures.close()
                report["failures"].extend(fixtures.cleanup_errors)
        if not report["usage_probes"] or any(row.get("status") != 200 for row in report["usage_probes"]):
            report["failures"].append("all_project_usage_failed")
        if report["failures"]:
            report["passed"] = False
    except Exception as error:
        report["passed"] = False
        report["failures"].append(str(error) if isinstance(error, SmokeError) else type(error).__name__)
    finally:
        for signum, handler in previous_handlers.items():
            signal.signal(signum, handler)
        report["finished_at"] = dt.datetime.now(dt.timezone.utc).isoformat()
        report["ci_gate_satisfied"] = report["passed"] and args.seconds == 600
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": report["passed"], "requested_seconds": args.seconds,
                      "failures": report["failures"], "report": str(output / "report.json")}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

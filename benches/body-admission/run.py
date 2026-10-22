#!/usr/bin/env python3
"""Reproducible two-pod body admission experiment using only Python's stdlib."""

import argparse
import datetime
import hashlib
import json
from pathlib import Path
import secrets
import subprocess
import time


HERE = Path(__file__).resolve().parent
NAMESPACE = "gateway-body-bench"
SAMPLER = r'''while :; do printf '%s %s %s %s %s\n' "$(date +%s%N)" "$(cat /sys/fs/cgroup/memory.current)" "$(awk '/^VmRSS:/{print $2 * 1024}' /proc/1/status)" "$(cat /sys/fs/cgroup/memory.peak)" "$(awk '/^throttled_usec /{print $2}' /sys/fs/cgroup/cpu.stat)"; sleep 0.25; done'''


def command(args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def deployment(name, image, *, env=None, command_args=None, ports=None, resources=None, volumes=False, replicas=1):
    container = {"name": name, "image": image, "imagePullPolicy": "IfNotPresent"}
    if env:
        container["env"] = [{"name": key, "value": str(value)} if not isinstance(value, dict) else {"name": key, **value} for key, value in env.items()]
    if command_args:
        container["command"] = command_args
    if ports:
        container["ports"] = [{"containerPort": port} for port in ports]
    if resources:
        container["resources"] = resources
    spec = {"containers": [container], "terminationGracePeriodSeconds": 5}
    if volumes:
        container["volumeMounts"] = [{"name": "scripts", "mountPath": "/bench", "readOnly": True}]
        spec["volumes"] = [{"name": "scripts", "configMap": {"name": "scripts"}}]
    if name == "gateway":
        container["readinessProbe"] = {"httpGet": {"path": "/admin-ui/readyz", "port": 8081}, "periodSeconds": 1, "timeoutSeconds": 2}
    metadata = {"labels": {"app": name}}
    if volumes:
        metadata["annotations"] = {"benchmark-script-sha256": hashlib.sha256(b"".join((HERE / filename).read_bytes() for filename in ["mock.mjs", "workload.mjs"])).hexdigest()}
    return {"apiVersion": "apps/v1", "kind": "Deployment", "metadata": {"name": name}, "spec": {
        "replicas": replicas, "selector": {"matchLabels": {"app": name}},
        "template": {"metadata": metadata, "spec": spec},
    }}


def service(name, port):
    return {"apiVersion": "v1", "kind": "Service", "metadata": {"name": name}, "spec": {"selector": {"app": name}, "ports": [{"port": port, "targetPort": port}]}}


class Benchmark:
    def __init__(self, args):
        self.args = args
        self.kubectl = ["kubectl", "--kubeconfig", str(Path(args.kubeconfig).resolve()), "-n", NAMESPACE]
        self.output = Path(args.output).resolve()
        self.output.mkdir(parents=True, exist_ok=True)

    def kube(self, *args, **kwargs):
        return command(self.kubectl + list(args), **kwargs)

    def apply(self, objects):
        self.kube("apply", "-f", "-", input=json.dumps({"apiVersion": "v1", "kind": "List", "items": objects}), stdout=subprocess.DEVNULL)

    def secret_ref(self, key):
        return {"valueFrom": {"secretKeyRef": {"name": "credentials", "key": key}}}

    def setup(self):
        self.apply([{"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": NAMESPACE}}])
        existing = self.kube("get", "secret", "credentials", "--ignore-not-found", "-o", "name", capture_output=True).stdout.strip()
        if not existing:
            self.apply([{"apiVersion": "v1", "kind": "Secret", "metadata": {"name": "credentials"}, "stringData": {
                "admin": "op_live_" + secrets.token_hex(24), "service": secrets.token_hex(24),
            }}])
        self.apply([
            {"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": "scripts"}, "data": {name: (HERE / name).read_text() for name in ["mock.mjs", "workload.mjs"]}},
            deployment("postgres", "postgres:16-alpine", env={"POSTGRES_USER": "benchmark", "POSTGRES_PASSWORD": "benchmark-local-only", "POSTGRES_DB": "benchmark"}),
            service("postgres", 5432),
            deployment("redis", "redis:7-alpine", command_args=["redis-server", "--save", "", "--appendonly", "no"]),
            service("redis", 6379),
            deployment("mock", "node:22.23.4-alpine", env={"SERVICE_TOKEN": self.secret_ref("service"), "DELAY_MS": self.args.delay_ms}, command_args=["node", "/bench/mock.mjs"], volumes=True),
            service("mock", 4000),
            deployment("driver", "node:22.23.4-alpine", env={"ADMIN_TOKEN": self.secret_ref("admin"), "SERVICE_TOKEN": self.secret_ref("service")}, command_args=["sleep", "infinity"], volumes=True),
        ])
        for name in ["postgres", "redis", "mock", "driver"]:
            self.kube("rollout", "status", f"deployment/{name}", "--timeout=180s", stdout=subprocess.DEVNULL)
        # Deployment readiness alone does not mean PostgreSQL accepts connections.
        for _ in range(60):
            ready = subprocess.run(self.kubectl + ["exec", "deployment/postgres", "--", "pg_isready", "-U", "benchmark"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if ready.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("PostgreSQL did not become ready")

    def gateways(self, limit):
        env = {
            "DATABASE_URL": "postgres://benchmark:benchmark-local-only@postgres:5432/benchmark",
            "REDIS_URL": "redis://redis:6379", "LITELLM_BASE_URL": "http://mock:4000",
            "LITELLM_SERVICE_KEY": self.secret_ref("service"), "GATEWAY_ADMIN_TOKEN": self.secret_ref("admin"),
            "GATEWAY_BIND_ADDR": "0.0.0.0:8080", "GATEWAY_CONTROL_BIND_ADDR": "0.0.0.0:8081",
            "GATEWAY_MAX_BUFFERED_REQUESTS": limit, "GATEWAY_MAX_INFLIGHT_BUFFER_BYTES": 512 * 1024 * 1024,
            "LOG_LEVEL": "error",
        }
        if self.args.malloc_mmap_threshold is not None:
            env["MALLOC_MMAP_THRESHOLD_"] = self.args.malloc_mmap_threshold
        self.apply([deployment("gateway", self.args.image, env=env, replicas=2, resources={
            "requests": {"cpu": "500m", "memory": "256Mi"}, "limits": {"cpu": "2", "memory": f"{self.args.memory_limit_mib}Mi"},
        })])
        self.kube("rollout", "restart", "deployment/gateway", stdout=subprocess.DEVNULL)
        self.kube("rollout", "status", "deployment/gateway", "--timeout=180s", stdout=subprocess.DEVNULL)
        for _ in range(60):
            pods = json.loads(self.kube("get", "pods", "-l", "app=gateway", "-o", "json", capture_output=True).stdout)["items"]
            if len(pods) == 2 and all(pod["status"].get("containerStatuses", [{}])[0].get("ready") for pod in pods):
                return sorted([{"name": pod["metadata"]["name"], "ip": pod["status"]["podIP"]} for pod in pods], key=lambda pod: pod["name"])
            time.sleep(1)
        raise RuntimeError("expected exactly two ready Gateway pods")

    def run(self):
        self.setup()
        metadata = {
            "label": self.args.label, "image": self.args.image, "source_commit": self.args.source_commit,
            "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "limits": self.args.limits, "repetitions": self.args.repetitions,
            "rps": self.args.rps, "duration_seconds": self.args.duration, "delay_ms": self.args.delay_ms,
            "gateway_resources": {"cpu_limit": 2, "memory_limit_bytes": self.args.memory_limit_mib * 1024 * 1024},
            "malloc_mmap_threshold_override": self.args.malloc_mmap_threshold,
            "docker": None,
        }
        if self.args.host_metadata:
            info = json.loads(command(["docker", "info", "--format", "{{json .}}"], capture_output=True).stdout)
            metadata["docker"] = {key: info[key] for key in ["NCPU", "MemTotal", "Architecture", "KernelVersion", "ServerVersion", "OperatingSystem"]}
        metadata["image_id"] = command(["docker", "image", "inspect", self.args.image, "--format", "{{.Id}}"], capture_output=True).stdout.strip()
        metadata["image_architecture"] = command(["docker", "image", "inspect", self.args.image, "--format", "{{.Architecture}}"], capture_output=True).stdout.strip()
        nodes = json.loads(self.kube("get", "nodes", "-o", "json", capture_output=True).stdout)["items"]
        metadata["kubernetes_nodes"] = [node["status"]["nodeInfo"] for node in nodes]
        metadata["scripts_sha256"] = {name: hashlib.sha256((HERE / name).read_bytes()).hexdigest() for name in ["mock.mjs", "workload.mjs"]}
        image_env = json.loads(command(["docker", "image", "inspect", self.args.image, "--format", "{{json .Config.Env}}"], capture_output=True).stdout)
        metadata["image_allocator_environment"] = dict(item.split("=", 1) for item in image_env if item.split("=", 1)[0] in {"MALLOC_MMAP_THRESHOLD_", "MALLOC_ARENA_MAX"})
        (self.output / "environment.json").write_text(json.dumps(metadata, indent=2))
        seeded = False
        for limit in self.args.limits:
            for repetition in range(1, self.args.repetitions + 1):
                name = f"limit-{limit:02}-rep-{repetition}"
                if (self.output / f"{name}.json").exists():
                    raise RuntimeError(f"refusing to overwrite existing result {name}")
                print(f"{self.args.label}: {name}: starting fresh pods", flush=True)
                pods = self.gateways(limit)
                if not seeded:
                    # Seed only once; the driver stores the generated key in its
                    # ephemeral filesystem and never includes it in artifacts.
                    self.kube("exec", "deployment/driver", "--", "node", "/bench/workload.mjs", "seed", f"http://{pods[0]['ip']}:8081", stdout=subprocess.DEVNULL)
                    seeded = True
                samplers = []
                for pod in pods:
                    stream = (self.output / f"{name}-{pod['name']}.memory.txt").open("w")
                    process = subprocess.Popen(self.kubectl + ["exec", pod["name"], "--", "sh", "-c", SAMPLER], stdout=stream, stderr=subprocess.DEVNULL)
                    samplers.append((process, stream))
                config = {"label": self.args.label, "limit": limit, "repetition": repetition,
                          "pods": pods, "duration_seconds": self.args.duration, "rps": self.args.rps,
                          "upstream_delay_ms": self.args.delay_ms}
                try:
                    with (self.output / f"{name}.json").open("w") as output:
                        self.kube("exec", "deployment/driver", "--", "node", "/bench/workload.mjs", json.dumps(config), stdout=output)
                finally:
                    for process, stream in samplers:
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
                        stream.close()
                self.kube("get", "pods", "-l", "app=gateway", "-o", "json", stdout=(self.output / f"{name}.pods.json").open("w"))
                result = json.loads((self.output / f"{name}.json").read_text())
                latency = result["success_latency_ms"]
                timing = f"{latency['p95']:.1f} ms" if latency else "n/a"
                print(f"{name}: statuses={result['status_counts']}, success p95={timing}", flush=True)
                if result["completed_requests"] != result["scheduled_requests"] or result["verified_requests"] != result["successful_requests"]:
                    raise RuntimeError(f"incomplete or corrupted run: {name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kubeconfig", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--limits", nargs="+", type=int, default=[8, 16, 24, 32, 64])
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--rps", type=float, default=12)
    parser.add_argument("--duration", type=float, default=15)
    parser.add_argument("--delay-ms", type=int, default=12000)
    parser.add_argument("--host-metadata", action="store_true")
    parser.add_argument("--malloc-mmap-threshold", type=int, help="diagnostic glibc allocator override; omitted for the comparison matrix")
    parser.add_argument("--memory-limit-mib", type=int, default=2048)
    args = parser.parse_args()
    if args.rps <= 0 or args.duration <= 0 or args.delay_ms < 0 or args.repetitions < 1 or args.memory_limit_mib <= 0 or any(limit < 1 for limit in args.limits) or (args.malloc_mmap_threshold is not None and args.malloc_mmap_threshold <= 0):
        parser.error("rate, duration, repetitions, and limits must be positive; delay must be nonnegative")
    Benchmark(args).run()


if __name__ == "__main__":
    main()

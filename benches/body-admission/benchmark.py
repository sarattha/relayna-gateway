#!/usr/bin/env python3
"""Run native request-body benchmarks and retain reports after failures."""

import argparse
import contextlib
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import uuid

import capacity_sweep
import capacity_report

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
NODE_IMAGE = "kindest/node:v1.36.1"
DEPENDENCIES = ["node:22.23.4-alpine", "postgres:16-alpine", "redis:7-alpine"]


def architecture(value):
    aliases = {"x86_64": "amd64", "amd64": "amd64", "aarch64": "arm64", "arm64": "arm64"}
    if value.lower() not in aliases:
        raise ValueError(f"unsupported architecture: {value}; use native amd64 or arm64")
    return aliases[value.lower()]


def now():
    return datetime.now(timezone.utc).isoformat()


def environment():
    env = os.environ.copy()
    env.pop("DOCKER_DEFAULT_PLATFORM", None)
    return env


def inspect_image(image):
    return json.loads(subprocess.check_output(["docker", "image", "inspect", image],
                                             text=True, stderr=subprocess.DEVNULL, env=environment(), timeout=30))[0]


def cpu_model():
    try:
        if platform.system() == "Darwin":
            return subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True,
                                           stderr=subprocess.DEVNULL, timeout=5).strip()
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            key, separator, value = line.partition(":")
            if separator and key.strip() in ("model name", "Hardware", "Model"):
                return value.strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return platform.processor() or "not recorded"


def doctor():
    tools = {name: shutil.which(name) for name in ("docker", "kind", "kubectl")}
    result = {"schema_version": 1, "ready": False, "python": platform.python_version(),
              "host": {"system": platform.system(), "architecture": platform.machine(), "cpu_model": cpu_model()},
              "tools": tools, "docker": None, "errors": []}
    for name, path in tools.items():
        if not path:
            result["errors"].append(f"{name} is missing from PATH")
    try:
        host_arch = architecture(platform.machine())
        if tools["docker"]:
            info = json.loads(subprocess.check_output(["docker", "info", "--format", "{{json .}}"],
                                                      text=True, stderr=subprocess.DEVNULL, timeout=20, env=environment()))
            result["docker"] = {key: info.get(key) for key in
                                ("Architecture", "NCPU", "MemTotal", "KernelVersion", "ServerVersion", "OperatingSystem", "CgroupVersion")}
            result["architecture"] = architecture(info["Architecture"])
            if result["architecture"] != host_arch:
                result["errors"].append("host and Docker architectures differ; native benchmarks require them to match")
            if info.get("OSType") != "linux":
                result["errors"].append("Docker must run Linux containers")
    except (ValueError, KeyError, subprocess.SubprocessError) as error:
        result["errors"].append(f"Docker/native preflight failed: {type(error).__name__}")
    result["ready"] = not result["errors"]
    return result


class Interrupted(KeyboardInterrupt):
    def __init__(self, signum):
        self.signum = signum
        super().__init__(f"received signal {signum}")


def run(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    run_id = uuid.uuid4().hex[:12]
    cluster = f"relayna-body-bench-{run_id}"
    meta = {"schema_version": 1, "run_id": run_id, "status": "running", "stage": "preflight",
            "started_at": now(), "body_mib": args.body_mib, "mode": "sweep" if args.slots else "capacity",
            "repetitions": args.repetitions, "cluster": cluster, "native_verified": False,
            "resources": {"pods": 2, "cpu_per_pod": 2, "memory_mib_per_pod": 1024, "body_budget_mib_per_pod": 512},
            "completed_sizes": [], "errors": []}
    capacity_report.write_json(output / "run.json", meta)
    capacity_report.generate(output)
    attempted_cluster = False
    exit_code = 0
    handlers = {}
    log = (output / "provision.log").open("w")

    def persist(stage):
        meta["stage"] = stage
        capacity_report.write_json(output / "run.json", meta)
        print(f"body benchmark: {stage}", file=sys.stderr, flush=True)

    def call(command, *, timeout=600, capture=False):
        print(json.dumps(command), file=log, flush=True)
        return subprocess.run(command, check=True, text=True, env=environment(), timeout=timeout,
                              **({"capture_output": True} if capture else {"stdout": log, "stderr": subprocess.STDOUT}))

    def interrupt(signum, _frame):
        raise Interrupted(signum)

    try:
        for signum in (signal.SIGINT, signal.SIGTERM):
            handlers[signum] = signal.signal(signum, interrupt)
        preflight = doctor()
        meta["preflight"] = preflight
        if not preflight["ready"]:
            raise RuntimeError("; ".join(preflight["errors"]))
        arch = preflight["architecture"]
        meta["architecture"] = arch
        meta["harness_commit"] = subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip()
        meta["harness_dirty"] = bool(subprocess.check_output(["git", "-C", str(ROOT), "status", "--porcelain"], text=True).strip())
        meta["harness_sha256"] = capacity_report.source_hashes(HERE)
        persist("prepare native image")
        image = args.image
        if args.build:
            image = f"relayna-body-benchmark:{run_id}"
            call(["docker", "build", "--platform", f"linux/{arch}", "--label",
                  f"org.opencontainers.image.revision={meta['harness_commit']}", "-t", image, str(ROOT)], timeout=3600)
        else:
            try:
                inspect_image(image)
            except subprocess.CalledProcessError:
                call(["docker", "pull", "--platform", f"linux/{arch}", image])
        inspected = inspect_image(image)
        if inspected["Os"] != "linux" or architecture(inspected["Architecture"]) != arch:
            raise RuntimeError(f"image architecture {inspected['Architecture']} does not match native {arch}; pull/build for linux/{arch}")
        labels = inspected["Config"].get("Labels") or {}
        reference = image
        if not args.build:
            image = f"relayna-body-benchmark:{run_id}"
            call(["docker", "tag", inspected["Id"], image])
        meta["image"] = {"reference": reference, "loaded_reference": image, "id": inspected["Id"], "architecture": arch,
                         "source_commit": labels.get("org.opencontainers.image.revision", "unknown"),
                         "version": labels.get("org.opencontainers.image.version", "unknown")}
        for dependency in [NODE_IMAGE, *DEPENDENCIES]:
            call(["docker", "pull", "--platform", f"linux/{arch}", dependency])
        with tempfile.TemporaryDirectory(prefix="relayna-body-bench-") as temporary:
            kubeconfig = Path(temporary) / "kubeconfig"
            persist("create isolated cluster")
            attempted_cluster = True
            call(["kind", "create", "cluster", "--name", cluster, "--image", NODE_IMAGE,
                  "--kubeconfig", str(kubeconfig), "--wait", "180s"])
            kubeconfig.chmod(0o600)
            call(["kind", "load", "docker-image", image, *DEPENDENCIES, "--name", cluster])
            nodes = json.loads(call(["kubectl", "--kubeconfig", str(kubeconfig), "get", "nodes", "-o", "json"], capture=True).stdout)["items"]
            meta["nodes"] = [node["status"]["nodeInfo"] for node in nodes]
            if not nodes or any(architecture(node["architecture"]) != arch for node in meta["nodes"]):
                raise RuntimeError("kind node architecture does not match the native image")
            call(["docker", "exec", f"{cluster}-control-plane", "sh", "-c",
                  "test -r /sys/fs/cgroup/memory.peak && test -r /sys/fs/cgroup/cpu.stat"])
            meta["native_verified"] = True
            for size in args.body_mib:
                persist(f"measure {size} MiB")
                command = ["--kubeconfig", str(kubeconfig), "--image", image,
                           "--source-commit", meta["image"]["source_commit"], "--body-mib", str(size),
                           "--output", str(output / f"{size}mib"), "--confirmation-repetitions", str(args.repetitions),
                           "--max-search-slots", str(args.max_search_slots)]
                if args.slots:
                    command += ["--fixed-limits", *map(str, args.slots)]
                with contextlib.redirect_stdout(sys.stderr):
                    capacity_sweep.main(command)
                summary = json.loads((output / f"{size}mib" / "summary.json").read_text())
                if (summary.get("refinement_stop") or summary.get("confirmation_stop") or
                        any(not row["resource_stable"] and not row["resource_failure"] for row in summary["trials"])):
                    raise RuntimeError(f"{size} MiB search stopped on a non-resource failure; see report")
                meta["completed_sizes"].append(size)
                capacity_report.write_json(output / "run.json", meta)
                capacity_report.generate(output)
        meta["status"] = "completed"
    except BaseException as error:
        meta["status"] = "interrupted" if isinstance(error, KeyboardInterrupt) else "failed"
        meta["errors"].append({"stage": meta["stage"], "message": f"{type(error).__name__}: {error}"})
        exit_code = 128 + getattr(error, "signum", signal.SIGINT) if isinstance(error, KeyboardInterrupt) else 1
    finally:
        try:
            if attempted_cluster:
                persist("cleanup owned cluster")
                with tempfile.TemporaryDirectory(prefix="relayna-body-cleanup-") as cleanup:
                    call(["kind", "delete", "cluster", "--name", cluster,
                          "--kubeconfig", str(Path(cleanup) / "kubeconfig")], timeout=180)
        except BaseException as error:
            meta["errors"].append({"stage": "cleanup", "message": f"{type(error).__name__}: {error}"})
            if not exit_code:
                exit_code = 1
                meta["status"] = "failed"
        finally:
            for signum, handler in handlers.items():
                signal.signal(signum, handler)
            log.close()
            meta["stage"] = "finished"
            meta["finished_at"] = now()
            capacity_report.write_json(output / "run.json", meta)
            capacity_report.generate(output)
    return exit_code, {"schema_version": 1, "status": meta["status"], "output": str(output),
                       "report": str(output / "report.html"), "architecture": meta.get("architecture"), "errors": meta["errors"]}


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--json", action="store_true", help="emit a stable JSON result on stdout; progress goes to stderr")
    commands = result.add_subparsers(dest="command", required=True)
    commands.add_parser("doctor", help="check local Python, Docker, kind, kubectl and native architecture")
    execute = commands.add_parser("run", help="create an isolated native kind cluster, benchmark, report and clean up")
    image = execute.add_mutually_exclusive_group(required=True)
    image.add_argument("--image", help="local or published native Gateway image; missing images are pulled")
    image.add_argument("--build", action="store_true", help="build the current source with its production Dockerfile before measurement")
    execute.add_argument("--output", type=Path, required=True, help="new artifact directory; existing directories are never overwritten")
    execute.add_argument("--body-mib", nargs="+", type=int, choices=capacity_sweep.GRIDS, default=list(capacity_sweep.GRIDS))
    execute.add_argument("--slots", nargs="+", type=int, help="fixed slot sweep; omit for adaptive boundary discovery")
    execute.add_argument("--repetitions", type=int, default=3)
    execute.add_argument("--max-search-slots", type=int, default=4096)
    report = commands.add_parser("report", help="regenerate HTML/JSON/CSV from complete or partial local checkpoints; no Docker needed")
    report.add_argument("directory", type=Path)
    return result


def main(argv=None):
    cli = parser()
    args = cli.parse_args(argv)
    if args.command == "run" and (args.repetitions < 1 or args.max_search_slots < 1 or
                                  (args.slots and (min(args.slots) < 1 or max(args.slots) > args.max_search_slots))):
        cli.error("repetitions/slots must be positive and slots cannot exceed the search bound")
    if args.command == "run":
        args.body_mib = list(dict.fromkeys(args.body_mib))
        if args.slots:
            args.slots = sorted(set(args.slots))
        elif args.max_search_slots < max(max(capacity_sweep.GRIDS[size]) for size in args.body_mib):
            cli.error("search bound must cover the initial grid; use --slots for a smaller fixed sweep")
    try:
        if args.command == "doctor":
            data = doctor()
            code = 0 if data["ready"] else 1
        elif args.command == "report":
            data = capacity_report.generate(args.directory.resolve())
            code = 0
        else:
            code, data = run(args)
    except Exception as error:
        code, data = 1, {"schema_version": 1, "status": "failed", "error": f"{type(error).__name__}: {error}"}
    if args.json:
        print(json.dumps(data))
    elif args.command == "doctor":
        print(json.dumps(data, indent=2))
    else:
        print(f"{data.get('status', 'reported')}: {data.get('report', data.get('error'))}")
    return code


if __name__ == "__main__":
    sys.exit(main())

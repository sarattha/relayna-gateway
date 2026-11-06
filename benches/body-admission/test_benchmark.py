"""Behavioral tests for native orchestration and retained partial evidence."""
import contextlib
import io
import json
from pathlib import Path
import signal
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import benchmark
import capacity_report as report
import capacity_sweep as sweep
import run as runner


def trial(slots=8, cohort=32, stable=True, oom=False, **extra):
    return dict(slots=slots, attempt=1, cohort_per_pod=cohort, resource_stable=stable,
                clean=stable, resource_failure=oom, oom_pods=int(oom), incomplete=False,
                memory_streams=2, memory_censored=oom, rss_peak_mib=700, cgroup_peak_mib=900,
                success_latency_ms={"p50": 12000, "p95": 15000, "p99": 16000},
                statuses={"200": 16, "503": 48}, successes=16, requests=64, **extra)


class Reports(unittest.TestCase):
    def test_repeated_setting_takes_precedence_over_single_discovery_success(self):
        rows = [trial(31) for _ in range(3)] + [trial(32, stable=False, oom=True), trial(256)]
        result = report.describe(dict(body_mib=5, complete=True, trials=rows))
        self.assertEqual((result["slots"], result["stable_trials"], result["label"]),
                         (31, 3, "tested lower bound"))

    def test_confirmed_edge_requires_repeated_adjacent_oom(self):
        rows = [trial() for _ in range(3)] + [trial(9, stable=False, oom=True) for _ in range(3)]
        result = report.describe(dict(body_mib=5, complete=True, trials=rows))
        self.assertEqual(result["label"], "confirmed resource edge")
        self.assertEqual(result["slots"], 8)
        self.assertEqual(result["headroom_mib"], 124)
        self.assertEqual(report.describe(dict(complete=True, trials=rows[:4]))["label"], "tested lower bound")

    def test_never_combine_cohorts_or_ignore_lower_slot_failures(self):
        rows = [trial(cohort=16) for _ in range(3)] + [trial(9, stable=False, oom=True) for _ in range(3)]
        self.assertIsNone(report.describe(dict(complete=True, trials=rows))["slots"])
        rows += [trial() for _ in range(3)] + [trial(4, stable=False, oom=True)]
        self.assertEqual(report.describe(dict(complete=True, trials=rows))["label"], "tested lower bound")

    def test_fixed_smoke_is_not_boundary_and_missing_memory_is_not_zero(self):
        row = trial()
        row.update(memory_streams=0, rss_peak_mib=0, cgroup_peak_mib=0)
        result = report.describe(dict(complete=True, search_mode="fixed", trials=[row]))
        self.assertEqual(result["label"], "tested setting")
        self.assertIsNone(result["rss_peak_mib"])
        self.assertEqual(report.describe(dict(complete=False, trials=[trial()]))["label"], "incomplete")

    def test_failure_partial_corruption_html_escape_and_csv(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            report.write_json(root / "run.json", {"status": "failed", "body_mib": [5, 100],
                              "errors": [{"stage": "setup", "message": "</script><img src=x onerror=alert(1)>"}]})
            folder = root / "5mib"
            folder.mkdir()
            report.write_json(folder / "summary.json", dict(body_mib=5, complete=False, trials=[trial(analysis_error="=formula")]))
            result = report.generate(root)
            page = (root / "report.html").read_text()
            self.assertEqual(result["status"], "failed")
            self.assertNotIn("</script><img", page)
            self.assertIn(r"\u003c/script>", page)
            self.assertIn("'=formula", (root / "trials.csv").read_text())
            data = json.loads((root / "report.json").read_text())
            self.assertEqual(data["sizes"][1]["label"], "not run")
            (folder / "summary.json").write_text('{"trials":')
            self.assertTrue(report.generate(root)["warnings"])
            self.assertTrue((root / "report.html").exists())
            report.write_json(folder / "summary.json", {"body_mib": 5, "trials": ["corrupted-row"]})
            self.assertTrue(report.generate(root)["warnings"])

    def test_running_checkpoint_and_offline_empty_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(ValueError):
                report.generate(root)
            report.write_json(root / "run.json", {"status": "running"})
            self.assertEqual(report.generate(root)["status"], "in progress / unfinished")

    def test_observation_window_survives_json_and_csv_without_inventing_legacy_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            timing = dict(client_deadline_ms=180000, condition_timeout_seconds=300, trial_timeout_seconds=600)
            report.write_json(root / "summary.json", dict(body_mib=10, complete=True, trials=[trial(**timing)], **timing))
            report.generate(root)
            data = json.loads((root / "report.json").read_text())["sizes"][0]
            for key, value in timing.items():
                self.assertEqual(data[key], value)
                self.assertEqual(data["trials"][0][key], value)
            self.assertIn("client_deadline_ms", (root / "trials.csv").read_text().splitlines()[0])
            self.assertIsNone(report.describe(dict(complete=True, trials=[trial()]))["client_deadline_ms"])
            self.assertIsNone(report.normalize(trial())["client_deadline_ms"])


class AdmissionAccounting(unittest.TestCase):
    def test_mixed_payloads_schedules_and_goodput(self):
        subprocess.run(["node", "--test", str(Path(__file__).parent / "mixed_workload.test.mjs")],
                       check=True, capture_output=True, text=True)

    def test_proxy_metrics_exclude_control_plane_probe_denials(self):
        source = (Path(__file__).parent / "workload.mjs").read_text()
        parser = source.split("function metrics(text) {", 1)[1].split("async function run(config)", 1)[0]
        lines = [
            'gateway_denials_total{kind="body_admission",route="all",reason="concurrency"} 5',
            'gateway_denials_total{kind="body_admission",route="all",reason="working_set"} 3',
            'gateway_denials_total{kind="body_admission",route="all",reason="processing_workspace"} 2',
            'gateway_denials_total{kind="upstream_admission",route="proxy",reason="concurrency"} 4',
            'gateway_denials_total{kind="auth_admission",route="proxy",reason="authentication_overloaded"} 7',
            'gateway_denials_total{kind="auth_admission",route="control",reason="authentication_overloaded"} 1',
            'gateway_denials_total{kind="auth_admission",route="proxy",reason="source_rate_limit_exceeded"} 11',
            'gateway_denials_total{kind="auth_admission",route="control",reason="source_rate_limit_exceeded"} 2',
        ]
        script = "function metrics(text) {" + parser + "\nprocess.stdout.write(JSON.stringify(metrics(" + json.dumps("\n".join(lines)) + ")));"
        result = subprocess.run(["node", "--input-type=module", "-e", script],
                                check=True, capture_output=True, text=True)
        values = json.loads(result.stdout)
        self.assertEqual(values["concurrency_denials"], 5)
        self.assertEqual(values["working_set_denials"], 5)
        self.assertEqual(values["processing_denials"], 2)
        self.assertEqual(values["upstream_denials"], 4)
        self.assertEqual(values["authentication_denials"], 7)
        self.assertEqual(values["source_denials"], 11)
        self.assertEqual(values["control_authentication_denials"], 1)
        self.assertEqual(values["control_source_denials"], 2)

    def test_body_authentication_and_source_denials_reconcile_separately(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "limit-08-rep-1.json"
            pods = [{"metadata": {"name": name}, "status": {"containerStatuses": [{"restartCount": 0}]},
                     "spec": {"containers": [{"resources": {"limits": {"cpu": "2", "memory": "1Gi"}},
                     "env": [{"name": "GATEWAY_MAX_BUFFERED_REQUESTS", "value": "8"},
                             {"name": "GATEWAY_MAX_INFLIGHT_BUFFER_BYTES", "value": str(512 * sweep.MIB)}]}]}}
                    for name in ["pod-a", "pod-b"]]
            path.with_suffix(".pods.json").write_text(json.dumps({"items": pods}))
            results, samples = [], []
            for pod in pods:
                name = pod["metadata"]["name"]
                (root / f"limit-08-rep-1-{name}.memory.txt").write_text("0 0 100 200 0\n")
                for status, code in [(200, None), (503, "gateway_overloaded"), (503, "authentication_overloaded"), (429, "source_rate_limit_exceeded")]:
                    results.append(dict(pod=name, status=status, error_code=code, verified=status == 200))
                for count in [0, 1]:
                    samples.append(dict(pod=name, buffered_requests=0, buffered_bytes=0,
                                        concurrency_denials=count, byte_denials=0,
                                        authentication_denials=count, source_denials=count))
            run = dict(body_bytes=sweep.MIB, scheduled_requests=8, completed_requests=8,
                       verified_requests=2, successful_requests=2, http_max_sockets_per_host=4,
                       pods=[{"name": pod["metadata"]["name"]} for pod in pods], results=results,
                       samples=samples, status_counts={"200": 2, "503": 4, "429": 2},
                       body_sha256="fixture", elapsed_ms=1, success_latency_ms={}, rejection_latency_ms={}, dispatch_lag_ms={})
            path.write_text(json.dumps(run))
            row = sweep.analyze(root, 8, 1, sweep.MIB, 4)
            self.assertTrue(row["clean"])
            self.assertEqual(row["preauth_denials"], 4)
            self.assertEqual(row["denial_codes"]["gateway_overloaded"], 2)
            self.assertEqual(row["admission_counter_gap"], 0)
            run["results"][3]["error_code"] = "rate_limit_exceeded"
            path.write_text(json.dumps(run))
            with self.assertRaises(AssertionError):
                sweep.analyze(root, 8, 1, sweep.MIB, 4)


class Orchestration(unittest.TestCase):
    def test_setup_waits_for_postgres_tcp_after_temporary_socket_server(self):
        args = SimpleNamespace(kubeconfig="fixture", output=str(self.output), delay_ms=2000,
                               read_delay_ms=10000, body_mib=5, body_mix=None, client_deadline_ms=180000)
        instance = runner.Benchmark(args)
        probes = []
        def probe(command, **kwargs):
            probes.append(command)
            tcp = "-h" in command and command[command.index("-h") + 1] == "127.0.0.1"
            return subprocess.CompletedProcess(command, int(tcp and len(probes) == 1))
        with patch.object(instance, "apply"), patch.object(instance, "kube", return_value=subprocess.CompletedProcess([], 0, stdout="secret/credentials")), patch.object(runner.subprocess, "run", side_effect=probe), patch.object(runner.time, "sleep") as sleep:
            instance.setup()
        self.assertEqual(len(probes), 2)
        sleep.assert_called_once_with(1)

    def test_shellless_production_image_gets_benchmark_tools_only_by_mount(self):
        spec = runner.deployment("gateway", "production@sha256:fixture")["spec"]["template"]["spec"]
        gateway = spec["containers"][0]
        self.assertEqual(gateway["image"], "production@sha256:fixture")
        self.assertNotIn("command", gateway)
        self.assertTrue(gateway["volumeMounts"][0]["readOnly"])
        tools = spec["initContainers"][0]
        self.assertEqual(tools["image"], runner.SAMPLER_IMAGE)
        self.assertEqual(tools["command"][-1], runner.SAMPLER_TOOL)
        self.assertNotIn("initContainers", runner.deployment("postgres", "postgres:16-alpine")["spec"]["template"]["spec"])

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.output = Path(self.temporary.name) / "run"
        self.args = benchmark.parser().parse_args(["run", "--image", "fixture:local", "--output", str(self.output),
                                                   "--body-mib", "5", "100", "--slots", "1", "--repetitions", "1"])
        self.native = dict(ready=True, architecture="arm64", errors=[])
        self.image = {"Os": "linux", "Architecture": "arm64", "Id": "sha256:fixture", "Config": {"Labels": {}}}

    def test_architecture_aliases_and_docker_platform_override(self):
        for alias in ("arm64", "aarch64", "ARM64"):
            self.assertEqual(benchmark.architecture(alias), "arm64")
        self.assertEqual(benchmark.architecture("x86_64"), "amd64")
        with self.assertRaises(ValueError):
            benchmark.architecture("riscv64")
        with patch.dict(benchmark.os.environ, {"DOCKER_DEFAULT_PLATFORM": "linux/amd64"}):
            self.assertNotIn("DOCKER_DEFAULT_PLATFORM", benchmark.environment())

    def test_doctor_rejects_engine_emulation(self):
        info = json.dumps(dict(Architecture="x86_64", OSType="linux"))
        with patch.object(benchmark.shutil, "which", return_value="tool"), patch.object(benchmark.platform, "machine", return_value="arm64"), patch.object(benchmark, "cpu_model", return_value="fixture CPU"), patch.object(benchmark.subprocess, "check_output", return_value=info):
            self.assertFalse(benchmark.doctor()["ready"])

    def test_preflight_failure_leaves_report_and_does_not_touch_clusters(self):
        with patch.object(benchmark, "doctor", return_value=dict(ready=False, errors=["docker unavailable"])), patch.object(benchmark.subprocess, "run") as process:
            code, result = benchmark.run(self.args)
        self.assertEqual(code, 1)
        self.assertEqual(result["status"], "failed")
        process.assert_not_called()
        self.assertIn("docker unavailable", (self.output / "report.html").read_text())
        with self.assertRaises(FileExistsError):
            benchmark.run(self.args)

    def lifecycle(self, fail=None, image_arch="arm64", node_arch="arm64"):
        commands = []
        def process(cmd, **kwargs):
            commands.append(cmd)
            if cmd[:3] == ["kind", "create", "cluster"]:
                if fail == "create":
                    raise subprocess.CalledProcessError(1, cmd)
                Path(cmd[cmd.index("--kubeconfig") + 1]).touch()
            return subprocess.CompletedProcess(cmd, 0, json.dumps({"items": [{"status": {"nodeInfo": {"architecture": node_arch}}}]}))
        def measure(argv):
            if fail == "interrupt":
                raise benchmark.Interrupted(signal.SIGTERM)
            size = int(argv[argv.index("--body-mib") + 1])
            directory = Path(argv[argv.index("--output") + 1])
            directory.mkdir()
            row = trial(slots=1)
            if fail == "non-resource":
                row.update(resource_stable=False, clean=False)
            report.write_json(directory / "summary.json", dict(body_mib=size, complete=True, search_mode="fixed", trials=[row]))
        image = {**self.image, "Architecture": image_arch}
        with patch.object(benchmark, "doctor", return_value=self.native), patch.object(benchmark, "inspect_image", return_value=image), patch.object(benchmark.subprocess, "check_output", side_effect=["commit", ""]), patch.object(benchmark.subprocess, "run", side_effect=process), patch.object(sweep, "main", side_effect=measure):
            code, result = benchmark.run(self.args)
        return code, result, commands

    def test_native_lifecycle_private_kubeconfig_and_cleanup(self):
        code, result, commands = self.lifecycle()
        self.assertEqual(code, 0)
        self.assertEqual(result["status"], "completed")
        meta = json.loads((self.output / "run.json").read_text())
        self.assertTrue(meta["native_verified"])
        self.assertEqual(meta["completed_sizes"], [5, 100])
        self.assertEqual(meta["client_deadline_ms"], 180000)
        self.assertEqual(meta["condition_timeout_seconds"], 300)
        self.assertEqual(meta["trial_timeout_seconds"], 600)
        create = next(cmd for cmd in commands if cmd[:3] == ["kind", "create", "cluster"])
        kubeconfig = Path(create[create.index("--kubeconfig") + 1])
        self.assertFalse(kubeconfig.exists())
        self.assertFalse(list(self.output.rglob("*kubeconfig*")))
        self.assertEqual(commands[-1][:5], ["kind", "delete", "cluster", "--name", meta["cluster"]])
        self.assertIn("--kubeconfig", commands[-1])
        self.assertNotIn("--context", " ".join(" ".join(cmd) for cmd in commands))

    def test_image_mismatch_stops_before_cluster(self):
        code, result, commands = self.lifecycle(image_arch="amd64")
        self.assertEqual(code, 1)
        self.assertFalse(any(cmd[0] == "kind" for cmd in commands))
        self.assertIn("does not match native", result["errors"][0]["message"])

    def test_node_mismatch_retains_report_and_cleans_owned_cluster(self):
        code, result, commands = self.lifecycle(node_arch="amd64")
        self.assertEqual(code, 1)
        self.assertEqual(commands[-1][:3], ["kind", "delete", "cluster"])
        self.assertEqual(json.loads((self.output / "report.json").read_text())["status"], "failed")

    def test_cluster_creation_failure_also_cleans_up(self):
        self.assertEqual(self.lifecycle(fail="create")[0], 1)
        self.assertTrue((self.output / "report.html").exists())

    def test_sigterm_retains_interrupted_report_and_cleans_up(self):
        code, result, commands = self.lifecycle(fail="interrupt")
        self.assertEqual(code, 143)
        self.assertEqual(result["status"], "interrupted")
        self.assertEqual(commands[-1][:3], ["kind", "delete", "cluster"])

    def test_non_resource_failure_is_not_successful_run(self):
        self.assertEqual(self.lifecycle(fail="non-resource")[0], 1)

    def test_json_stdout_is_machine_readable_on_failure(self):
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout), patch.object(benchmark, "doctor", return_value=dict(ready=False, errors=["missing tools"])):
            code = benchmark.main(["--json", "run", "--image", "fixture", "--output", str(self.output)])
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(stdout.getvalue())["status"], "failed")


class Checkpointing(unittest.TestCase):
    def execute(self, root, process):
        args = ["--kubeconfig", "private", "--image", "fixture", "--source-commit", "fixture", "--body-mib", "5",
                "--output", str(root), "--fixed-limits", "1", "4", "--confirmation-repetitions", "2"]
        with patch.object(sweep.subprocess, "run", side_effect=process), patch.object(sweep, "analyze", side_effect=lambda folder, slots, attempt, body, cohort: trial(slots, cohort)):
            sweep.main(args)

    def test_fixed_sweep_exact_repeats(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.execute(root, lambda cmd, **kw: subprocess.CompletedProcess(cmd, 0))
            data = json.loads((root / "summary.json").read_text())
            self.assertEqual([row["slots"] for row in data["trials"]], [1, 1, 4, 4])
            self.assertTrue(data["complete"])
            self.assertEqual(data["search_mode"], "fixed")

    def test_observation_window_is_applied_after_checkpointing_and_before_outer_timeout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def process(cmd, **kwargs):
                data = json.loads((root / "summary.json").read_text())
                self.assertEqual(data["client_deadline_ms"], 180000)
                self.assertEqual(data["condition_timeout_seconds"], 300)
                self.assertEqual(data["trial_timeout_seconds"], 600)
                self.assertEqual(data["trials"][-1]["client_deadline_ms"], 180000)
                self.assertEqual(cmd[cmd.index("--client-deadline-ms") + 1], "180000")
                self.assertEqual(cmd[cmd.index("--condition-timeout") + 1], "300")
                self.assertEqual(kwargs["timeout"], 600)
                return subprocess.CompletedProcess(cmd, 0)
            self.execute(root, process)

    def test_timeout_checkpoint_precedes_child_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def fail(cmd, **kw):
                data = json.loads((root / "summary.json").read_text())
                self.assertEqual(data["trials"][0]["status"], "running")
                raise subprocess.TimeoutExpired(cmd, 300)
            with self.assertRaises(subprocess.TimeoutExpired):
                self.execute(root, fail)
            data = json.loads((root / "summary.json").read_text())
            self.assertFalse(data["complete"])
            self.assertEqual(data["trials"][0]["status"], "invalid")
            self.assertIn("TimeoutExpired", data["error"])
            self.assertEqual(report.generate(root)["status"], "incomplete")


class AdaptiveSearch(unittest.TestCase):
    def search(self, boundary, *, delayed=False, lost=False, incomplete=False):
        def analyze(folder, slots, attempt, body, cohort):
            failed = slots > boundary or (delayed and slots == boundary and attempt == 3)
            row = trial(slots, cohort, stable=not failed and not (incomplete and slots == boundary), oom=failed)
            row.update(attempt=attempt, clean=row["resource_stable"] and not lost)
            return row
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(sweep, "analyze", side_effect=analyze), patch.object(sweep.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)), contextlib.redirect_stdout(io.StringIO()):
                sweep.main(["--kubeconfig", "fixture", "--image", "fixture", "--source-commit", "fixture",
                            "--body-mib", "100", "--output", temporary, "--max-search-slots", "128"])
            return json.loads((Path(temporary) / "summary.json").read_text())

    def test_accounted_lost_denials_do_not_stop_expansion(self):
        summary = self.search(37, lost=True)
        self.assertTrue(any(row["slots"] == 64 for row in summary["trials"]))
        result = report.describe(summary)
        self.assertEqual((result["slots"], result["label"], result["http_clean_trials"]), (37, "confirmed resource edge", 0))

    def test_failure_only_during_confirmation_refines_again(self):
        result = report.describe(self.search(32, delayed=True))
        self.assertEqual((result["slots"], result["stable_trials"], result["label"]), (31, 3, "confirmed resource edge"))

    def test_finite_ceiling_is_only_lower_bound(self):
        result = report.describe(self.search(128))
        self.assertEqual((result["slots"], result["label"]), (128, "tested lower bound"))

    def test_missing_admitted_response_prevents_confirmation(self):
        summary = self.search(37, incomplete=True)
        self.assertIn("non-resource failure", summary["confirmation_stop"])
        self.assertEqual(report.describe(summary)["label"], "incomplete")


if __name__ == "__main__":
    unittest.main()

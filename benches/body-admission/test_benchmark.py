"""Behavioral tests for native orchestration and retained partial evidence."""
import contextlib
import io
import json
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import benchmark
import capacity_report as report
import capacity_sweep as sweep


def trial(slots=8, cohort=32, stable=True, oom=False, **extra):
    return dict(slots=slots, attempt=1, cohort_per_pod=cohort, resource_stable=stable,
                clean=stable, resource_failure=oom, oom_pods=int(oom), incomplete=False,
                memory_streams=2, memory_censored=oom, rss_peak_mib=700, cgroup_peak_mib=900,
                success_latency_ms={"p50": 12000, "p95": 15000, "p99": 16000},
                statuses={"200": 16, "503": 48}, successes=16, requests=64, **extra)


class Reports(unittest.TestCase):
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


class Orchestration(unittest.TestCase):
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

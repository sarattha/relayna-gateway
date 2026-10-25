"""CI duration, private evidence and fixture ownership checks; no live load."""
import os
from pathlib import Path
import secrets
import signal
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

import run_smoke


class SmokeRunnerTests(unittest.TestCase):
    def report(self, **changes):
        return {"passed": True, "failures": {}, "requested_seconds": 600,
                "load_elapsed_seconds": 600.001,
                "counts": [{"socket_renewals": 1}, {"socket_renewals": 1}], **changes}

    def test_gate_defaults_to_ten_minutes(self):
        args = run_smoke.parser().parse_args(["--binary", "gateway-api", "--output", "output/check"])
        self.assertEqual(args.seconds, 600)

    def test_shortened_or_failed_load_cannot_pass_the_gate(self):
        run_smoke.validate_report(self.report(), 600)
        for changes in ({"load_elapsed_seconds": 599.999}, {"requested_seconds": 300},
                        {"passed": False}, {"failures": {"unknown_error": 1}},
                        {"counts": []}, {"counts": [{"socket_renewals": 1}, {}]}):
            with self.subTest(changes=changes), self.assertRaises(run_smoke.SmokeError):
                run_smoke.validate_report(self.report(**changes), 600)

    def test_explicit_short_diagnostic_does_not_require_socket_replacement(self):
        run_smoke.validate_report(self.report(requested_seconds=60, load_elapsed_seconds=60,
                                              counts=[{}, {}]), 60)

    def test_ci_runner_cannot_start_a_day_long_soak(self):
        with self.assertRaises(SystemExit) as error, patch("sys.stderr"):
            run_smoke.main(["--binary", "unused", "--output", "output/unused", "--seconds", "86400"])
        self.assertEqual(error.exception.code, 2)

    def test_private_credentials_stay_private_and_are_redacted(self):
        token = "op_live_" + secrets.token_urlsafe(36)
        password = secrets.token_urlsafe(32)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "token"
            run_smoke.private_file(path, token)
            self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                run_smoke.private_file(path, "replacement")
        diagnostic = f"Bearer {token} postgres://user:{password}@localhost/test /run?private=value"
        public = run_smoke.redact(diagnostic, [token, password])
        self.assertNotIn(token, public)
        self.assertNotIn(password, public)
        self.assertNotIn("private=value", public)
        self.assertIn("[REDACTED]", public)

    def test_cleanup_continues_after_one_owned_container_times_out(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = run_smoke.Fixtures(Path("unused"), Path(temporary), Path(temporary))
            fixture.containers = ["owned-postgres", "owned-redis"]
            with patch.object(run_smoke.subprocess, "run", side_effect=[
                subprocess.TimeoutExpired("docker", 30), Mock(returncode=0),
            ]) as stopped:
                fixture.close()
            self.assertEqual([call.args[0][-1] for call in stopped.call_args_list],
                             ["owned-redis", "owned-postgres"])
            self.assertEqual(fixture.cleanup_errors, ["owned_container_cleanup_failed"])

    def test_drained_owned_gateways_use_pingora_fast_shutdown(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = run_smoke.Fixtures(Path("unused"), Path(temporary), Path(temporary))
            process = Mock()
            process.poll.return_value = None
            fixture.gateways = [process]
            fixture.close()
            process.send_signal.assert_called_once_with(signal.SIGINT)
            process.terminate.assert_not_called()
            process.kill.assert_not_called()
            self.assertEqual(fixture.cleanup_errors, [])

    def test_workflow_runs_exactly_ten_minutes_and_retains_evidence(self):
        workflow = (Path(__file__).resolve().parents[3] / ".github/workflows/ci.yml").read_text()
        job = workflow.split("  monitoring-smoke:", 1)[1].split("  repository-metadata:", 1)[0]
        self.assertIn("name: monitoring smoke", job)
        self.assertIn("--seconds 600", job)
        self.assertIn("if: always()", job)
        self.assertIn("github.run_attempt", job, "reruns must retain earlier failed artifacts")
        self.assertIn("runner.temp }}/monitoring-python-cache", job,
                      "bytecode must not falsely mark the checked-out candidate dirty")
        self.assertNotIn("86400", job)


if __name__ == "__main__":
    unittest.main()

"""Isolated admin API fixtures for Cortex channel bootstrap key reuse."""

import copy
import importlib.util
import io
import json
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
from urllib.parse import urlparse


spec = importlib.util.spec_from_file_location(
    "bootstrap_cortex", Path(__file__).with_name("bootstrap-cortex.py")
)
bootstrap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bootstrap)


class BootstrapKeyPolicyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.key_output = Path(self.directory.name) / "channel.key"
        self.key_output.write_text("fixture-private-key")
        self.policy = {
            "allowed_routes": ["/services/*"],
            "allowed_providers": ["internal-service"],
            "allowed_services": ["cortex-web"],
            "allow_streaming": True,
            "rpm_limit": 300,
            "monthly_budget_usd": 10,
        }
        self.key = {
            "id": "fixture-key-id",
            "name": "Cortex BFF Kind E2E",
            "owner_type": "project",
            "project_id": "fixture-project-id",
            "service_names": ["cortex-web"],
            "policy": {**self.policy, "deny": False},
            "disabled": False,
            "revoked_at": None,
            "expires_at": None,
            "key_prefix": "fixture-private-",
        }
        entra = {
            "audience": "api://keystone-mcp",
            "required_scopes": ["keystone.documents.read", "keystone.retrieval.search"],
            "required_roles": [],
            "allowed_groups": [],
            "allow_apigee": False,
        }
        self.service = {
            "name": "cortex-web",
            "project_id": "fixture-project-id",
            "allowed_methods": ["GET", "POST", "PUT", "PATCH", "DELETE"],
            "access": {
                "accessa": {
                    "app": "cortex", "channel": "web", "idle_timeout_ms": 120000,
                    "max_connections": 20, "max_connections_per_key": 10,
                    "max_frame_bytes": 1048576,
                },
                "authentication_profiles": {
                    "revision": 1,
                    "profiles": [{
                        "id": "cortex-development-users", "name": "Cortex development users",
                        "type": "entra_and_relayna_key", "enabled": True, "entra": entra,
                    }],
                    "bindings": [{"key_id": self.key["id"], "profile_id": "cortex-development-users"}],
                },
            },
        }
        self.calls = []
        self.new_key = False
        self.new_service = False

    def admin_api(self, request, timeout):
        self.assertEqual(timeout, 30)
        path = urlparse(request.full_url).path.removeprefix("/admin-ui/admin/")
        body = json.loads(request.data) if request.data else None
        method = request.get_method()
        self.calls.append((method, path, body))
        if method == "GET":
            responses = {
                "projects": [{"id": "fixture-project-id", "name": "Cortex Kind E2E"}],
                "policy-layers": [{
                    "kind": "project", "scope_id": "fixture-project-id",
                    "policy": {k: v for k, v in self.policy.items()
                               if k not in {"rpm_limit", "monthly_budget_usd"}},
                }],
                "services": [] if self.new_service else [self.service],
                "keys": [] if self.new_key else [self.key],
            }
            response = responses[path]
        elif method == "POST" and path == "services" and self.new_service:
            response = self.service
        elif method == "POST" and path == "keys" and self.new_key:
            response = {"raw_key": "fixture-new-private-key", "key": self.key}
        elif method == "PATCH" and path == "services/cortex-web":
            response = self.service
        else:
            self.fail(f"Unexpected admin request: {method} {path}")
        return io.BytesIO(json.dumps(response).encode())

    def run_bootstrap(self):
        with (
            patch.object(bootstrap, "secret_value", return_value="fixture-service-token"),
            patch.object(bootstrap.urllib.request, "urlopen", side_effect=self.admin_api),
            patch("sys.argv", ["bootstrap-cortex.py", "--key-output", str(self.key_output)]),
            patch("sys.stdout", new_callable=io.StringIO) as output,
        ):
            bootstrap.main()
            return json.loads(output.getvalue())

    def test_matching_key_reuses_private_material_and_profile_revision(self):
        before = self.key_output.read_bytes()
        result = self.run_bootstrap()
        self.assertEqual(result["key_id"], self.key["id"])
        self.assertEqual(result["revision"], 1)
        self.assertEqual(self.key_output.read_bytes(), before)
        self.assertFalse(any(method == "POST" for method, _, _ in self.calls))
        patch_body = next(body for method, _, body in self.calls if method == "PATCH")
        self.assertNotIn("access", patch_body)

    def test_new_service_includes_put_for_feedback_and_settings(self):
        self.new_service = True
        self.run_bootstrap()
        creation = next(body for method, path, body in self.calls
                        if method == "POST" and path == "services")
        self.assertEqual(creation["allowed_methods"], ["GET", "POST", "PUT", "PATCH", "DELETE"])

    def test_missing_put_requires_explicit_operator_update(self):
        self.service["allowed_methods"].remove("PUT")
        before = self.key_output.read_bytes()
        with self.assertRaisesRegex(SystemExit, "missing required methods"):
            self.run_bootstrap()
        self.assertFalse(any(method != "GET" for method, _, _ in self.calls))
        self.assertEqual(self.key_output.read_bytes(), before)

    def test_drifted_key_fails_before_profile_patch_or_private_key_changes(self):
        cases = [
            ("service_names", []),
            ("service_names", ["cortex-web", "unrelated-service"]),
            ("deny", True),
            ("allowed_routes", []),
            ("allowed_providers", []),
            ("allowed_services", []),
            ("allow_streaming", False),
            ("rpm_limit", 99),
            ("monthly_budget_usd", 1),
        ]
        original = copy.deepcopy(self.key)
        before = self.key_output.read_bytes()
        for field, value in cases:
            with self.subTest(field=field, value=value):
                self.key = copy.deepcopy(original)
                self.calls.clear()
                target = self.key if field == "service_names" else self.key["policy"]
                target[field] = value
                with self.assertRaisesRegex(SystemExit, "service or policy has drifted"):
                    self.run_bootstrap()
                self.assertFalse(any(method != "GET" for method, _, _ in self.calls))
                self.assertEqual(self.key_output.read_bytes(), before)

    def test_missing_policy_fields_fail_closed(self):
        for field in ["deny", *self.policy]:
            with self.subTest(field=field):
                value = self.key["policy"].pop(field)
                self.calls.clear()
                with self.assertRaisesRegex(SystemExit, "service or policy has drifted"):
                    self.run_bootstrap()
                self.assertFalse(any(method != "GET" for method, _, _ in self.calls))
                self.key["policy"][field] = value

    def test_created_key_uses_the_same_policy_and_private_output(self):
        self.new_key = True
        result = self.run_bootstrap()
        creation = next(body for method, path, body in self.calls
                        if method == "POST" and path == "keys")
        self.assertEqual(creation["service_names"], ["cortex-web"])
        self.assertEqual(creation["policy"], self.policy)
        self.assertEqual(self.key_output.read_text(), "fixture-new-private-key")
        self.assertEqual(stat.S_IMODE(self.key_output.stat().st_mode), 0o600)
        self.assertEqual(result["key_id"], self.key["id"])


if __name__ == "__main__":
    unittest.main()

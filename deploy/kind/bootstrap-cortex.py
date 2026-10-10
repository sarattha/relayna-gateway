#!/usr/bin/env python3
"""Provision the development Cortex channel through supported gateway admin APIs."""

import argparse
import base64
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import urllib.error
import urllib.request


def secret_value(context, namespace, name, key):
    result = subprocess.run(
        ["kubectl", "--context", context, "-n", namespace, "get", "secret", name, "-o", "json"],
        check=True, capture_output=True, text=True,
    )
    return base64.b64decode(json.loads(result.stdout)["data"][key]).decode()


def save_private(path, value):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as output:
        os.fchmod(output.fileno(), 0o600)
        output.write(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--context", default="kind-keystone")
    parser.add_argument("--namespace", default="cortex-e2e")
    parser.add_argument("--control-url", default="http://127.0.0.1:18381")
    parser.add_argument("--adapter-url", default="http://accessa-adapter:8080")
    parser.add_argument("--key-output", type=Path, default=Path("target/kind-cortex-e2e/relayna-channel.key"))
    args = parser.parse_args()
    if args.key_output.is_symlink():
        raise SystemExit("Refusing a symlink key output")
    admin = secret_value(args.context, args.namespace, "relayna-gateway-secrets", "GATEWAY_ADMIN_TOKEN")
    adapter = secret_value(args.context, args.namespace, "cortex-accessa-secrets", "ACCESSA_ADAPTER_TOKEN")

    def request(method, path, body=None):
        payload = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(
            args.control_url.rstrip("/") + "/admin-ui/admin/" + path,
            data=payload, method=method,
            headers={"Authorization": "Bearer " + admin, "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            # Error bodies are intentionally not echoed: never expose credentials.
            raise SystemExit(f"Gateway admin {method} {path} failed: HTTP {error.code}") from None

    project = next((item for item in request("GET", "projects") if item["name"] == "Cortex Kind E2E"), None)
    if project is None:
        project = request("POST", "projects", {"name": "Cortex Kind E2E"})
    name = "cortex-web"
    # With no inherited layer, the gateway uses its LLM-only default policy.
    # Register the dedicated project's service boundary, without changing global policy.
    project_policy = {"allowed_routes": ["/services/*"], "allowed_providers": ["internal-service"],
                      "allowed_services": [name], "allow_streaming": True}
    layer = next((item for item in request("GET", "policy-layers")
                  if item["kind"] == "project" and item["scope_id"] == project["id"]), None)
    if layer is None or any(layer["policy"].get(field) != value for field, value in project_policy.items()):
        request("POST", "policy-layers", {"kind": "project", "scope_id": project["id"],
                                         "policy": project_policy})
    accessa = {"app": "cortex", "channel": "web", "idle_timeout_ms": 120000,
               "max_connections": 20, "max_connections_per_key": 10, "max_frame_bytes": 1048576}
    entra = {"audience": "api://keystone-mcp",
             "required_scopes": ["keystone.documents.read", "keystone.retrieval.search"],
             "required_roles": [], "allowed_groups": [], "allow_apigee": False}
    service = next((item for item in request("GET", "services") if item["name"] == name), None)
    if service is not None and service["project_id"] != project["id"]:
        raise SystemExit("Existing Cortex service belongs to another project; refusing to replace it")
    if service is None:
        service = request("POST", "services", {
            "name": name, "project_id": project["id"], "route_pattern": "/app/cortex/channel/web/v1/*",
            "upstream_base_url": args.adapter_url, "credential": adapter,
            "allowed_methods": ["GET", "POST", "DELETE"], "timeout_ms": 120000,
            "max_body_bytes": 1048576, "cost_mode": "none",
            "access": {"entra": entra, "accessa": accessa},
        })
    keys = [item for item in request("GET", "keys") if item.get("name") == "Cortex BFF Kind E2E"]
    if keys:
        if len(keys) != 1 or not args.key_output.is_file():
            raise SystemExit("Existing channel key requires its original private key file; rotate explicitly through admin APIs")
        key = keys[0]
        if key["owner_type"] != "project" or key["project_id"] != project["id"]:
            raise SystemExit("Existing Cortex key belongs to another owner; refusing to reuse it")
        if key["disabled"] or key["revoked_at"] is not None:
            raise SystemExit("Existing Cortex key is inactive; restore or rotate explicitly")
        if key["expires_at"] and datetime.fromisoformat(key["expires_at"].replace("Z", "+00:00")) <= datetime.now(timezone.utc):
            raise SystemExit("Existing Cortex key has expired; rotate explicitly")
        if not args.key_output.read_text().strip().startswith(key["key_prefix"]):
            raise SystemExit("Private key file does not match the saved Cortex key")
    else:
        created = request("POST", "keys", {
            "name": "Cortex BFF Kind E2E", "owner_type": "project", "project_id": project["id"],
            "service_names": [name],
            "policy": {"allowed_routes": ["/services/*"], "allowed_providers": ["internal-service"],
                       "allowed_services": [name], "allow_streaming": True, "rpm_limit": 300,
                       "monthly_budget_usd": 10},
        })
        save_private(args.key_output, created["raw_key"])
        key = created["key"]
    profile = {"id": "cortex-development-users", "name": "Cortex development users",
               "type": "entra_and_relayna_key", "enabled": True, "entra": entra}
    existing_profiles = service["access"].get("authentication_profiles") or {}
    patch = {"upstream_base_url": args.adapter_url, "credential": adapter, "enabled": True}
    binding = {"key_id": key["id"], "profile_id": profile["id"]}
    if (existing_profiles.get("profiles") != [profile] or existing_profiles.get("bindings") != [binding]
            or service["access"].get("accessa") != accessa):
        # Avoid advancing the profile revision and invalidating live admissions on an unchanged rerun.
        patch["access"] = {"accessa": accessa, "authentication_profiles": {
            "revision": existing_profiles.get("revision", 0), "profiles": [profile],
            "bindings": [binding],
        }}
    configured = request("PATCH", "services/" + name, patch)
    print(json.dumps({"project_id": project["id"], "key_id": key["id"], "service": name,
                      "profile": profile["id"], "revision": configured["access"]["authentication_profiles"]["revision"],
                      "key_file": str(args.key_output.resolve())}))


if __name__ == "__main__":
    main()

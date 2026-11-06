#!/usr/bin/env python3
"""Smoke-test an immutable runtime image with disposable Docker fixtures."""
import argparse
import asyncio
import contextlib
import hashlib
import json
import os
from pathlib import Path
import secrets
import ssl
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "monitoring"))
from soak import Upstream, frame_write, probe, request, websocket


def verify_zlib_payload(image):
    """Check the shipped files as well as the package version seen by scanners."""
    container = subprocess.check_output(["docker", "create", image], text=True).strip()
    try:
        with tempfile.TemporaryDirectory(prefix="relayna-image-inventory-") as temporary:
            archive = Path(temporary) / "rootfs.tar"
            subprocess.run(["docker", "export", "--output", str(archive), container],
                           check=True, capture_output=True, timeout=120)
            with tarfile.open(archive) as root:
                metadata = root.extractfile("var/lib/dpkg/status.d/zlib1g").read().decode()
                assert "Version: 1:1.3.dfsg+really1.3.2-4\n" in metadata
                libraries = [entry for entry in root.getmembers()
                             if entry.isfile() and "/libz.so.1." in entry.name]
                assert len(libraries) == 1 and libraries[0].name.endswith("/libz.so.1.3.2")
                alias = root.getmember(str(Path(libraries[0].name).with_name("libz.so.1")))
                assert alias.issym() and alias.linkname == "libz.so.1.3.2"
                checksums = root.extractfile("var/lib/dpkg/status.d/zlib1g.md5sums").read().decode()
                for line in checksums.splitlines():
                    expected, filename = line.split(maxsplit=1)
                    # MD5 is Debian's inventory integrity check. The downloaded
                    # package itself is authenticated by a pinned SHA-256.
                    actual = hashlib.md5(root.extractfile(filename).read(), usedforsecurity=False).hexdigest()
                    assert actual == expected, "zlib package inventory does not match its payload"
                assert "tmp/busybox" not in root.getnames()
    finally:
        subprocess.run(["docker", "rm", container], check=True, capture_output=True, timeout=30)


async def serve():
    upstream = Upstream(os.environ["SERVICE_TOKEN"], 0)
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain("/certificates/cert.pem", "/certificates/key.pem")
    servers = [await asyncio.start_server(upstream.handle, "0.0.0.0", 9000),
               await asyncio.start_server(upstream.handle, "0.0.0.0", 9001, ssl=tls)]
    await asyncio.gather(*(server.serve_forever() for server in servers))


async def check(image):
    prefix = "relayna-image-smoke-" + secrets.token_hex(6)
    containers, checks = [], []
    admin = "op_live_" + secrets.token_urlsafe(36)
    credential, password = secrets.token_urlsafe(32), secrets.token_urlsafe(32)

    def docker(*args, environment=None, timeout=120):
        result = subprocess.run(["docker", *args], capture_output=True, text=True,
                                env={**os.environ, **(environment or {})}, timeout=timeout)
        if result.returncode:
            # Docker errors may echo environment arguments or application logs.
            raise RuntimeError("docker_" + args[0] + "_failed")
        return result.stdout.strip()

    def start(role, container_image, environment=None, *extra, command=()):
        name = prefix + "-" + role
        containers.append(name)
        docker("run", "--detach", "--name", name, "--network", prefix,
               "--network-alias", role,
               *[arg for key in (environment or {}) for arg in ("--env", key)],
               *extra, container_image, *command, environment=environment)
        return name

    async def fetch(base, path, token=admin, method="GET", body=None, status=200, extra=None):
        async with asyncio.timeout(20):
            response = await request(base, path, token, method, body, extra)
            try:
                data = await response.body()
                if response.status != status:
                    raise RuntimeError("unexpected_http_status_" + str(response.status))
                return data
            finally:
                await response.close()

    async def api(path, method="GET", body=None):
        return json.loads(await fetch(control, "/admin-ui/admin/" + path, method=method, body=body))

    try:
        assert docker("image", "inspect", image, "--format", "{{.Config.User}}") == "10001:10001"
        verify_zlib_payload(image)
        checks.append("patched_zlib_payload_inventory_and_no_stale_library")
        docker("network", "create", prefix)
        with tempfile.TemporaryDirectory(prefix=prefix) as temporary:
            certificates = Path(temporary)
            certificates.chmod(0o755)
            result = subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                "-keyout", str(certificates / "key.pem"), "-out", str(certificates / "cert.pem"),
                "-days", "1", "-subj", "/CN=upstream", "-addext", "subjectAltName=DNS:upstream",
                "-addext", "basicConstraints=critical,CA:FALSE"],
                capture_output=True, timeout=30)
            if result.returncode:
                raise RuntimeError("certificate_fixture_failed")
            for path in certificates.iterdir():
                path.chmod(0o644)  # Synthetic one-day fixture, readable by UID 10001.
            cert_mount = f"type=bind,src={certificates},dst=/certificates,readonly"
            postgres = start("postgres", "postgres:16-alpine", {
                "POSTGRES_USER": "smoke", "POSTGRES_PASSWORD": password, "POSTGRES_DB": "smoke"})
            start("redis", "redis:7-alpine")
            for _ in range(120):
                # Ignore the socket-only temporary server used during initdb.
                ready = subprocess.run(["docker", "exec", postgres, "pg_isready", "-h", "127.0.0.1", "-U", "smoke"],
                                       capture_output=True, timeout=10)
                if ready.returncode == 0:
                    break
                await asyncio.sleep(.5)
            else:
                raise RuntimeError("postgres_not_ready")
            # Reuse the bounded HTTP/SSE/WebSocket fixture, inside the same network.
            tests = Path(__file__).resolve().parents[1]
            start("upstream", "python:3.13-alpine", {"SERVICE_TOKEN": credential},
                  "--mount", f"type=bind,src={tests},dst=/tests,readonly", "--mount", cert_mount,
                  command=("python", "/tests/runtime-image/run.py", "--serve"))
            environment = {
                "DATABASE_URL": f"postgres://smoke:{password}@postgres:5432/smoke",
                "REDIS_URL": "redis://redis:6379", "LITELLM_BASE_URL": "http://upstream:9000",
                "LITELLM_SERVICE_KEY": credential, "GATEWAY_ADMIN_TOKEN": admin, "LOG_LEVEL": "error",
                "SSL_CERT_FILE": "/certificates/cert.pem", "PORTAL_OIDC_ENABLED": "true",
                "ENTRA_APPLICATION_ID": "image-smoke", "PORTAL_OIDC_TENANT_ID": "image-smoke",
                "PORTAL_OIDC_PRIVATE_KEY_PATH": "/certificates/key.pem",
                "PORTAL_OIDC_CERTIFICATE_PATH": "/certificates/cert.pem",
                "PORTAL_OIDC_ISSUER": "http://upstream:9000",
                "PORTAL_OIDC_DISCOVERY_URL": "http://upstream:9000/.well-known/openid-configuration",
                "PORTAL_OIDC_REDIRECT_URI": "http://localhost/admin-ui/auth/callback",
                "PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI": "http://localhost/admin-ui",
                "PORTAL_SESSION_COOKIE_SECURE": "false"}
            gateway = start("gateway", image, environment, "--read-only", "--cap-drop=ALL",
                "--security-opt=no-new-privileges", "--mount", cert_mount,
                "--publish", "127.0.0.1::8080", "--publish", "127.0.0.1::8081")
            proxy, control = ["http://" + docker("port", gateway, port) for port in ("8080/tcp", "8081/tcp")]
            for _ in range(120):
                try:
                    await fetch(control, "/admin-ui/readyz")
                    break
                except (OSError, EOFError, RuntimeError):
                    if docker("inspect", "--format", "{{.State.Running}}", gateway) != "true":
                        raise RuntimeError("gateway_exited") from None
                    await asyncio.sleep(.5)
            else:
                raise RuntimeError("gateway_not_ready")
            checks.append("startup_migrations_dns_readiness_and_openssl_portal_keypair")
            for path in ("/admin-ui/healthz", "/admin-ui", "/admin-ui/app.js", "/admin-ui/app.css", "/admin-ui/metrics"):
                assert await fetch(control, path)
            await fetch(control, "/admin-ui/admin/projects", token="invalid", status=401)
            checks.append("embedded_assets_metrics_and_admin_denial")
            project = await api("projects", "POST", {"name": prefix})
            await api("policy-layers", "POST", {"kind": "global", "policy": {}, "guardrail_policy": {}})
            service = {"name": "image-smoke", "project_id": project["id"],
                "upstream_base_url": "http://upstream:9000", "credential": credential,
                "allowed_methods": ["GET", "POST"], "timeout_ms": 30000, "cost_mode": "none",
                "access": {"websocket": {"idle_timeout_ms": 30000, "max_connections": 2,
                    "max_connections_per_key": 2, "max_frame_bytes": 65536, "max_session_duration_seconds": 60}}}
            await api("services", "POST", service)
            await api("services", "POST", {**service, "name": "image-tls",
                                           "upstream_base_url": "https://upstream:9001"})
            created = await api("keys", "POST", {"name": prefix, "owner_type": "project",
                "project_id": project["id"], "service_names": [service["name"], "image-tls"], "policy": {
                    "allowed_routes": ["/services/*"], "allowed_providers": ["internal-service"],
                    "allowed_services": [service["name"], "image-tls"], "allow_streaming": True}})
            key, route = created["raw_key"], "/services/image-smoke"
            await fetch(proxy, route + "/http", token="invalid", status=401)
            http_id = prefix + "-http"
            assert json.loads(await fetch(proxy, route + "/http", key, "POST", {"synthetic": True},
                                         extra={"x-request-id": http_id})) == {"ok": True}
            # The fixture takes 1.2 seconds to finish. A first chunk must arrive
            # while it is still producing events, including response headers.
            response = None
            try:
                async with asyncio.timeout(1):
                    response = await request(proxy, route + "/sse", key)
                    assert response.status == 200
                    chunks = response.chunks()
                    first = await anext(chunks)
                    assert b"data:" in first
                async with asyncio.timeout(10):
                    rest = b"".join([chunk async for chunk in chunks])
                assert b"[DONE]" in rest
            finally:
                if response:
                    await response.close()
            socket = await websocket(proxy, route + "/ws", key, prefix)
            try:
                await probe(socket, 1)
                await frame_write(socket.writer, 8, masked=True)
            finally:
                await socket.close()
            checks.append("http_sse_websocket_and_upstream_credential_translation")
            assert json.loads(await fetch(proxy, "/services/image-tls/http", key)) == {"ok": True}
            checks.append("https_sni_and_custom_ca")
            await fetch(control, "/admin-ui/admin/keys/" + created["key"]["id"] + "/disable", method="POST")
            await fetch(proxy, route + "/http", key, status=401)
            checks.append("disabled_key_denial")
            await asyncio.sleep(2)
            usage = await api("usage/events?limit=10")
            assert any(row["request_id"] == http_id and row["status_code"] == 200 for row in usage["rows"])
            status = await api("gateway/status?instance_id=all&window_seconds=60")
            assert not status["unavailable"]
            assert any(not row["stale"] for row in status["instances"])
            checks.append("usage_and_instance_monitoring")
            # Pingora 0.9 retains its released five-minute SIGTERM grace period.
            docker("stop", "--time", "330", gateway, timeout=360)
            assert docker("inspect", "--format", "{{.State.ExitCode}}", gateway) == "0"
            checks.append("sigterm_shutdown")
            print(json.dumps({"passed": True, "image_id": image, "checks": checks}))
    finally:
        for name in reversed(containers):
            with contextlib.suppress(RuntimeError):
                docker("rm", "--force", "--volumes", name)
        with contextlib.suppress(RuntimeError):
            docker("network", "rm", prefix)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", help="local image tag or ID (resolved before starting)")
    parser.add_argument("--serve", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.serve:
        asyncio.run(serve())
    elif args.image:
        image_id = subprocess.check_output(["docker", "image", "inspect", args.image,
                                            "--format", "{{.Id}}"], text=True).strip()
        asyncio.run(check(image_id))
    else:
        parser.error("--image is required")

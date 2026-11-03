#!/usr/bin/env python3
"""Compare complete-body analysis with PR #146 using extracted Rust source.

This is a CPU/allocation microbenchmark, not a proxy throughput or RSS test.
Both variants run in one release binary against the same gateway-core and lockfile.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", default="277851f24a82fd5d870edabcb0a6d1c334fa932f")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--iterations", type=int, default=20)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--prepare-only", action="store_true", help="build now; run the recorded measure_command after other work stops")
    args = parser.parse_args()
    if args.iterations < 1 or args.repetitions < 3:
        parser.error("positive iterations and at least three repetitions required")
    repo = Path(__file__).resolve().parents[2]

    def git(*parts):
        return subprocess.check_output(["git", "-C", str(repo), *parts])

    baseline = git("rev-parse", args.baseline).decode().strip()
    # Otherwise a shared library would silently substitute candidate behavior
    # for the historical implementation under comparison.
    for path in ("Cargo.toml", "Cargo.lock", "crates/gateway-core"):
        if git("diff", baseline, "--", path):
            parser.error(f"shared benchmark dependency differs from baseline: {path}")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    project = output / "probe"
    (project / "src").mkdir(parents=True)
    source_path = "crates/gateway-proxy/src/pingora_plane.rs"
    old = git("show", f"{baseline}:{source_path}").decode()
    new = (repo / source_path).read_text()

    def section(source, start, end):
        return source[source.index(start):source.index(end, source.index(start))]

    imports = "use bytes::Bytes; use chrono::Utc; use gateway_core::*;\n"
    old_helpers = section(old, "fn check_complete_request_policy(", "fn bind_request_body_admission(")
    new_helpers = section(new, "struct CompleteRequestBody {", "fn bind_request_body_admission(")
    # Reproduce metadata extraction at the two policy lookups and original/final
    # admission. Storage I/O, body buffering, guardrails and budgets are excluded.
    old_run = """
pub fn run(body: Bytes, policy: &KeyPolicy, route: &RouteMatch) -> i64 {
    let features = analyze_generation_request(&body).unwrap().features;
    std::hint::black_box(extract_generation_features(&body));
    check_complete_request_policy(policy, route, &body, &features).unwrap();
    let features = extract_generation_features(&body);
    std::hint::black_box(extract_generation_features(&body));
    check_complete_request_policy(policy, route, &body, &features).unwrap();
    policy.tpm_limit.map_or(0, |_| estimate_generation_tokens(&body))
}
"""
    new_run = """
pub fn run(bytes: Bytes, policy: &KeyPolicy, route: &RouteMatch) -> i64 {
    let body = CompleteRequestBody::new(bytes);
    let features = body.features();
    std::hint::black_box(body.features());
    check_complete_request_policy(policy, route, &body, &features).unwrap();
    let features = body.features();
    std::hint::black_box(body.features());
    check_complete_request_policy(policy, route, &body, &features).unwrap();
    policy.tpm_limit.map_or(0, |_| body.estimated_tokens())
}
"""
    for name, helpers, wrapper in (("baseline", old_helpers, old_run), ("candidate", new_helpers, new_run)):
        (project / "src" / f"{name}.rs").write_text(imports + helpers + wrapper)
    driver = Path(__file__).with_name("analysis_driver.rs").read_text()
    (project / "src/main.rs").write_text(driver)
    core = json.dumps(str(repo / "crates/gateway-core"))
    (project / "Cargo.toml").write_text(f'''[package]
name = "request-analysis-probe"
version = "0.0.0"
edition = "2021"
[workspace]
[dependencies]
gateway-core = {{ path = {core} }}
bytes = "1.11.1"
chrono = "0.4.42"
http = "1.4.0"
serde_json = {{ version = "1.0.145", features = ["float_roundtrip", "raw_value"] }}
''')
    (project / "Cargo.lock").write_bytes((repo / "Cargo.lock").read_bytes())
    env = dict(os.environ, CARGO_TARGET_DIR=str(repo / "target/request-analysis-probe"))
    manifest = {
        "baseline": baseline, "candidate_head": git("rev-parse", "HEAD").decode().strip(),
        "candidate_diff_sha256": hashlib.sha256(git("diff", "HEAD")).hexdigest(),
        "source_sha256": {"baseline": hashlib.sha256(old.encode()).hexdigest(), "candidate": hashlib.sha256(new.encode()).hexdigest()},
        "host": platform.platform(), "architecture": platform.machine(),
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
        "iterations": args.iterations, "repetitions": args.repetitions,
        "harness_sha256": {name: hashlib.sha256(Path(__file__).with_name(name).read_bytes()).hexdigest() for name in ("analysis_benchmark.py", "analysis_driver.rs")},
        "method": "Matched source-extracted analysis pipeline; AB/BA alternating order; warmed release binary; synthetic flat and message-array JSON. Not end-to-end proxy, RSS, or capacity evidence.",
    }
    (output / "provenance.json").write_text(json.dumps(manifest, indent=2) + "\n")
    with (output / "build.log").open("w") as log:
        subprocess.run(["cargo", "build", "--release", "--offline", "--manifest-path", str(project / "Cargo.toml")], env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    binary = Path(env["CARGO_TARGET_DIR"]) / "release/request-analysis-probe"
    manifest["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
    manifest["probe_lock_sha256"] = hashlib.sha256((project / "Cargo.lock").read_bytes()).hexdigest()
    manifest["measure_command"] = [str(binary), str(args.iterations), str(args.repetitions)]
    (output / "provenance.json").write_text(json.dumps(manifest, indent=2) + "\n")
    if args.prepare_only:
        print(f"Prepared probe; measure_command is recorded in {output / 'provenance.json'}")
        return
    with (output / "trials.jsonl").open("w") as trials:
        subprocess.run([str(binary), str(args.iterations), str(args.repetitions)], stdout=trials, check=True)
    print(f"Recorded source, provenance and trials in {output}")


if __name__ == "__main__":
    main()

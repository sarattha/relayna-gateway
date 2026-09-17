#!/usr/bin/env python3
"""Require >98% LLVM line coverage for changed production Rust (rustfmt source).

Usage: python3 scripts/check-changed-rust-coverage.py report.lcov [base-ref]
Includes staged, unstaged and untracked Rust files. Excludes integration tests,
benches and cfg(test) items, not production error paths. LLVM DA records define
executable lines; missing source-file coverage fails closed.
"""
import json
from pathlib import Path
import re
import subprocess
import sys

root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
base = sys.argv[2] if len(sys.argv) > 2 else "HEAD"
changed = {}
for line in subprocess.check_output(["git", "diff", "--no-ext-diff", "-U0", base, "--", "*.rs"], text=True, cwd=root).splitlines():
    if line.startswith("+++ b/"):
        path = line[6:]
        changed[path] = set()
    elif line.startswith("@@"):
        match = re.search(r"\+(\d+)(?:,(\d+))?", line)
        start = int(match[1])
        changed[path].update(range(start, start + int(match[2] or 1)))
for path in subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True, cwd=root).splitlines():
    if path.endswith(".rs"):
        changed[path] = set(range(1, len((root / path).read_text().splitlines()) + 1))
coverage = {}
for line in Path(sys.argv[1]).read_text().splitlines():
    if line.startswith("SF:"):
        source = Path(line[3:])
        path = str(source.relative_to(root)) if source.is_absolute() else str(source)
        coverage.setdefault(path, {})
    elif line.startswith("DA:"):
        number, count = map(int, line[3:].split(",")[:2])
        coverage[path][number] = coverage[path].get(number, 0) + count
report = {}
for path, lines in changed.items():
    if "/tests/" in path or "/benches/" in path or not lines:
        continue
    source = (root / path).read_text().splitlines()
    # rustfmt places the item's closing brace at its original indentation.
    excluded = set()
    for index, line in enumerate(source):
        if line.strip() != "#[cfg(test)]":
            continue
        indent = line[:len(line) - len(line.lstrip())]
        end = next((i for i in range(index + 1, len(source)) if source[i] == indent + "}"), len(source) - 1)
        excluded.update(range(index + 1, end + 2))
    production = lines - excluded
    if not production:
        continue
    if path not in coverage and any(source[n - 1].strip() and not source[n - 1].lstrip().startswith(("//", "pub use", "pub mod", "mod ", "#")) for n in production):
        raise SystemExit(f"No LLVM coverage records for changed source: {path}")
    data = {n: count for n, count in coverage.get(path, {}).items() if n in production}
    report[path] = {"covered": sum(count > 0 for count in data.values()), "executable": len(data), "uncovered_lines": [n for n, count in data.items() if count == 0]}
covered = sum(item["covered"] for item in report.values())
total = sum(item["executable"] for item in report.values())
percent = covered / total * 100 if total else 0
print(json.dumps({"base": base, "scope": "changed production Rust executable lines; tests excluded", "covered": covered, "executable": total, "percent": percent, "files": report}, indent=2))
raise SystemExit(0 if percent > 98 else 1)

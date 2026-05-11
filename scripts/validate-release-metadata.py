#!/usr/bin/env python3
import argparse
import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]


def workspace_version(cargo_toml: pathlib.Path) -> str:
    in_workspace_package = False
    for line in cargo_toml.read_text().splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            in_workspace_package = stripped == "[workspace.package]"
            continue
        if in_workspace_package:
            match = re.match(r'version\s*=\s*"([^"]+)"', stripped)
            if match:
                return match.group(1)
    raise ValueError("missing [workspace.package] version in Cargo.toml")


def changelog_has_version(changelog: pathlib.Path, version: str) -> bool:
    heading = re.compile(rf"^##\s+{re.escape(version)}\s+-")
    return any(heading.match(line) for line in changelog.read_text().splitlines())


def normalize_tag(tag: str) -> str:
    tag = tag.strip()
    if not tag:
        raise ValueError("release tag is empty")
    return tag[1:] if tag.startswith("v") else tag


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate release tag, workspace version, and changelog metadata."
    )
    parser.add_argument(
        "tag",
        nargs="?",
        help="Release tag, for example v0.0.3. Omit to validate the current workspace version.",
    )
    args = parser.parse_args()

    cargo_version = workspace_version(ROOT / "Cargo.toml")
    version = normalize_tag(args.tag) if args.tag else cargo_version
    if version != cargo_version:
        print(
            f"release tag version {version} does not match Cargo.toml workspace version {cargo_version}",
            file=sys.stderr,
        )
        return 1

    if not changelog_has_version(ROOT / "CHANGELOG.md", version):
        print(f"CHANGELOG.md is missing a section for {version}", file=sys.stderr)
        return 1

    print(f"release metadata validated for {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

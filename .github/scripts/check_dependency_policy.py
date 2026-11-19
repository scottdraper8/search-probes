#!/usr/bin/env python3
"""Validate Search Probes' pinned Cargo dependency and toolchain contract."""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path

DEPENDENCY_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")
PINNED_TOOLCHAIN = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+")


def inspect_dependencies(
    manifest: Path, section: str, dependencies: dict[str, object]
) -> list[str]:
    failures = []
    for name, specification in dependencies.items():
        version = (
            specification
            if isinstance(specification, str)
            else specification.get("version")
            if isinstance(specification, dict)
            else None
        )
        if version is not None and (
            not isinstance(version, str) or not version.startswith("=")
        ):
            failures.append(
                f"{manifest}: {section}.{name} must use an exact '=VERSION' pin, got {version!r}"
            )
    return failures


def manifest_failures(root: Path) -> list[str]:
    failures = []
    tracked = subprocess.check_output(
        ["git", "ls-files"], cwd=root, text=True
    ).splitlines()
    for relative in sorted(path for path in tracked if path.endswith("Cargo.toml")):
        manifest = root / relative
        with manifest.open("rb") as stream:
            document = tomllib.load(stream)
        for section in DEPENDENCY_SECTIONS:
            failures.extend(
                inspect_dependencies(manifest, section, document.get(section, {}))
            )
            failures.extend(
                inspect_dependencies(
                    manifest,
                    f"workspace.{section}",
                    document.get("workspace", {}).get(section, {}),
                )
            )
        for target, configuration in document.get("target", {}).items():
            for section in DEPENDENCY_SECTIONS:
                failures.extend(
                    inspect_dependencies(
                        manifest,
                        f"target.{target}.{section}",
                        configuration.get(section, {}),
                    )
                )
    return failures


def toolchain_failures(root: Path) -> list[str]:
    path = root / "rust-toolchain.toml"
    with path.open("rb") as stream:
        toolchain = tomllib.load(stream).get("toolchain", {})
    failures = []
    channel = toolchain.get("channel")
    if not isinstance(channel, str) or PINNED_TOOLCHAIN.fullmatch(channel) is None:
        failures.append("rust-toolchain.toml must pin a full numeric Rust version")
    components = toolchain.get("components")
    if not isinstance(components, list) or not {"rustfmt", "clippy"}.issubset(components):
        failures.append("rust-toolchain.toml must install rustfmt and clippy")
    targets = toolchain.get("targets")
    if not isinstance(targets, list) or "x86_64-pc-windows-gnu" not in targets:
        failures.append("rust-toolchain.toml must install the Windows GNU release target")
    return failures


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    failures = manifest_failures(root) + toolchain_failures(root)
    metadata = subprocess.run(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if metadata.returncode:
        failures.append(f"Cargo metadata/lock validation failed:\n{metadata.stderr.strip()}")
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

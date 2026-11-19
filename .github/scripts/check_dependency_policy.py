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
PINNED_REVISION = re.compile(r"[0-9a-f]{40}")
ACTION_REFERENCE = re.compile(r"^\s*uses:\s*[^\s@]+@([^\s#]+)", re.MULTILINE)
PRECOMMIT_REPOSITORY = re.compile(
    r"^\s*-\s+repo:\s*(?!local\s*$)\S+.*?^\s+rev:\s*([^\s#]+)",
    re.MULTILINE | re.DOTALL,
)


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
        if isinstance(specification, dict) and specification.get("git") is not None:
            failures.append(
                f"{manifest}: {section}.{name} must not use a Git dependency"
            )
        if version is not None and (
            not isinstance(version, str) or not version.startswith("=")
        ):
            failures.append(
                f"{manifest}: {section}.{name} must use an exact '=VERSION' pin, got {version!r}"
            )
    return failures


def immutable_reference_failures(root: Path) -> list[str]:
    failures = []
    for workflow in sorted((root / ".github" / "workflows").glob("*.y*ml")):
        text = workflow.read_text(encoding="utf-8")
        for revision in ACTION_REFERENCE.findall(text):
            if PINNED_REVISION.fullmatch(revision) is None:
                failures.append(
                    f"{workflow}: GitHub Action revision {revision!r} is not a full commit SHA"
                )
    precommit = root / ".pre-commit-config.yaml"
    text = precommit.read_text(encoding="utf-8")
    for revision in PRECOMMIT_REPOSITORY.findall(text):
        if PINNED_REVISION.fullmatch(revision) is None:
            failures.append(
                f"{precommit}: hook revision {revision!r} is not a full commit SHA"
            )
    return failures


def dependabot_failures(root: Path) -> list[str]:
    path = root / ".github" / "dependabot.yml"
    text = path.read_text(encoding="utf-8")
    failures = []
    for ecosystem in ("cargo", "github-actions"):
        marker = f"package-ecosystem: {ecosystem}"
        if marker not in text:
            failures.append(f"{path}: missing {ecosystem} security-update configuration")
    if text.count("open-pull-requests-limit: 0") != 2:
        failures.append(f"{path}: routine version-update PRs must be disabled")
    if "versioning-strategy: increase" not in text:
        failures.append(f"{path}: Cargo updates must preserve exact dependency pins")
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
    failures = (
        manifest_failures(root)
        + toolchain_failures(root)
        + immutable_reference_failures(root)
        + dependabot_failures(root)
    )
    if not (root / "Cargo.lock").is_file():
        failures.append("Cargo.lock is required")
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

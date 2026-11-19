#!/usr/bin/env python3
"""Run Windows-target Clippy natively or in the repository toolchain container."""

from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import sys
from pathlib import Path

TARGET = "x86_64-pc-windows-gnu"
NATIVE_TOOLS = (
    "x86_64-w64-mingw32-gcc",
    "x86_64-w64-mingw32-g++",
    "x86_64-w64-mingw32-ar",
    "x86_64-w64-mingw32-dlltool",
)
CLIPPY = (
    "cargo",
    "clippy",
    "--workspace",
    "--all-targets",
    "--all-features",
    "--locked",
    "--target",
    TARGET,
)


def _run(command: list[str] | tuple[str, ...], *, cwd: Path, env: dict[str, str]) -> None:
    print("+ " + " ".join(command), file=sys.stderr)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def _tool_home(variable: str, fallback: str) -> Path:
    path = Path(os.environ.get(variable, Path.home() / fallback)).expanduser().resolve()
    if not path.is_dir():
        raise RuntimeError(f"{variable} directory does not exist: {path}")
    return path


def _container_command(engine: str, root: Path, image: str) -> list[str]:
    cargo_home = _tool_home("CARGO_HOME", ".cargo")
    rustup_home = _tool_home("RUSTUP_HOME", ".rustup")
    command = [engine, "run", "--rm", "--security-opt=label=disable"]
    if Path(engine).name == "podman":
        command.append("--userns=keep-id")
    elif hasattr(os, "getuid"):
        command.extend(("--user", f"{os.getuid()}:{os.getgid()}"))
    command.extend(
        (
            "--volume",
            f"{root}:/workspace:rw",
            "--volume",
            f"{cargo_home}:/cargo:rw",
            "--volume",
            f"{rustup_home}:/rustup:rw",
            "--env",
            "CARGO_BUILD_WARNINGS=deny",
            "--env",
            "CARGO_HOME=/cargo",
            "--env",
            "CARGO_TARGET_DIR=/workspace/target/windows-container",
            "--env",
            "HOME=/tmp",
            "--env",
            "RUSTUP_HOME=/rustup",
            "--env",
            "PATH=/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            "--workdir",
            "/workspace",
            image,
            *CLIPPY,
        )
    )
    return command


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    environment = dict(os.environ, CARGO_BUILD_WARNINGS="deny")
    missing = [tool for tool in NATIVE_TOOLS if shutil.which(tool) is None]
    if not missing:
        _run(CLIPPY, cwd=root, env=environment)
        return 0

    engine = shutil.which("podman") or shutil.which("docker")
    if engine is None:
        print(
            "Windows-target validation requires either the MinGW tools "
            f"({', '.join(missing)}) or Podman/Docker.",
            file=sys.stderr,
        )
        return 2

    containerfile = root / "tooling" / "windows.Containerfile"
    digest = hashlib.sha256(containerfile.read_bytes()).hexdigest()[:16]
    image = f"localhost/search-probes-windows-toolchain:{digest}"
    _run(
        [
            engine,
            "build",
            "--pull=missing",
            "--tag",
            image,
            "--file",
            str(containerfile),
            str(containerfile.parent),
        ],
        cwd=root,
        env=environment,
    )
    _run(_container_command(engine, root, image), cwd=root, env=environment)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Windows-target validation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error

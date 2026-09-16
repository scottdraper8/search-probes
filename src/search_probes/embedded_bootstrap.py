"""Start the Search Probes pyMHF runtime from the in-process WINMM bootstrap."""

from __future__ import annotations

import ctypes
import hashlib
import importlib
import os
import time
import traceback
from pathlib import Path

EXPECTED_EXE_SHA256 = "78c1d883a8d47c99308795ee22e8bdf7c03970f0af090effa45107cefb194ba4"
WINDOW_TITLE = "No Man's Sky"
WINDOW_WAIT_SECONDS = 180.0


def _executable_path() -> Path:
    buffer = ctypes.create_unicode_buffer(32768)
    length = ctypes.windll.kernel32.GetModuleFileNameW(None, buffer, len(buffer))
    if not length:
        raise OSError(
            f"GetModuleFileNameW failed: {ctypes.windll.kernel32.GetLastError()}"
        )
    return Path(buffer.value)


def _image_layout() -> tuple[int, int]:
    kernel32 = ctypes.windll.kernel32
    kernel32.GetModuleHandleW.argtypes = [ctypes.c_wchar_p]
    kernel32.GetModuleHandleW.restype = ctypes.c_void_p
    base = int(kernel32.GetModuleHandleW(None) or 0)
    if not base:
        raise RuntimeError("GetModuleHandleW(NULL) returned no executable base")
    pe_offset = ctypes.c_uint32.from_address(base + 0x3C).value
    if ctypes.string_at(base + pe_offset, 4) != b"PE\0\0":
        raise RuntimeError("the host module does not have a PE signature")
    optional_header = base + pe_offset + 24
    if ctypes.c_uint16.from_address(optional_header).value != 0x20B:
        raise RuntimeError("the host module is not PE32+")
    size = ctypes.c_uint32.from_address(optional_header + 56).value
    if size < 0x1000:
        raise RuntimeError(f"implausible host image size: {size}")
    return base, size


def _wait_for_window() -> None:
    user32 = ctypes.windll.user32
    user32.FindWindowW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p]
    user32.FindWindowW.restype = ctypes.c_void_p
    deadline = time.monotonic() + WINDOW_WAIT_SECONDS
    while time.monotonic() < deadline:
        if user32.FindWindowW(None, WINDOW_TITLE):
            return
        time.sleep(0.1)
    raise TimeoutError(
        f"{WINDOW_TITLE!r} window did not appear within {WINDOW_WAIT_SECONDS}s"
    )


def _configure_pymhf(app_root: Path, executable: Path) -> None:
    import pymhf._preinject
    import pymhf.core._internal as internal

    base, size = _image_layout()
    kernel32 = ctypes.windll.kernel32
    kernel32.GetCurrentProcess.restype = ctypes.c_void_p
    binary_hash = hashlib.sha256(executable.read_bytes()).hexdigest()
    if binary_hash != EXPECTED_EXE_SHA256:
        raise RuntimeError(
            "NMS.exe hash changed; Search Probes refuses current-build native addresses: "
            f"{binary_hash}"
        )
    state_root = app_root / ".resident-search"
    state_root.mkdir(parents=True, exist_ok=True)
    (app_root / "logs").mkdir(parents=True, exist_ok=True)
    (app_root / ".cache").mkdir(parents=True, exist_ok=True)
    module = app_root / "search_probes/resident_search_probe.py"
    internal.MODULE_PATH = str(module)
    internal.BASE_ADDRESS = base
    internal.SIZE_OF_IMAGE = size
    internal.CWD = str(Path(pymhf._preinject.__file__).resolve().parent)
    internal.PID = os.getpid()
    internal.HANDLE = int(kernel32.GetCurrentProcess() or 0)
    internal.BINARY_HASH = binary_hash
    internal.CONFIG = {
        "logging": {
            "shown": False,
            "log_dir": str(app_root / "logs"),
            "log_level": "debug",
        },
        "gui": {"shown": False},
        "internal_mod_dir": None,
    }
    internal.EXE_NAME = executable.name
    internal.BINARY_PATH = str(executable)
    internal.LOAD_TYPE = 1
    internal.MOD_SAVE_DIR = str(state_root / "mod-saves")
    internal.INCLUDED_ASSEMBLIES = {}
    internal.CACHE_DIR = str(app_root / ".cache")
    internal._SENTINEL_PTR = 0


def main() -> None:
    app_root = Path(__file__).resolve().parents[1]
    failure_log = app_root.parent / "bootstrap-error.log"
    try:
        executable = _executable_path()
        _wait_for_window()
        _configure_pymhf(app_root, executable)
        importlib.import_module(
            "pymhf.injected"
        )  # importing runs the resident event loop
    except BaseException:
        failure_log.write_text(traceback.format_exc(), encoding="utf-8")
        raise

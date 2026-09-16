"""Small x64 absolute-jump hook used when MinHook cannot reserve a near relay.

Validates and relocates whole instructions, patches only while every other process thread is
suspended, and restores the exact original bytes on close.
"""

from __future__ import annotations

import ctypes
import os
import threading
import time
from collections.abc import Callable
from typing import Self

from iced_x86 import BlockEncoder, Code, Decoder

if os.name != "nt":
    raise RuntimeError("absolute_hook is Windows-only")


MEM_COMMIT = 0x1000
MEM_RESERVE = 0x2000
MEM_RELEASE = 0x8000
PAGE_EXECUTE_READWRITE = 0x40
MEM_FREE = 0x10000
TH32CS_SNAPTHREAD = 0x00000004
THREAD_SUSPEND_RESUME = 0x0002
THREAD_QUERY_LIMITED_INFORMATION = 0x0800
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
ERROR_ACCESS_DENIED = 5
ERROR_INVALID_HANDLE = 6
ERROR_INVALID_PARAMETER = 87
STILL_ACTIVE = 259
ABSOLUTE_JUMP_SIZE = 14
ALLOCATION_GRANULARITY = 0x10000
MAX_RELATIVE_DISTANCE = 0x7FFF0000


class THREADENTRY32(ctypes.Structure):
    _fields_ = [
        ("dwSize", ctypes.c_uint32),
        ("cntUsage", ctypes.c_uint32),
        ("th32ThreadID", ctypes.c_uint32),
        ("th32OwnerProcessID", ctypes.c_uint32),
        ("tpBasePri", ctypes.c_int32),
        ("tpDeltaPri", ctypes.c_int32),
        ("dwFlags", ctypes.c_uint32),
    ]


class MEMORY_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("BaseAddress", ctypes.c_void_p),
        ("AllocationBase", ctypes.c_void_p),
        ("AllocationProtect", ctypes.c_uint32),
        ("RegionSize", ctypes.c_size_t),
        ("State", ctypes.c_uint32),
        ("Protect", ctypes.c_uint32),
        ("Type", ctypes.c_uint32),
    ]


kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
kernel32.CloseHandle.restype = ctypes.c_int
kernel32.CreateToolhelp32Snapshot.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
kernel32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
kernel32.FlushInstructionCache.argtypes = [
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_size_t,
]
kernel32.FlushInstructionCache.restype = ctypes.c_int
kernel32.GetCurrentProcess.restype = ctypes.c_void_p
kernel32.GetCurrentProcessId.restype = ctypes.c_uint32
kernel32.GetCurrentThreadId.restype = ctypes.c_uint32
kernel32.OpenThread.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
kernel32.OpenThread.restype = ctypes.c_void_p
kernel32.ResumeThread.argtypes = [ctypes.c_void_p]
kernel32.ResumeThread.restype = ctypes.c_uint32
kernel32.SuspendThread.argtypes = [ctypes.c_void_p]
kernel32.SuspendThread.restype = ctypes.c_uint32
kernel32.GetExitCodeThread.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
kernel32.GetExitCodeThread.restype = ctypes.c_int
kernel32.Thread32First.argtypes = [ctypes.c_void_p, ctypes.POINTER(THREADENTRY32)]
kernel32.Thread32First.restype = ctypes.c_int
kernel32.Thread32Next.argtypes = [ctypes.c_void_p, ctypes.POINTER(THREADENTRY32)]
kernel32.Thread32Next.restype = ctypes.c_int
kernel32.VirtualAlloc.argtypes = [
    ctypes.c_void_p,
    ctypes.c_size_t,
    ctypes.c_uint32,
    ctypes.c_uint32,
]
kernel32.VirtualAlloc.restype = ctypes.c_void_p
kernel32.VirtualFree.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_uint32]
kernel32.VirtualFree.restype = ctypes.c_int
kernel32.VirtualProtect.argtypes = [
    ctypes.c_void_p,
    ctypes.c_size_t,
    ctypes.c_uint32,
    ctypes.POINTER(ctypes.c_uint32),
]
kernel32.VirtualProtect.restype = ctypes.c_int
kernel32.VirtualQuery.argtypes = [
    ctypes.c_void_p,
    ctypes.POINTER(MEMORY_BASIC_INFORMATION),
    ctypes.c_size_t,
]
kernel32.VirtualQuery.restype = ctypes.c_size_t


def _winerror(action: str) -> OSError:
    code = ctypes.get_last_error()
    return OSError(code, f"{action} failed: {ctypes.FormatError(code)}")


def absolute_jump(destination: int) -> bytes:
    """Encode ``jmp qword ptr [rip]`` followed by the 64-bit destination."""

    return b"\xff\x25\x00\x00\x00\x00" + int(destination).to_bytes(8, "little")


def _decode_prefix(target: int) -> tuple[list[object], int, bytes]:
    source = ctypes.string_at(target, 64)
    decoder = Decoder(64, source, ip=target)
    instructions: list[object] = []
    length = 0
    while length < ABSOLUTE_JUMP_SIZE:
        instruction = decoder.decode()
        if instruction.code == Code.INVALID:
            raise RuntimeError(
                f"invalid instruction while decoding hook target 0x{target:X}"
            )
        instructions.append(instruction)
        length += instruction.len
    return instructions, length, source[:length]


def _thread_ids() -> list[int]:
    snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    if snapshot == INVALID_HANDLE_VALUE:
        raise _winerror("CreateToolhelp32Snapshot")
    entry = THREADENTRY32()
    entry.dwSize = ctypes.sizeof(entry)
    process_id = kernel32.GetCurrentProcessId()
    current_id = kernel32.GetCurrentThreadId()
    result: list[int] = []
    try:
        more = bool(kernel32.Thread32First(snapshot, ctypes.byref(entry)))
        while more:
            if (
                entry.th32OwnerProcessID == process_id
                and entry.th32ThreadID != current_id
            ):
                result.append(int(entry.th32ThreadID))
            entry.dwSize = ctypes.sizeof(entry)
            more = bool(kernel32.Thread32Next(snapshot, ctypes.byref(entry)))
    finally:
        kernel32.CloseHandle(snapshot)
    return result


def _suspend_other_threads() -> list[ctypes.c_void_p]:
    handles: list[ctypes.c_void_p] = []
    try:
        for thread_id in _thread_ids():
            for attempt in range(3):
                handle = kernel32.OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    False,
                    thread_id,
                )
                if not handle:
                    error = ctypes.get_last_error()
                    if error in (ERROR_INVALID_HANDLE, ERROR_INVALID_PARAMETER):
                        break
                    raise OSError(
                        error,
                        f"OpenThread({thread_id}) failed: {ctypes.FormatError(error)}",
                    )
                if kernel32.SuspendThread(handle) != 0xFFFFFFFF:
                    handles.append(handle)
                    break

                error = ctypes.get_last_error()
                exit_code = ctypes.c_uint32()
                exit_query_ok = bool(
                    kernel32.GetExitCodeThread(handle, ctypes.byref(exit_code))
                )
                exit_query_error = ctypes.get_last_error()
                exited = (
                    exit_query_ok and exit_code.value != STILL_ACTIVE
                ) or (
                    not exit_query_ok
                    and exit_query_error
                    in (ERROR_INVALID_HANDLE, ERROR_INVALID_PARAMETER)
                )
                kernel32.CloseHandle(handle)
                if exited:
                    break
                if error not in (
                    ERROR_ACCESS_DENIED,
                    ERROR_INVALID_HANDLE,
                    ERROR_INVALID_PARAMETER,
                ) or attempt == 2:
                    raise OSError(
                        error,
                        f"SuspendThread({thread_id}) failed: {ctypes.FormatError(error)}",
                    )
                time.sleep(0.01)
            else:
                raise RuntimeError(f"could not suspend thread {thread_id}")
    except Exception:
        _resume_threads(handles)
        raise
    return handles


def _resume_threads(handles: list[ctypes.c_void_p]) -> None:
    for handle in reversed(handles):
        kernel32.ResumeThread(handle)
        kernel32.CloseHandle(handle)


def _write_code(address: int, data: bytes) -> None:
    previous = ctypes.c_uint32()
    if not kernel32.VirtualProtect(
        address, len(data), PAGE_EXECUTE_READWRITE, ctypes.byref(previous)
    ):
        raise _winerror("VirtualProtect(enable write)")
    try:
        ctypes.memmove(address, data, len(data))
        if not kernel32.FlushInstructionCache(
            kernel32.GetCurrentProcess(), address, len(data)
        ):
            raise _winerror("FlushInstructionCache")
    finally:
        ignored = ctypes.c_uint32()
        kernel32.VirtualProtect(
            address, len(data), previous.value, ctypes.byref(ignored)
        )


def _allocate_trampoline_near(target: int) -> int:
    """Allocate one executable page within rel32 reach of ``target``."""

    lower = max(ALLOCATION_GRANULARITY, target - MAX_RELATIVE_DISTANCE)
    upper = target + MAX_RELATIVE_DISTANCE
    address = lower
    info = MEMORY_BASIC_INFORMATION()
    while address < upper:
        queried = kernel32.VirtualQuery(
            address, ctypes.byref(info), ctypes.sizeof(info)
        )
        if not queried:
            break
        base = int(info.BaseAddress or 0)
        region_end = base + int(info.RegionSize)
        if info.State == MEM_FREE:
            candidate = (max(base, lower) + ALLOCATION_GRANULARITY - 1) & ~(
                ALLOCATION_GRANULARITY - 1
            )
            if candidate + 0x1000 <= min(region_end, upper):
                allocated = kernel32.VirtualAlloc(
                    candidate,
                    0x1000,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                )
                if allocated:
                    return int(allocated)
        if region_end <= address:
            break
        address = region_end
    raise RuntimeError(f"no free trampoline page within rel32 reach of 0x{target:X}")


class AbsoluteHook:
    """Validated reversible hook for one x64 function."""

    def __init__(
        self,
        target: int,
        signature: type[ctypes._CFuncPtr],
        detour: Callable[..., object],
        *,
        expected_prefix: bytes | None = None,
    ) -> None:
        self.target = int(target)
        self.signature = signature
        self.detour = detour
        self.expected_prefix = expected_prefix
        self.stolen_length = 0
        self.original_bytes = b""
        self.patched_bytes = b""
        self.trampoline_address = 0
        self.original: Callable[..., object] | None = None
        self.callback: ctypes._CFuncPtr | None = None
        self.installed = False
        self._active_callbacks = 0
        self._callback_threads: set[int] = set()
        self._callback_condition = threading.Condition()

    def install(self) -> AbsoluteHook:
        if self.installed or self.trampoline_address:
            raise RuntimeError("hook is already installed or awaiting finalization")
        instructions, stolen_length, original_bytes = _decode_prefix(self.target)
        if self.expected_prefix is not None and not original_bytes.startswith(
            self.expected_prefix
        ):
            raise RuntimeError(
                f"target 0x{self.target:X} prefix changed: {original_bytes.hex()}"
            )
        try:
            trampoline = _allocate_trampoline_near(self.target)
        except RuntimeError as near_allocation_error:
            trampoline = kernel32.VirtualAlloc(
                None,
                0x1000,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
            if not trampoline:
                raise _winerror("VirtualAlloc(trampoline)") from near_allocation_error
        try:
            encoder = BlockEncoder(64)
            encoder.add_many(instructions)
            relocated = encoder.encode(trampoline)
            trampoline_code = relocated + absolute_jump(self.target + stolen_length)
            if len(trampoline_code) > 0x1000:
                raise RuntimeError("trampoline exceeds one page")
            ctypes.memmove(trampoline, trampoline_code, len(trampoline_code))
            if not kernel32.FlushInstructionCache(
                kernel32.GetCurrentProcess(), trampoline, len(trampoline_code)
            ):
                raise _winerror("FlushInstructionCache(trampoline)")

            original = self.signature(trampoline)

            def callback(*args):
                thread_id = int(kernel32.GetCurrentThreadId())
                with self._callback_condition:
                    self._active_callbacks += 1
                    self._callback_threads.add(thread_id)
                try:
                    return self.detour(original, *args)
                finally:
                    with self._callback_condition:
                        self._active_callbacks -= 1
                        self._callback_threads.discard(thread_id)
                        self._callback_condition.notify_all()

            callback_pointer = self.signature(callback)
            callback_address = ctypes.cast(callback_pointer, ctypes.c_void_p).value
            if not callback_address:
                raise RuntimeError("ctypes callback has no address")
            patch = absolute_jump(callback_address).ljust(stolen_length, b"\x90")
            handles = _suspend_other_threads()
            try:
                if ctypes.string_at(self.target, stolen_length) != original_bytes:
                    raise RuntimeError("target bytes changed before patch")
                _write_code(self.target, patch)
            finally:
                _resume_threads(handles)
        except Exception:
            kernel32.VirtualFree(trampoline, 0, MEM_RELEASE)
            raise

        self.stolen_length = stolen_length
        self.original_bytes = original_bytes
        self.patched_bytes = patch
        self.trampoline_address = int(trampoline)
        self.original = original
        self.callback = callback_pointer
        self.installed = True
        return self

    def restore(self) -> None:
        """Restore the target bytes while retaining callback storage until invocations drain."""

        if not self.installed:
            return
        handles = _suspend_other_threads()
        try:
            actual = ctypes.string_at(self.target, self.stolen_length)
            if actual != self.patched_bytes:
                raise RuntimeError(
                    f"hook target 0x{self.target:X} changed before restoration: {actual.hex()}"
                )
            _write_code(self.target, self.original_bytes)
        finally:
            _resume_threads(handles)
        self.installed = False

    def close(self, timeout: float = 5.0) -> None:
        """Restore the target, drain active callbacks, and release trampoline storage."""

        self.restore()
        if not self.trampoline_address:
            return
        current_thread = int(kernel32.GetCurrentThreadId())
        with self._callback_condition:
            if current_thread in self._callback_threads:
                raise RuntimeError(
                    "cannot finalize a hook from inside its active callback"
                )
            deadline = time.monotonic() + timeout
            while self._active_callbacks:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(
                        f"hook 0x{self.target:X} still has "
                        f"{self._active_callbacks} active callbacks"
                    )
                self._callback_condition.wait(remaining)
        if not kernel32.VirtualFree(self.trampoline_address, 0, MEM_RELEASE):
            raise _winerror("VirtualFree(trampoline)")
        self.trampoline_address = 0
        self.original = None
        self.callback = None
        self.patched_bytes = b""

    def __enter__(self) -> Self:
        return self.install()

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.close()


def self_test() -> dict[str, object]:
    """Hook and restore a function allocated entirely by this module."""

    target = kernel32.VirtualAlloc(
        None, 0x1000, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE
    )
    if not target:
        raise _winerror("VirtualAlloc(self-test target)")
    signature = ctypes.WINFUNCTYPE(ctypes.c_uint32)
    try:
        ctypes.memmove(target, b"\xb8\x2a\x00\x00\x00\xc3" + b"\x90" * 26, 32)
        function = signature(target)
        before = int(function())

        def detour(original):
            return int(original()) + 1

        hook = AbsoluteHook(
            target, signature, detour, expected_prefix=b"\xb8\x2a\x00\x00\x00\xc3"
        )
        hook.install()
        during = int(function())
        hook.close()
        after = int(function())
        return {
            "before": before,
            "during": during,
            "after": after,
            "passed": (before, during, after) == (42, 43, 42),
            "stolen_length": hook.stolen_length,
        }
    finally:
        kernel32.VirtualFree(target, 0, MEM_RELEASE)

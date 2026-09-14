from __future__ import annotations

import ctypes

from search_probes.native_layout import (
    MAX_DYNAMIC_ARRAY_ITEMS,
    MAX_DYNAMIC_STRING_BYTES,
    MAX_USER_POINTER,
)


def read_memory(address: int, size: int) -> bytes:
    """Use the OS read boundary for unproved addresses instead of risking an AV."""

    if address < 0x10000 or not 0 < size <= 0x100000:
        raise ValueError(f"refusing memory read at 0x{address:X} with size {size}")
    buffer = (ctypes.c_ubyte * size)()
    bytes_read = ctypes.c_size_t()
    kernel32 = ctypes.windll.kernel32
    kernel32.GetCurrentProcess.restype = ctypes.c_void_p
    kernel32.ReadProcessMemory.argtypes = (
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
    )
    kernel32.ReadProcessMemory.restype = ctypes.c_int
    process = kernel32.GetCurrentProcess()
    succeeded = kernel32.ReadProcessMemory(
        process,
        ctypes.c_void_p(address),
        ctypes.byref(buffer),
        size,
        ctypes.byref(bytes_read),
    )
    if not succeeded or bytes_read.value != size:
        raise RuntimeError(
            f"could not safely read {size} bytes at 0x{address:X}: "
            f"read={bytes_read.value}, error={kernel32.GetLastError()}"
        )
    return bytes(buffer)


def read_uint(address: int, size: int) -> int:
    return int.from_bytes(read_memory(address, size), "little")


def validate_pointer(name: str, pointer: int) -> int:
    if not 0x10000 <= pointer <= MAX_USER_POINTER:
        raise RuntimeError(f"{name} pointer is implausible: 0x{pointer:X}")
    return pointer


def safe_fixed_string(address: int, size: int) -> str:
    return (
        read_memory(address, size)
        .split(b"\0", 1)[0]
        .decode("utf-8", errors="backslashreplace")
    )


def safe_dynamic_array_header(
    address: int, maximum: int, *, name: str
) -> dict[str, object]:
    raw = read_memory(address, 16)
    pointer = int.from_bytes(raw[:8], "little")
    size = int.from_bytes(raw[8:12], "little")
    allocated_from_data = bool(raw[12])
    if size > maximum:
        raise RuntimeError(f"dynamic array size {size} exceeds safe maximum {maximum}")
    if size:
        validate_pointer(name, pointer)
    return {
        "pointer": pointer,
        "size": size,
        "allocated_from_data": allocated_from_data,
    }


def write_memory(address: int, payload: bytes) -> None:
    """Write one already-validated owned record through the OS process boundary."""

    if address < 0x10000 or not payload or len(payload) > 0x100000:
        raise ValueError(f"refusing memory write at 0x{address:X}")
    written = ctypes.c_size_t()
    kernel32 = ctypes.windll.kernel32
    kernel32.GetCurrentProcess.restype = ctypes.c_void_p
    kernel32.WriteProcessMemory.argtypes = (
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
    )
    kernel32.WriteProcessMemory.restype = ctypes.c_int
    owned = (ctypes.c_ubyte * len(payload)).from_buffer_copy(payload)
    succeeded = kernel32.WriteProcessMemory(
        kernel32.GetCurrentProcess(),
        ctypes.c_void_p(address),
        ctypes.byref(owned),
        len(payload),
        ctypes.byref(written),
    )
    if not succeeded or written.value != len(payload):
        raise RuntimeError(
            f"could not write {len(payload)} bytes at 0x{address:X}: "
            f"written={written.value}, error={kernel32.GetLastError()}"
        )


def fixed_string(address: int, size: int) -> str:
    raw = ctypes.string_at(address, size).split(b"\0", 1)[0]
    return raw.decode("utf-8", "backslashreplace")


def dynamic_array_header(address: int) -> dict[str, object]:
    pointer = int(ctypes.c_void_p.from_address(address).value or 0)
    size = int(ctypes.c_uint32.from_address(address + 8).value)
    allocated_from_data = bool(ctypes.c_uint8.from_address(address + 12).value)
    if size > MAX_DYNAMIC_ARRAY_ITEMS:
        raise RuntimeError(f"dynamic array size {size} exceeds investigation bound")
    if size and not pointer:
        raise RuntimeError("non-empty dynamic array has a null pointer")
    return {
        "pointer": pointer,
        "size": size,
        "allocated_from_data": allocated_from_data,
    }


def dynamic_string(address: int) -> str:
    header = dynamic_array_header(address)
    size = int(header["size"])
    if not size:
        return ""
    if size > MAX_DYNAMIC_STRING_BYTES:
        raise RuntimeError(f"dynamic string size {size} exceeds investigation bound")
    raw = ctypes.string_at(int(header["pointer"]), size).split(b"\0", 1)[0]
    return raw.decode("utf-8", "backslashreplace")

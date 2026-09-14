from __future__ import annotations

import ctypes
import struct
from collections.abc import Callable

APPLICATION_SINGLETON = 0x146E81828
MISSION_MANAGER_OFFSET = 0x837B20
START_MISSION = 0x1409B13C0
IS_MISSION_ACTIVE = 0x1405685A0
MAX_MISSIONS = 4096


def target_route_active(events: list[dict], mission_id: bytes, address: int) -> bool:
    owned = [
        event
        for event in events
        if event.get("mission_context_id") == mission_id.decode("ascii")
    ]
    if len(owned) > 1:
        raise RuntimeError("NMS published multiple routes for the target mission")
    return bool(owned and int(owned[0]["address"], 16) == address)


class NativeMissions:
    def __init__(
        self, resolve: Callable[[int], int], read: Callable[[int, int], bytes]
    ):
        self._resolve = resolve
        self._read = read
        prefixes = {
            START_MISSION: "48 89 5C 24 10 48 89 74 24 18 55 57 41 56",
            IS_MISSION_ACTIVE: "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20",
        }
        for address, value in prefixes.items():
            expected = bytes.fromhex(value)
            if read(resolve(address), len(expected)) != expected:
                raise RuntimeError(
                    f"unsupported native mission function at {address:#x}"
                )
        self._start = ctypes.WINFUNCTYPE(
            ctypes.c_bool,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_bool,
            ctypes.c_bool,
        )(resolve(START_MISSION))
        self._active = ctypes.WINFUNCTYPE(
            ctypes.c_bool,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
        )(resolve(IS_MISSION_ACTIVE))

    def _manager(self) -> int:
        application = int.from_bytes(
            self._read(self._resolve(APPLICATION_SINGLETON), 8), "little"
        )
        if not 0x10000 <= application <= 0x00007FFFFFFFFFFF:
            raise RuntimeError("application singleton is unavailable")
        return application + MISSION_MANAGER_OFFSET

    @staticmethod
    def _identity(mission_id: bytes):
        if not mission_id or len(mission_id) >= 16 or b"\0" in mission_id:
            raise ValueError("mission identifier must contain 1–15 non-null bytes")
        return ctypes.create_string_buffer(mission_id, 16), (ctypes.c_ubyte * 16)()

    def active(self, mission_id: bytes) -> bool:
        identity, seed = self._identity(mission_id)
        return bool(
            self._active(
                self._manager(), ctypes.addressof(identity), ctypes.addressof(seed)
            )
        )

    def pending(self, mission_id: bytes) -> bool:
        manager = self._manager()
        for offset in (0x180, 0x190):
            capacity, count, pointer = struct.unpack(
                "<IIQ", self._read(manager + offset, 16)
            )
            if count > capacity or capacity > MAX_MISSIONS:
                raise RuntimeError(
                    "native mission request collection has an unsupported shape"
                )
            if count and not 0x10000 <= pointer <= 0x00007FFFFFFFFFFF:
                raise RuntimeError("native mission request collection is unavailable")
            for index in range(count):
                if (
                    self._read(pointer + index * 0x28, 16).split(b"\0", 1)[0]
                    == mission_id
                ):
                    return True
        return False

    def require_available(self, mission_id: bytes) -> None:
        if self.active(mission_id):
            raise RuntimeError(
                "A Search Probe Target is already active. Abandon it in the Log before choosing another target."
            )
        if self.pending(mission_id):
            raise RuntimeError(
                "A Search Probe Target is still starting. Return to gameplay and try again."
            )

    def start(self, mission_id: bytes) -> None:
        identity, seed = self._identity(mission_id)
        accepted = self._start(
            self._manager(),
            ctypes.addressof(identity),
            ctypes.addressof(seed),
            False,
            True,
        )
        if not accepted:
            raise RuntimeError("NMS declined the Search Probe Target request")

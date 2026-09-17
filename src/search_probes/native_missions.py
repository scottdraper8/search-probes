from __future__ import annotations

import ctypes
import struct
from collections.abc import Callable, Iterator

from search_probes.native_layout import (
    APPLICATION_MISSION_MANAGER_OFFSET,
    APPLICATION_SINGLETON,
    MAX_USER_POINTER,
)

MISSION_MANAGER_OFFSET = APPLICATION_MISSION_MANAGER_OFFSET
START_MISSION = 0x1409B1F10
SELECT_MISSION = 0x1409BA700
MAX_MISSIONS = 4096


class MissionSeed(ctypes.Structure):
    _fields_ = [("value", ctypes.c_uint64), ("valid", ctypes.c_bool)]


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
            START_MISSION: (
                "48 89 5C 24 10 48 89 74 24 18 55 57 41 56 48 8D 6C 24 C1 "
                "48 81 EC 00 01 00 00 8B 99 C4 01 00 00"
            ),
            SELECT_MISSION: (
                "48 8B C4 44 88 40 18 53 56 57 41 54 48 81 EC 58 01 00 00 "
                "45 33 E4 41 0F B6 F8 48 8B DA 48 8B F1"
            ),
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
        self._select = ctypes.WINFUNCTYPE(
            None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_bool
        )(resolve(SELECT_MISSION))

    def _manager(self) -> int:
        application = int.from_bytes(
            self._read(self._resolve(APPLICATION_SINGLETON), 8), "little"
        )
        if not 0x10000 <= application <= MAX_USER_POINTER:
            raise RuntimeError("application singleton is unavailable")
        return application + MISSION_MANAGER_OFFSET

    @staticmethod
    def _identity(mission_id: bytes):
        if not mission_id or len(mission_id) >= 16 or b"\0" in mission_id:
            raise ValueError("mission identifier must contain 1–15 non-null bytes")
        return ctypes.create_string_buffer(mission_id, 16), MissionSeed(0, True)

    def active(self, mission_id: bytes) -> bool:
        matches = self._active_instances(mission_id)
        if len(matches) > 1:
            raise RuntimeError("multiple active Search Probe Target instances exist")
        return bool(matches)

    def _mission_instances(self) -> Iterator[tuple[int, int]]:
        manager = self._manager()
        capacity, count, pointer = struct.unpack(
            "<IIQ", self._read(manager + 0x1C0, 16)
        )
        if count > capacity or capacity > MAX_MISSIONS:
            raise RuntimeError("native mission collection has an unsupported shape")
        if count and not 0x10000 <= pointer <= MAX_USER_POINTER:
            raise RuntimeError("native mission collection is unavailable")
        for index in range(count):
            mission = int.from_bytes(self._read(pointer + index * 8, 8), "little")
            if not 0x10000 <= mission <= MAX_USER_POINTER:
                raise RuntimeError("native mission instance is unavailable")
            yield index, mission

    def _active_instances(self, mission_id: bytes) -> list[tuple[int, int]]:
        identity, seed = self._identity(mission_id)
        expected_identity = bytes(identity)
        expected_seed = bytes(seed)[:9]
        matches = []
        for index, mission in self._mission_instances():
            if self._read(mission + 8, 16) != expected_identity:
                continue
            if self._read(mission + 0x100, 9) != expected_seed:
                continue
            stages = int.from_bytes(self._read(mission + 0x1C, 4), "little")
            progress = int.from_bytes(
                self._read(mission + 0x48, 4), "little", signed=True
            )
            if 0 <= progress < stages:
                matches.append((index, mission))
        return matches

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

    def select(self, mission_id: bytes) -> None:
        manager = self._manager()
        matches = self._active_instances(mission_id)
        if len(matches) != 1:
            raise RuntimeError("expected exactly one active Search Probe Target instance")
        index, mission = matches[0]
        self._select(manager, mission, False)
        selected = int.from_bytes(
            self._read(manager + 0x250, 4), "little", signed=True
        )
        if selected != index:
            raise RuntimeError("NMS did not select the Search Probe Target")

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

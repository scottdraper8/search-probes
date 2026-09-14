from __future__ import annotations

import ctypes

from search_probes import native_memory as memory
from search_probes import resident_search_presets as _PRESETS
from search_probes import resident_search_protocol as _PROTOCOL
from search_probes.native_layout import (
    EVENT_SOLAR_SYSTEM_LOCATION_OFFSET,
    EVENT_TABLE,
    EVENT_UAS_LIST_OFFSET,
    FORM_NAVIGATION_TARGET_EVENT_NAME,
    FORM_NAVIGATION_TARGET_MISSION_ID,
    LEGACY_FORM_NAVIGATION_ROUTES,
    LEGACY_GUIDE_ROUTE_PREFIXES,
    SCAN_EVENT_CONTEXT_SIZE,
    SOLAR_SYSTEM_LOCATION_FROM_LIST,
)


class Navigation:
    def __init__(self, engine, native, missions, emit):
        self.engine = engine
        self.native = native
        self.missions = missions
        self.emit = emit

    def active_scan_event_snapshot(self) -> list[dict[str, object]]:
        """Copy the routing fields of every active event for lifecycle diagnosis."""

        manager = self.engine.scan_event_manager()
        count = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        if count > 1024:
            raise RuntimeError(
                f"active scan-event count exceeds diagnostic bound: {count}"
            )
        records = int(ctypes.c_void_p.from_address(manager + 0xD8).value or 0)
        if count and records < 0x10000:
            raise RuntimeError("active scan-event array is null")
        result: list[dict[str, object]] = []
        for index in range(count):
            record = records + index * 0x70
            context = memory.read_memory(record + 0x40, 0x10) + memory.read_memory(
                record + 0x58, 0x08
            )
            result.append(
                {
                    "index": index,
                    "name": memory.safe_fixed_string(record + 0x10, 0x20),
                    "address": f"0x{memory.read_uint(record + 0x50, 8):016X}",
                    "table": memory.read_uint(record + 0x64, 4),
                    "mission_context_id": context[:0x10]
                    .split(b"\0", 1)[0]
                    .decode("ascii", errors="replace"),
                    "mission_context_seed": (
                        f"0x{int.from_bytes(context[0x10:0x18], 'little'):016X}"
                    ),
                }
            )
        return result

    def published_event_record(
        self,
        manager: int,
        index: int,
        *,
        expected_name: bytes,
        expected_table: int,
        expected_context: bytes,
        expected_address: int,
    ) -> dict[str, object]:
        """Validate the route-bearing fields copied into one new active scan-event record."""

        records = int(ctypes.c_void_p.from_address(manager + 0xD8).value or 0)
        if records < 0x10000:
            raise RuntimeError("active scan-event array is null after publication")
        record = records + index * 0x70
        name = memory.read_memory(record + 0x10, 0x20).split(b"\0", 1)[0]
        table = int(ctypes.c_int32.from_address(record + 0x64).value)
        address = int(ctypes.c_uint64.from_address(record + 0x50).value)
        context = memory.read_memory(record + 0x40, 0x10) + memory.read_memory(
            record + 0x58, 0x08
        )
        if name != expected_name:
            raise RuntimeError(f"published unexpected scan event: {name!r}")
        if table != expected_table:
            raise RuntimeError(
                f"published scan-event table changed unexpectedly: {table}"
            )
        if address != expected_address:
            raise RuntimeError(
                f"published scan-event address changed unexpectedly: 0x{address:016X}"
            )
        if context != expected_context:
            raise RuntimeError("published scan event lost its mission routing context")
        return {
            "active_event_index": index,
            "active_event_name": name.decode("ascii", errors="replace"),
            "mission_context_preserved": True,
            "mission_context_id": context[:0x10]
            .split(b"\0", 1)[0]
            .decode("ascii", errors="replace"),
            "mission_context_seed": f"0x{int.from_bytes(context[0x10:0x18], 'little'):016X}",
            "address": f"0x{address:016X}",
        }

    def locate_published_event_record(
        self,
        manager: int,
        *,
        expected_name: bytes,
        expected_table: int,
        expected_context: bytes,
        expected_address: int,
    ) -> dict[str, object]:
        """Find the exact active record after either an append or an in-place replacement."""

        count = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        records = int(ctypes.c_void_p.from_address(manager + 0xD8).value or 0)
        if count and records < 0x10000:
            raise RuntimeError("active scan-event array is null after publication")
        matches: list[int] = []
        for index in range(count):
            record = records + index * 0x70
            name = memory.read_memory(record + 0x10, 0x20).split(b"\0", 1)[0]
            table = int(ctypes.c_int32.from_address(record + 0x64).value)
            address = int(ctypes.c_uint64.from_address(record + 0x50).value)
            context = memory.read_memory(record + 0x40, 0x10) + memory.read_memory(
                record + 0x58, 0x08
            )
            if (
                name == expected_name
                and table == expected_table
                and address == expected_address
                and context == expected_context
            ):
                matches.append(index)
        if len(matches) != 1:
            raise RuntimeError(
                "expected exactly one route-bearing active scan event after publication, "
                f"found {len(matches)}"
            )
        return self.published_event_record(
            manager,
            matches[0],
            expected_name=expected_name,
            expected_table=expected_table,
            expected_context=expected_context,
            expected_address=expected_address,
        )

    def matching_active_route_indices(
        self,
        manager: int,
        *,
        expected_name: bytes,
        expected_context: bytes,
    ) -> list[int]:
        """Find active routes owned by one exact scan-event and mission context."""

        if not expected_name or len(expected_name) >= 0x20:
            raise ValueError("active-route event name must fit TkID<256>")
        if len(expected_context) != SCAN_EVENT_CONTEXT_SIZE:
            raise ValueError("active-route mission context has the wrong size")
        count = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        records = int(ctypes.c_void_p.from_address(manager + 0xD8).value or 0)
        if count and records < 0x10000:
            raise RuntimeError("active scan-event array is null")
        matches: list[int] = []
        for index in range(count):
            record = records + index * 0x70
            name = memory.read_memory(record + 0x10, 0x20).split(b"\0", 1)[0]
            context = memory.read_memory(record + 0x40, 0x10) + memory.read_memory(
                record + 0x58, 0x08
            )
            if name == expected_name and context == expected_context:
                matches.append(index)
        return matches

    def remove_active_route(
        self,
        manager: int,
        *,
        expected_name: bytes,
        expected_context: bytes,
    ) -> dict[str, int]:
        """Remove every prior instance through NMS's own scan-event teardown path.

        cGcScanEventManager's low-level publication primitive always appends. The normal mission
        path implements ReplaceEventIfAlreadyActive above that primitive by tearing down matching
        runtime markers and saved records first. Direct navigation must reproduce that ownership
        transition rather than mutating or erasing the manager's vector itself.
        """

        matches = self.matching_active_route_indices(
            manager,
            expected_name=expected_name,
            expected_context=expected_context,
        )
        before = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        if matches:
            name = ctypes.create_string_buffer(0x20)
            name.value = expected_name
            context = (ctypes.c_ubyte * SCAN_EVENT_CONTEXT_SIZE).from_buffer_copy(
                expected_context
            )
            removed = bool(
                self.native.remove_scan_event(
                    manager,
                    ctypes.addressof(name),
                    ctypes.addressof(context),
                )
            )
            if not removed:
                raise RuntimeError("NMS rejected scan-event teardown")
        after = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        remaining = self.matching_active_route_indices(
            manager,
            expected_name=expected_name,
            expected_context=expected_context,
        )
        if remaining:
            raise RuntimeError(
                "NMS scan-event teardown left matching active routes at indices "
                + ", ".join(str(index) for index in remaining)
            )
        if after != before - len(matches):
            raise RuntimeError(
                "scan-event teardown count changed unexpectedly: "
                f"{before} -> {after}, expected removal of {len(matches)}"
            )
        return {
            "active_scan_events_before_cleanup": before,
            "active_scan_events_after_cleanup": after,
            "removed_active_routes": len(matches),
        }

    @staticmethod
    def is_legacy_owned_route(event_name: bytes, mission_id: bytes) -> bool:
        if (event_name, mission_id) in LEGACY_FORM_NAVIGATION_ROUTES:
            return True
        for event_prefix, mission_prefix in LEGACY_GUIDE_ROUTE_PREFIXES:
            if not event_name.startswith(event_prefix):
                continue
            suffix = event_name[len(event_prefix) :]
            return (
                len(suffix) == 2
                and suffix.isdigit()
                and int(suffix) < _PRESETS.MAX_PRESETS
                and mission_id == mission_prefix + suffix
            )
        return False

    def remove_legacy_owned_routes(self, manager: int) -> int:
        """Remove saved routes whose mission progress contract is now obsolete."""

        count = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        records = int(ctypes.c_void_p.from_address(manager + 0xD8).value or 0)
        if count and records < 0x10000:
            raise RuntimeError(
                "active scan-event array is null while removing legacy routes"
            )
        routes: dict[tuple[bytes, bytes], None] = {}
        for index in range(count):
            record = records + index * 0x70
            event_name = memory.read_memory(record + 0x10, 0x20).split(b"\0", 1)[0]
            context = memory.read_memory(record + 0x40, 0x10) + memory.read_memory(
                record + 0x58, 0x08
            )
            mission_id = context[:0x10].split(b"\0", 1)[0]
            if self.is_legacy_owned_route(event_name, mission_id):
                routes[(event_name, context)] = None
        removed = 0
        for event_name, context in routes:
            result = self.remove_active_route(
                manager,
                expected_name=event_name,
                expected_context=context,
            )
            removed += int(result["removed_active_routes"])
        return removed

    def scan_event_data(self, event_name: bytes) -> int:
        """Resolve one product-owned event definition through NMS's current table."""

        if not event_name or len(event_name) >= 0x20:
            raise ValueError(
                "scan-event name does not fit NMS's fixed event identifier"
            )
        manager = self.engine.scan_event_manager()
        name = ctypes.create_string_buffer(0x20)
        name.value = event_name
        event_data = int(
            self.native.lookup_event(manager, ctypes.addressof(name), EVENT_TABLE) or 0
        )
        if event_data < 0x10000:
            raise RuntimeError(f"NMS could not resolve scan event {event_name!r}")
        actual = memory.read_memory(event_data + 0x228, 0x20).split(b"\0", 1)[0]
        if actual != event_name:
            raise RuntimeError(
                f"scan-event lookup returned {actual!r} for requested {event_name!r}"
            )
        return event_data

    def set_from_list_target(
        self, event_name: bytes, address: int
    ) -> dict[str, object]:
        """Replace the one owned FromList placeholder with an exact universe address."""

        if _PROTOCOL.universe_address_planet_index(address) == 0:
            raise ValueError(
                "native FromList navigation requires an exact planet address"
            )
        event_data = self.scan_event_data(event_name)
        location = memory.read_uint(event_data + EVENT_SOLAR_SYSTEM_LOCATION_OFFSET, 4)
        if location != SOLAR_SYSTEM_LOCATION_FROM_LIST:
            raise RuntimeError(
                f"{event_name.decode('ascii')} is not a FromList scan event: {location}"
            )
        header = memory.read_memory(event_data + EVENT_UAS_LIST_OFFSET, 0x10)
        entries = int.from_bytes(header[:8], "little")
        count = int.from_bytes(header[8:12], "little")
        if entries < 0x10000 or count != 1:
            raise RuntimeError(
                f"{event_name.decode('ascii')} must own exactly one UA list entry; "
                f"pointer=0x{entries:X}, count={count}"
            )
        string_pointer = int.from_bytes(memory.read_memory(entries, 8), "little")
        if string_pointer < 0x10000:
            raise RuntimeError("FromList entry has no writable string buffer")
        previous = memory.read_memory(string_pointer, 17)
        previous_text = previous.split(b"\0", 1)[0]
        if len(previous_text) != 16 or any(
            character not in b"0123456789abcdefABCDEF" for character in previous_text
        ):
            raise RuntimeError(
                f"FromList entry is not the owned 16-digit address buffer: {previous_text!r}"
            )
        encoded = f"{address:016X}".encode("ascii") + b"\0"
        memory.write_memory(string_pointer, encoded)
        verified = memory.read_memory(string_pointer, len(encoded))
        if verified != encoded:
            raise RuntimeError("FromList address write did not verify")
        return {
            "event": event_name.decode("ascii"),
            "event_data": f"0x{event_data:016X}",
            "list_entries": f"0x{entries:016X}",
            "string_buffer": f"0x{string_pointer:016X}",
            "previous": previous_text.decode("ascii"),
            "target": encoded[:-1].decode("ascii"),
        }

    def publish_navigation_ready(
        self,
        *,
        ready_event_name: bytes,
        event_context: bytes,
    ) -> dict[str, object]:
        """Advance the mission gate; the following target publication remains native-owned."""

        if len(event_context) != SCAN_EVENT_CONTEXT_SIZE:
            raise ValueError("ready signal has the wrong mission-context size")
        event_data = self.scan_event_data(ready_event_name)
        manager = self.engine.scan_event_manager()
        cleanup = self.remove_active_route(
            manager,
            expected_name=ready_event_name,
            expected_context=event_context,
        )
        context = (ctypes.c_ubyte * SCAN_EVENT_CONTEXT_SIZE).from_buffer_copy(
            event_context
        )
        current = self.engine.current_universe_address()
        before = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        self.native.publish_scan_event(
            manager,
            event_data,
            EVENT_TABLE,
            ctypes.addressof(context),
            None,
            current,
        )
        after = int(ctypes.c_uint32.from_address(manager + 0xD4).value)
        if after != before + 1:
            raise RuntimeError(
                f"ready-event publication count changed unexpectedly: {before} -> {after}"
            )
        publication = self.locate_published_event_record(
            manager,
            expected_name=ready_event_name,
            expected_table=EVENT_TABLE,
            expected_context=event_context,
            expected_address=current,
        )
        return {
            "cleanup": cleanup,
            "publication": publication,
            "current_address": f"0x{current:016X}",
        }

    def arm_native_navigation(
        self,
        *,
        target_event_name: bytes,
        ready_event_name: bytes,
        event_context: bytes,
        address: int,
    ) -> dict[str, object]:
        """Supply a target and release the mission into NMS's native StartScanEvent stage."""

        mission_id = event_context[:0x10].split(b"\0", 1)[0]
        target = self.set_from_list_target(target_event_name, address)
        ready = self.publish_navigation_ready(
            ready_event_name=ready_event_name,
            event_context=event_context,
        )
        self.emit(
            "native_navigation_armed",
            mission_context_id=mission_id.decode("ascii", errors="replace"),
            target=target,
            ready=ready,
        )
        return {"target": target, "ready": ready}

    def start_form_navigation(self, address: int) -> dict[str, object]:
        """Patch the target definition, then start its ordinary mission through NMS."""

        if self.missions is None:
            raise RuntimeError("native mission-start function is unavailable")
        self.missions.require_available(FORM_NAVIGATION_TARGET_MISSION_ID)
        target = self.set_from_list_target(FORM_NAVIGATION_TARGET_EVENT_NAME, address)
        self.missions.start(FORM_NAVIGATION_TARGET_MISSION_ID)
        result = {
            "mission_id": FORM_NAVIGATION_TARGET_MISSION_ID.decode("ascii"),
            "seed": "0x0000000000000000",
            "seed_valid": True,
            "restart": False,
            "selected": True,
            "target": target,
        }
        self.emit("form_navigation_requested", **result)
        return result

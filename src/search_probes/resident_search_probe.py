"""Compose Search Probes services and keep native work on the gameplay thread."""

from __future__ import annotations

import ctypes
import json
import logging
import queue
import sys
import threading
import time
import uuid
from pathlib import Path

from pymhf import Mod
from pymhf.main import run_module

from search_probes import native_memory as memory
from search_probes import resident_search_overlay as _OVERLAY
from search_probes import resident_search_presets as _PRESETS
from search_probes import resident_search_protocol as _PROTOCOL
from search_probes.absolute_hook import AbsoluteHook, self_test
from search_probes.guide_catalog import GuideCatalog
from search_probes.native_calls import NativeCalls
from search_probes.native_layout import (
    EVENT_REPLACE_IF_ACTIVE_OFFSET,
    EVENT_TABLE,
    FORM_NAVIGATION_TARGET_MISSION_ID,
    PLANET_DATA_DESTRUCTOR,
    PLAYER_COMMUNICATOR_UPDATE,
    POLL_SECONDS,
    PREFERRED_IMAGE_BASE,
    PUBLISH_SCAN_EVENT,
    REMOVE_SCAN_EVENT,
    SCAN_EVENT_CONTEXT_SIZE,
    WARMUP_TICKS,
    ResolverResult,
    SpatialOutput,
)
from search_probes.native_missions import NativeMissions, target_route_active
from search_probes.navigation import Navigation
from search_probes.search_engine import SearchEngine
from search_probes.search_scheduler import SearchScheduler

LOGGER = logging.getLogger("squinch.resident_search")


def _atomic_json(path: Path, payload: object) -> None:
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    temporary.write_text(
        json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


class ResidentSearchProbe(Mod):
    __author__ = "scottdraper8"
    __description__ = "Resident bounded native-search command bridge"
    __version__ = "1"

    def __init__(self) -> None:
        self._root = Path(__file__).resolve().parents[1] / ".resident-search"
        self._hook: AbsoluteHook | None = None
        self._planet_data_hook: AbsoluteHook | None = None
        self._scan_event_hook: AbsoluteHook | None = None
        self._remove_scan_event_hook: AbsoluteHook | None = None
        self._hook_lock = threading.Lock()
        self._commands: queue.Queue[dict[str, object]] = queue.Queue(
            maxsize=_PROTOCOL.MAX_QUEUE_SIZE
        )
        self._responses: queue.SimpleQueue[dict[str, object]] = queue.SimpleQueue()
        self._internal_writes: queue.SimpleQueue[tuple[Path, dict[str, object]]] = (
            queue.SimpleQueue()
        )
        self._cancelled: set[str] = set()
        self._cancel_lock = threading.Lock()
        # DearPyGUI reads result files from a second Python thread.  Serialize its reads with
        # progress overwrites; repeated atomic replacement of the same Wine Z:-drive path can
        # otherwise hang inside MoveFileEx even when prior replacements succeeded.
        self._result_io_lock = threading.Lock()
        self._generation = 0
        self._stop_worker = threading.Event()
        self._close_requested = threading.Event()
        self._ticks = 0
        self._ready = False
        self._missions = None
        self._pending_navigation = None
        self._scan_event_observations: list[dict[str, object]] = []
        self._session_id = f"{time.time_ns()}-{uuid.uuid4().hex}"
        self._guide_dispatches: dict[str, dict[str, object]] = {}
        self._guide_status_lock = threading.Lock()
        self._guide_status_revision = 0
        self._guide_status: dict[str, object] = {
            "revision": 0,
            "state": "initializing",
            "message": "Preparing Guide presets...",
        }
        self._overlay_enabled = _PROTOCOL.read_launch_overlay_enabled(self._root)
        self._preset_store = _PRESETS.PresetStore(
            self._root / _PRESETS.PRESET_FILE,
            defaults=_PRESETS.DEFAULT_PRESETS,
            normalize_criteria=_PROTOCOL.normalize_criteria,
            default_candidate_limit=5000,
            max_candidate_limit=_PROTOCOL.MAX_PROVEN_UI_CANDIDATES,
            default_result_limit=5,
            max_result_limit=_PROTOCOL.MAX_RESULTS,
        )
        self._guide = GuideCatalog(
            self._preset_store,
            self._runtime_address,
            self._set_guide_status,
            self._emit,
        )
        self._session = self._root / "sessions" / self._session_id
        self._command_dir = self._session / "commands"
        self._result_dir = self._session / "results"
        self._seen: set[str] = set()
        super().__init__()
        if ctypes.sizeof(ResolverResult) != 16 or ResolverResult.address.offset != 8:
            raise RuntimeError("unexpected resolver result ABI layout")
        if ctypes.sizeof(SpatialOutput) != 32 or SpatialOutput.count.offset != 24:
            raise RuntimeError("unexpected spatial output ABI layout")
        control = self_test()
        if not control["passed"]:
            raise RuntimeError(f"absolute-hook self-test failed: {control!r}")
        self._prepare_mailbox()
        self._install()
        self._ready = True
        self._write_state("ready", control=control)
        self._emit("probe_ready", control=control)
        self._worker = threading.Thread(
            target=self._mailbox_worker,
            name="SQNResidentSearchMailbox",
            daemon=True,
        )
        self._worker.start()
        self._overlay = None
        if self._overlay_enabled:
            try:
                self._overlay = _OVERLAY.ResidentSearchOverlay(
                    _OVERLAY.OverlayClient(
                        self._command_dir,
                        self._result_dir,
                        self._result_io_lock,
                        lambda: self._generation,
                        self.guide_status_snapshot,
                        self._preset_store,
                        self._emit,
                    )
                )
                self._overlay.start()
            except Exception:
                self._stop_worker.set()
                self._worker.join(timeout=1.0)
                self._close_hook("overlay_start_failure")
                self._write_state("failed", error="overlay failed to start")
                raise

    @staticmethod
    def _runtime_address(preferred: int) -> int:
        kernel32 = ctypes.windll.kernel32
        kernel32.GetModuleHandleW.argtypes = [ctypes.c_wchar_p]
        kernel32.GetModuleHandleW.restype = ctypes.c_void_p
        loaded_base = int(kernel32.GetModuleHandleW(None) or 0)
        if not loaded_base:
            raise RuntimeError("GetModuleHandleW(NULL) returned no executable base")
        return loaded_base + preferred - PREFERRED_IMAGE_BASE

    def guide_status_snapshot(self) -> dict[str, object]:
        """Return detached Guide-search status for the owned UI thread."""

        with self._guide_status_lock:
            return dict(self._guide_status)

    def _set_guide_status(self, state: str, message: str, **payload: object) -> None:
        with self._guide_status_lock:
            self._guide_status_revision += 1
            self._guide_status = {
                "revision": self._guide_status_revision,
                "state": state,
                "message": message,
                **payload,
            }

    def _prepare_mailbox(self) -> None:
        self._command_dir.mkdir(parents=True, exist_ok=False)
        self._result_dir.mkdir(parents=True, exist_ok=False)
        _atomic_json(
            self._root / "current.json",
            {
                "protocol": _PROTOCOL.PROTOCOL_VERSION,
                "session_id": self._session_id,
                "session_path": str(self._session),
            },
        )

    def _install(self) -> None:
        self._native = NativeCalls(self._runtime_address)
        self._engine = SearchEngine(self._native, self._runtime_address)
        self._scheduler = SearchScheduler(
            self._engine,
            self._commands,
            self._respond,
            self._respond_progress,
            self._complete_guide_search,
            self._emit,
        )
        update_signature = ctypes.WINFUNCTYPE(None, ctypes.c_void_p, ctypes.c_float)
        publish_signature = type(self._native.publish_scan_event)
        remove_scan_event_signature = type(self._native.remove_scan_event)
        query_lifecycle_signature = type(self._native.destroy_solar_query)
        self._missions = NativeMissions(self._runtime_address, memory.read_memory)
        self._navigation = Navigation(
            self._engine, self._native, self._missions, self._emit
        )

        def remove_scan_event_detour(original, manager, event_name, context):
            name = ""
            context_bytes = b""
            try:
                name = (
                    memory.safe_fixed_string(int(event_name or 0), 0x20)
                    if int(event_name or 0) >= 0x10000
                    else ""
                )
                context_pointer = int(context or 0)
                context_bytes = (
                    memory.read_memory(context_pointer, SCAN_EVENT_CONTEXT_SIZE)
                    if context_pointer >= 0x10000
                    else b""
                )
            except Exception as exc:  # noqa: BLE001 - never unwind through NMS
                self._emit(
                    "scan_event_remove_observation_error",
                    phase="capture",
                    error=f"{type(exc).__name__}: {exc}",
                )
            result = bool(original(manager, event_name, context))
            try:
                self._scan_event_observations.append(
                    {
                        "operation": "remove",
                        "name": name,
                        "manager": f"0x{int(manager or 0):016X}",
                        "context_present": bool(context),
                        "mission_context_id": (
                            context_bytes[:0x10]
                            .split(b"\0", 1)[0]
                            .decode("ascii", errors="replace")
                            if context_bytes
                            else ""
                        ),
                        "mission_context_seed": (
                            f"0x{int.from_bytes(context_bytes[0x10:0x18], 'little'):016X}"
                            if context_bytes
                            else "0x0000000000000000"
                        ),
                        "removed": result,
                        "thread_id": int(ctypes.windll.kernel32.GetCurrentThreadId()),
                        "monotonic_ns": time.monotonic_ns(),
                    }
                )
                del self._scan_event_observations[:-64]
            except Exception as exc:  # noqa: BLE001 - never unwind through NMS
                self._emit(
                    "scan_event_remove_observation_error",
                    phase="record",
                    error=f"{type(exc).__name__}: {exc}",
                )
            return result

        self._remove_scan_event_hook = AbsoluteHook(
            self._runtime_address(REMOVE_SCAN_EVENT),
            remove_scan_event_signature,
            remove_scan_event_detour,
            # _install's executable preflight validates the longer 17-byte signature. The hook
            # itself compares only its 14-byte whole-instruction patch window.
            expected_prefix=bytes.fromhex("48 83 EC 58 48 89 5C 24 60 48 89 6C 24 68"),
        ).install()

        def publish_scan_event_detour(
            original, manager, event_data, event_table, context, callback, address
        ):
            name = ""
            try:
                pointer = int(event_data or 0)
                context_pointer = int(context or 0)
                context_bytes = (
                    memory.read_memory(context_pointer, SCAN_EVENT_CONTEXT_SIZE)
                    if context_pointer >= 0x10000
                    else b""
                )
                name = (
                    memory.safe_fixed_string(pointer + 0x228, 0x20)
                    if pointer >= 0x10000
                    else ""
                )
                self._scan_event_observations.append(
                    {
                        "operation": "publish",
                        "name": name,
                        "event_table": int(event_table),
                        "address": f"0x{int(address):016X}",
                        "manager": f"0x{int(manager or 0):016X}",
                        "context_present": bool(context),
                        "mission_context_id": (
                            context_bytes[:0x10]
                            .split(b"\0", 1)[0]
                            .decode("ascii", errors="replace")
                            if context_bytes
                            else ""
                        ),
                        "mission_context_seed": (
                            f"0x{int.from_bytes(context_bytes[0x10:0x18], 'little'):016X}"
                            if context_bytes
                            else "0x0000000000000000"
                        ),
                        "callback_present": bool(callback),
                        "thread_id": int(ctypes.windll.kernel32.GetCurrentThreadId()),
                        "monotonic_ns": time.monotonic_ns(),
                    }
                )
                del self._scan_event_observations[:-64]
            except Exception as exc:  # noqa: BLE001 - never unwind through NMS
                self._emit(
                    "scan_event_observation_error",
                    error=f"{type(exc).__name__}: {exc}",
                )
            slot = _PRESETS.guide_event_slot(name)
            if slot is not None and int(event_table) == EVENT_TABLE:
                try:
                    self._start_guide_search(
                        slot,
                        int(event_data or 0),
                        int(event_table),
                        int(context or 0),
                    )
                except Exception as exc:  # noqa: BLE001 - never unwind through NMS
                    self._set_guide_status(
                        "error",
                        f"Could not start Guide preset: {type(exc).__name__}: {exc}",
                        slot=slot,
                    )
                    self._emit(
                        "guide_search_dispatch_error",
                        slot=slot,
                        error=f"{type(exc).__name__}: {exc}",
                    )
                return
            original(manager, event_data, event_table, context, callback, address)

        try:
            self._scan_event_hook = AbsoluteHook(
                self._runtime_address(PUBLISH_SCAN_EVENT),
                publish_signature,
                publish_scan_event_detour,
                expected_prefix=bytes.fromhex(
                    "48 8B C4 55 48 8D A8 28 FF FF FF 48 81 EC D0 01 00 00"
                ),
            ).install()
        except Exception:
            self._remove_scan_event_hook.close()
            self._remove_scan_event_hook = None
            raise

        def planet_data_destructor_detour(original, pointer):
            self._engine.capture_planet(pointer)
            original(pointer)

        try:
            self._planet_data_hook = AbsoluteHook(
                self._runtime_address(PLANET_DATA_DESTRUCTOR),
                query_lifecycle_signature,
                planet_data_destructor_detour,
                expected_prefix=bytes.fromhex(
                    "48 89 5C 24 08 57 48 83 EC 20 48 8B 91 20 34 00 00"
                ),
            ).install()
        except Exception:
            self._scan_event_hook.close()
            self._scan_event_hook = None
            if self._remove_scan_event_hook is not None:
                self._remove_scan_event_hook.close()
                self._remove_scan_event_hook = None
            raise

        def update_detour(original, communicator, timestep):
            original(communicator, timestep)
            self._ticks += 1
            if self._ticks < WARMUP_TICKS or self._close_requested.is_set():
                return
            self._check_navigation()
            try:
                self._guide.refresh_guide_mirror_if_pending()
            except Exception as exc:  # noqa: BLE001 - never unwind through NMS
                self._set_guide_status(
                    "error",
                    f"Could not update Guide presets: {type(exc).__name__}: {exc}",
                )
                self._emit(
                    "guide_mirror_error",
                    error=f"{type(exc).__name__}: {exc}",
                )
            try:
                command = self._commands.get_nowait()
            except queue.Empty:
                return
            try:
                self._execute(command)
            except Exception as exc:  # noqa: BLE001 - never unwind through an NMS callback
                self._respond(
                    command,
                    "error",
                    error=f"{type(exc).__name__}: {exc}",
                )
                self._fail_guide_command(command, f"{type(exc).__name__}: {exc}")

        try:
            self._hook = AbsoluteHook(
                self._runtime_address(PLAYER_COMMUNICATOR_UPDATE),
                update_signature,
                update_detour,
                expected_prefix=bytes.fromhex(
                    "F3 0F 11 4C 24 10 4C 8B DC 55 56 49 8D AB"
                ),
            ).install()
        except Exception:
            if self._planet_data_hook is not None:
                self._planet_data_hook.close()
                self._planet_data_hook = None
            if self._scan_event_hook is not None:
                self._scan_event_hook.close()
                self._scan_event_hook = None
            if self._remove_scan_event_hook is not None:
                self._remove_scan_event_hook.close()
                self._remove_scan_event_hook = None
            raise

    def _start_guide_search(
        self,
        slot: int,
        event_data: int,
        event_table: int,
        event_context: int,
    ) -> None:
        """Convert one precompiled Guide event into an owned, time-sliced preset search."""

        if event_data < 0x10000 or event_table != EVENT_TABLE:
            raise ValueError(
                "Guide dispatch did not carry a valid table-3 event record"
            )
        if event_context < 0x10000:
            raise ValueError("Guide dispatch did not carry a valid mission context")
        active_for_slot = next(
            (
                dispatch
                for dispatch in self._guide_dispatches.values()
                if int(dispatch["slot"]) == slot
            ),
            None,
        )
        if active_for_slot is not None:
            active_name = str(active_for_slot["name"])
            self._set_guide_status(
                "busy",
                f"That Guide search is already running: {active_name}",
                slot=slot,
            )
            return
        if len(self._guide_dispatches) >= _PROTOCOL.MAX_ACTIVE_SEARCHES:
            raise RuntimeError(
                f"at most {_PROTOCOL.MAX_ACTIVE_SEARCHES} Guide searches may run at once"
            )
        records = self._guide.records
        if slot >= len(records):
            raise RuntimeError(f"Guide slot {slot} has no launch-snapshot preset")
        record = records[slot]
        criteria = record["criteria"]
        if not isinstance(criteria, dict):
            raise TypeError("stored preset criteria are not detached JSON data")
        command_id = f"guide-{time.time_ns()}-{slot:02d}"
        generation = int(self._generation) + 1
        command = _PROTOCOL.normalize_command(
            {
                "protocol": _PROTOCOL.PROTOCOL_VERSION,
                "id": command_id,
                "generation": generation,
                "action": "search",
                "name": str(record["name"]),
                "criteria": criteria,
                "candidate_limit": int(record["candidate_limit"]),
                "result_limit": int(record["result_limit"]),
            }
        )
        command["_guide_slot"] = slot
        owned_event = (ctypes.c_ubyte * _PROTOCOL.EVENT_SIZE).from_buffer_copy(
            memory.read_memory(event_data, _PROTOCOL.EVENT_SIZE)
        )
        owned_context = (ctypes.c_ubyte * SCAN_EVENT_CONTEXT_SIZE).from_buffer_copy(
            memory.read_memory(event_context, SCAN_EVENT_CONTEXT_SIZE)
        )
        owned_event[EVENT_REPLACE_IF_ACTIVE_OFFSET] = 1
        self._guide_dispatches[command_id] = {
            "id": command_id,
            "slot": slot,
            "name": str(record["name"]),
            "event": owned_event,
            "event_context": owned_context,
            "event_table": event_table,
        }
        self._generation = generation
        try:
            self._commands.put_nowait(command)
        except queue.Full:
            self._guide_dispatches.pop(command_id, None)
            raise RuntimeError("command queue is full") from None
        self._set_guide_status(
            "searching",
            f"Searching preset: {record['name']}",
            command_id=command_id,
            slot=slot,
            candidates_checked=0,
            candidate_count=int(record["candidate_limit"]),
            matches_found=0,
        )
        self._emit(
            "guide_search_started",
            command_id=command_id,
            slot=slot,
            preset_name=record["name"],
        )

    def _fail_guide_command(self, command: dict[str, object], error: str) -> None:
        dispatch = self._guide_dispatches.pop(str(command.get("id")), None)
        if dispatch is None:
            return
        self._set_guide_status(
            "error",
            f"Guide search failed: {error}",
            command_id=command.get("id"),
            slot=dispatch["slot"],
        )

    def _complete_guide_search(
        self, command: dict[str, object], matches: list[object]
    ) -> dict[str, object]:
        dispatch = self._guide_dispatches.get(str(command["id"]))
        if dispatch is None:
            raise RuntimeError("Guide search completion has no matching dispatch")
        mission_id = _PRESETS.guide_slot_mission(int(dispatch["slot"])).encode("ascii")
        if not self._missions.active(mission_id):
            raise RuntimeError(
                "The Guide probe was abandoned before its search completed"
            )
        if not matches:
            self._guide_dispatches.pop(str(command["id"]), None)
            self._set_guide_status(
                "no_match",
                f"No exact match for {dispatch['name']}. Abandon this probe in the Log.",
                command_id=command["id"],
                slot=dispatch["slot"],
            )
            return {"guide_navigation_published": False}
        match = matches[0]
        if not isinstance(match, dict):
            raise TypeError("Guide search produced a non-object match")
        address = int(str(match.get("navigation_address", match["address"])), 0)
        self._engine.require_current_reality(address, operation="Guide search")
        manager = self._engine.scan_event_manager()
        legacy_routes_removed = self._navigation.remove_legacy_owned_routes(manager)
        owned_context = dispatch["event_context"]
        slot = int(dispatch["slot"])
        native = self._navigation.arm_native_navigation(
            target_event_name=_PRESETS.guide_slot_target_event(slot).encode("ascii"),
            ready_event_name=_PRESETS.guide_slot_ready_event(slot).encode("ascii"),
            event_context=bytes(owned_context),
            address=address,
        )
        self._guide_dispatches.pop(str(command["id"]), None)
        self._set_guide_status(
            "completed",
            f"Native navigation armed for preset: {dispatch['name']}",
            command_id=command["id"],
            slot=slot,
            address=f"0x{address:016X}",
            legacy_routes_removed=legacy_routes_removed,
            native=native,
        )
        self._emit(
            "guide_native_navigation_armed",
            command_id=command["id"],
            slot=slot,
            address=f"0x{address:016X}",
        )
        return {
            "guide_navigation_published": False,
            "guide_native_navigation_armed": True,
            "guide_navigation_address": f"0x{address:016X}",
            "guide_slot": slot,
            "guide_legacy_routes_removed": legacy_routes_removed,
            "guide_native_navigation": native,
        }

    def _check_navigation(self) -> None:
        pending = self._pending_navigation
        if pending is None:
            return
        command, native, started_tick = pending
        if (self._ticks - started_tick) % 15:
            return
        try:
            active = self._missions.active(FORM_NAVIGATION_TARGET_MISSION_ID)
            routed = active and target_route_active(
                self._navigation.active_scan_event_snapshot(),
                FORM_NAVIGATION_TARGET_MISSION_ID,
                int(command["address"]),
            )
            if not routed:
                if self._ticks - started_tick < 600:
                    return
                raise RuntimeError(
                    "NMS did not publish the target route. Return to gameplay and retry."
                )
            self._pending_navigation = None
            self._emit("form_navigation_active", **native)
            self._respond(
                command,
                "completed",
                address=f"0x{int(command['address']):016X}",
                native_navigation_armed=True,
                native=native,
            )
        except Exception as exc:  # noqa: BLE001 - never unwind through the game callback
            self._pending_navigation = None
            self._respond(command, "error", error=f"{type(exc).__name__}: {exc}")

    def _execute(self, command: dict[str, object]) -> None:
        command_id = str(command["id"])
        if (
            int(command["generation"]) < self._generation
            and "_guide_slot" not in command
        ):
            self._scheduler.searches.pop(command_id, None)
            self._scheduler.surveys.pop(command_id, None)
            self._respond(command, "stale")
            return
        with self._cancel_lock:
            cancelled = command_id in self._cancelled
        if cancelled:
            self._scheduler.searches.pop(command_id, None)
            self._scheduler.surveys.pop(command_id, None)
            self._respond(command, "cancelled")
            self._fail_guide_command(command, "cancelled")
            return
        action = command["action"]
        if action == "shutdown":
            # This command runs inside the gameplay-thread update callback. Restore both entry
            # points here, so no worker can suspend the gameplay thread while it is still inside
            # the callback being torn down. Callback/trampoline storage is retained until the
            # worker observes that this invocation has returned.
            self._guide.restore_wiki_probe()
            if self._remove_scan_event_hook is not None:
                self._remove_scan_event_hook.restore()
            if self._scan_event_hook is not None:
                self._scan_event_hook.restore()
            if self._planet_data_hook is not None:
                self._planet_data_hook.restore()
            if self._hook is not None:
                self._hook.restore()
            self._respond(command, "accepted", message="hook entry points restored")
            self._close_requested.set()
            return
        if action == "enumerate":
            started = time.perf_counter_ns()
            candidates = self._engine.enumerate_candidates(
                int(command["candidate_limit"])
            )
            self._respond(
                command,
                "completed",
                candidates=candidates,
                candidate_count=len(candidates),
                metadata=self._engine.last_enumeration,
                elapsed_ns=time.perf_counter_ns() - started,
            )
            return
        if action == "wiki_snapshot":
            started = time.perf_counter_ns()
            self._respond(
                command,
                "completed",
                snapshot=self._guide.wiki_snapshot(),
                elapsed_ns=time.perf_counter_ns() - started,
            )
            return
        if action == "wiki_mirror_probe":
            started = time.perf_counter_ns()
            self._respond(
                command,
                "completed",
                probe=self._guide.wiki_mirror_probe(
                    int(command["topic_count"]),
                    str(command["label_prefix"]),
                ),
                elapsed_ns=time.perf_counter_ns() - started,
            )
            return
        if action == "scan_event_observations":
            self._respond(
                command,
                "completed",
                observations=list(self._scan_event_observations),
                active_events=self._navigation.active_scan_event_snapshot(),
            )
            return
        if action == "snapshot":
            started = time.perf_counter_ns()
            address = int(command["address"])
            snapshot = self._engine.system_snapshot(
                address,
                include_colours=True,
                include_weather=True,
                include_metadata=True,
                include_spawn_flags=True,
                include_spawns=True,
            )
            self._respond(
                command,
                "completed",
                address=f"0x{address:016X}",
                snapshot=snapshot,
                elapsed_ns=time.perf_counter_ns() - started,
            )
            return
        if action == "system_name_probe":
            started = time.perf_counter_ns()
            address = int(command["address"])
            self._respond(
                command,
                "completed",
                address=f"0x{address:016X}",
                generated_system_name=self._engine.generated_system_name(address),
                elapsed_ns=time.perf_counter_ns() - started,
            )
            return
        if action == "object_resolver_probe":
            started = time.perf_counter_ns()
            address = int(command["address"])
            planet_index = int(command["planet_index"])
            snapshot = self._engine.system_snapshot(
                address,
                include_spawns=True,
                include_resolved_object_lists=True,
                resolved_object_planet_indices={planet_index},
            )
            planets = snapshot.get("planets", ())
            if not isinstance(planets, (list, tuple)):
                raise TypeError("object resolver snapshot has no planet collection")
            target = next(
                (
                    planet
                    for planet in planets
                    if isinstance(planet, dict)
                    and int(planet.get("planet_index", -1)) == planet_index
                ),
                None,
            )
            if target is None:
                raise RuntimeError(
                    f"generated system has no planet with index {planet_index}"
                )
            resolved = target.get("resolved_object_lists")
            if not isinstance(resolved, dict):
                raise RuntimeError("target planet has no resolved object-list result")
            self._respond(
                command,
                "completed",
                address=f"0x{address:016X}",
                planet_index=planet_index,
                source_headers=target.get("spawn_headers", {}).get(
                    "object_list_sources", {}
                ),
                object_list_sources=target.get("spawn_data", {}).get(
                    "object_list_sources", []
                ),
                resolved_object_lists=resolved,
                elapsed_ns=time.perf_counter_ns() - started,
            )
            return
        if action == "navigate":
            if self._pending_navigation is not None:
                raise RuntimeError("A Search Probe Target is already starting.")
            address = int(command["address"])
            self._engine.require_current_reality(address, operation="Navigation")
            manager = self._engine.scan_event_manager()
            self._navigation.remove_legacy_owned_routes(manager)
            native = self._navigation.start_form_navigation(address)
            self._pending_navigation = (command, native, self._ticks)
            return
        if action == "search":
            self._scheduler.advance_search(command)
            return
        if action in {"survey", "colour_survey", "spawn_survey"}:
            self._scheduler.advance_survey(command)
            return
        criteria = command.get("criteria", {})
        started = time.perf_counter_ns()
        if action == "candidate":
            address = int(command["address"])
            accepted, evaluation_pass, summary = self._engine.candidate_matches(
                criteria, address
            )
            self._respond(
                command,
                "completed",
                accepted=accepted,
                evaluation_pass=evaluation_pass,
                summary=summary,
                address=f"0x{address:016X}",
                elapsed_ns=time.perf_counter_ns() - started,
            )
            return
        if action != "nearest":
            raise RuntimeError(f"gameplay queue received unsupported action {action!r}")

        owned = self._engine.event_copy(criteria)
        result = ResolverResult()
        returned = int(
            self._native.find_nearest(
                ctypes.addressof(result),
                ctypes.addressof(owned),
                False,
                None,
                True,
                False,
            )
            or 0
        )
        elapsed_ns = time.perf_counter_ns() - started
        pointer_matches = returned == ctypes.addressof(result)
        fallback_used = bool(result.fallback_used)
        address = int(result.address)
        independent_accept = None
        independent_pass = None
        has_result = address != 0xFFFFFFFFFFFFFFFF
        if has_result:
            independent_accept, independent_pass, _summary = (
                self._engine.candidate_matches(criteria, address)
            )
        if not pointer_matches:
            raise RuntimeError(
                "resolver did not return its caller-owned result pointer"
            )
        self._respond(
            command,
            "completed",
            matched=bool(has_result and independent_accept),
            fallback_used=fallback_used,
            address=f"0x{address:016X}",
            candidate_satisfies_query=independent_accept,
            candidate_evaluation_pass=independent_pass,
            result_pointer_matches=pointer_matches,
            elapsed_ns=elapsed_ns,
        )

    def _mailbox_worker(self) -> None:
        while not self._stop_worker.is_set():
            self._drain_internal_writes()
            self._drain_responses()
            if self._close_requested.is_set():
                time.sleep(0.25)
                self._close_hook("commanded_shutdown")
                self._write_state("closed")
                self._drain_internal_writes()
                self._drain_responses()
                self._stop_worker.set()
                break
            for path in sorted(self._command_dir.glob("*.json")):
                if path.name in self._seen:
                    continue
                self._seen.add(path.name)
                try:
                    command = _PROTOCOL.normalize_command(
                        json.loads(path.read_text(encoding="utf-8"))
                    )
                    action = command["action"]
                    if action == "ping":
                        self._respond(
                            command, "completed", ready=self._ready, ticks=self._ticks
                        )
                    elif action == "cancel":
                        with self._cancel_lock:
                            self._cancelled.add(str(command["target_id"]))
                        self._respond(
                            command, "completed", cancelled=command["target_id"]
                        )
                    else:
                        generation = int(command["generation"])
                        if generation < self._generation:
                            self._respond(command, "stale")
                            continue
                        self._generation = generation
                        self._write_state("ready")
                        self._commands.put_nowait(command)
                except queue.Full:
                    self._respond_raw(
                        path.stem, 0, "error", error="command queue is full"
                    )
                except Exception as exc:  # noqa: BLE001 - malformed mailbox input is bounded
                    self._respond_raw(
                        path.stem,
                        0,
                        "error",
                        error=f"{type(exc).__name__}: {exc}",
                    )
            time.sleep(POLL_SECONDS)

    def _drain_internal_writes(self) -> None:
        while True:
            try:
                path, payload = self._internal_writes.get_nowait()
            except queue.Empty:
                return
            _atomic_json(path, payload)

    def _respond(
        self, command: dict[str, object], status: str, **payload: object
    ) -> None:
        self._responses.put(
            {
                "protocol": _PROTOCOL.PROTOCOL_VERSION,
                "session_id": self._session_id,
                "id": command["id"],
                "generation": command["generation"],
                "action": command["action"],
                "status": status,
                "thread_id": int(ctypes.windll.kernel32.GetCurrentThreadId()),
                "monotonic_ns": time.monotonic_ns(),
                **payload,
            }
        )

    def _respond_raw(
        self, command_id: str, generation: int, status: str, **payload: object
    ) -> None:
        self._responses.put(
            {
                "protocol": _PROTOCOL.PROTOCOL_VERSION,
                "session_id": self._session_id,
                "id": command_id,
                "generation": generation,
                "status": status,
                "thread_id": int(ctypes.windll.kernel32.GetCurrentThreadId()),
                "monotonic_ns": time.monotonic_ns(),
                **payload,
            }
        )

    def _respond_progress(self, command: dict[str, object], **payload: object) -> None:
        dispatch = self._guide_dispatches.get(str(command["id"]))
        if "_guide_slot" in command and dispatch is not None:
            checked = int(payload.get("candidates_checked", 0))
            total = int(payload.get("candidate_count", 0))
            found = int(payload.get("matches_found", 0))
            self._set_guide_status(
                "searching",
                f"Searching preset: {dispatch['name']} ({checked}/{total}, {found} found)",
                command_id=command["id"],
                slot=dispatch["slot"],
                candidates_checked=checked,
                candidate_count=total,
                matches_found=found,
            )
        self._responses.put(
            {
                "protocol": _PROTOCOL.PROTOCOL_VERSION,
                "session_id": self._session_id,
                "id": command["id"],
                "generation": command["generation"],
                "action": command["action"],
                "status": "progress",
                "thread_id": int(ctypes.windll.kernel32.GetCurrentThreadId()),
                "monotonic_ns": time.monotonic_ns(),
                **payload,
            }
        )

    def _drain_responses(self) -> None:
        while True:
            try:
                response = self._responses.get_nowait()
            except queue.Empty:
                return
            suffix = (
                ".progress.json" if response.get("status") == "progress" else ".json"
            )
            path = self._result_dir / f"{response['id']}{suffix}"
            with self._result_io_lock:
                if response.get("status") == "progress":
                    # This file is mutable but has only one in-process reader, protected by the
                    # same lock. Final results remain write-once and atomically published below.
                    path.write_text(
                        json.dumps(response, sort_keys=True, separators=(",", ":"))
                        + "\n",
                        encoding="utf-8",
                    )
                else:
                    _atomic_json(path, response)
            self._emit("response", response=response)

    def _write_state(self, state: str, **payload: object) -> None:
        _atomic_json(
            self._session / "state.json",
            {
                "protocol": _PROTOCOL.PROTOCOL_VERSION,
                "session_id": self._session_id,
                "state": state,
                "overlay_enabled": self._overlay_enabled,
                "ticks": self._ticks,
                "queue_size": self._commands.qsize(),
                "generation": self._generation,
                "monotonic_ns": time.monotonic_ns(),
                **payload,
            },
        )

    def _emit(self, kind: str, **payload: object) -> None:
        LOGGER.info(
            "SQN_RESIDENT_SEARCH %s",
            json.dumps(
                {
                    "schema": 1,
                    "kind": kind,
                    "session_id": self._session_id,
                    "monotonic_ns": time.monotonic_ns(),
                    "thread_id": int(ctypes.windll.kernel32.GetCurrentThreadId()),
                    **payload,
                },
                sort_keys=True,
                separators=(",", ":"),
            ),
        )

    def _squinch_before_injected_shutdown(self) -> None:
        overlay = getattr(self, "_overlay", None)
        if overlay is not None:
            overlay.close()
        self._stop_worker.set()
        if self._worker.is_alive() and self._worker is not threading.current_thread():
            self._worker.join(timeout=1.0)
        self._close_hook("controller_shutdown")
        self._write_state("closed")
        self._drain_internal_writes()
        self._drain_responses()

    def _close_hook(self, reason: str) -> None:
        self._guide.restore_wiki_probe()
        with self._hook_lock:
            hook = self._hook
            planet_data_hook = self._planet_data_hook
            scan_event_hook = self._scan_event_hook
            remove_scan_event_hook = self._remove_scan_event_hook
            if (
                hook is None
                and planet_data_hook is None
                and scan_event_hook is None
                and remove_scan_event_hook is None
            ):
                return
            if hook is not None:
                hook.close()
            if planet_data_hook is not None:
                planet_data_hook.close()
            if scan_event_hook is not None:
                scan_event_hook.close()
            if remove_scan_event_hook is not None:
                remove_scan_event_hook.close()
            self._hook = None
            self._planet_data_hook = None
            self._scan_event_hook = None
            self._remove_scan_event_hook = None
        self._emit("probe_closed", reason=reason, ticks=self._ticks)


def main() -> None:
    if len(sys.argv) != 2 or not sys.argv[1].lower().endswith(".exe"):
        raise SystemExit("usage: resident_search_probe.py <process.exe>")
    probe = Path(__file__).resolve()
    run_module(
        str(probe),
        {
            "exe": sys.argv[1],
            "start_exe": False,
            "start_paused": False,
            "interactive_console": False,
            "logging": {
                "shown": False,
                "log_dir": "{CURR_DIR}/logs",
                "log_level": "debug",
            },
            "gui": {"shown": False},
            "internal_mod_dir": None,
        },
    )


if __name__ == "__main__":
    main()

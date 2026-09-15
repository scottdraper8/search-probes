from __future__ import annotations

import ctypes
import time

from search_probes import planet_data as planet_snapshot
from search_probes import resident_search_protocol as _PROTOCOL
from search_probes import system_query_snapshot as _SNAPSHOT
from search_probes.native_layout import (
    APPLICATION_CURRENT_GALACTIC_POSITION_OFFSET,
    APPLICATION_GALAXY_GRAPH_OFFSET,
    APPLICATION_PROCEDURAL_ROOT_OFFSET,
    APPLICATION_SCAN_EVENT_MANAGER_OFFSET,
    APPLICATION_SINGLETON,
    EVENT_NAME,
    EVENT_REPLACE_IF_ACTIVE_OFFSET,
    EVENT_TABLE,
    PLANET_DIFFICULTY_OFFSET,
    PROCEDURAL_PLANET_GENERATOR_OFFSET,
    PROCEDURAL_RESOURCE_CACHE_OFFSET,
    PROCEDURAL_RESOURCE_KEY_CACHE_OFFSET,
    PROCEDURAL_RNG_OFFSET,
    RESOLVED_OBJECT_LIST_ELEMENT_SIZE,
    InternalVector,
    SpatialOutput,
)
from search_probes.planet_colours import capture_selected_colours


class SearchEngine:
    def __init__(self, native, resolve):
        self.native = native
        self.resolve = resolve
        self.last_enumeration = {}
        self._capture_enabled = False
        self._capture_planet_colours = False
        self._capture_planet_weather = False
        self._capture_planet_metadata = False
        self._capture_planet_spawn_flags = False
        self._capture_planet_spawns = False
        self._capture_resolved_object_lists = False
        self._resolved_object_planet_indices: frozenset[int] = frozenset()
        self._captured_planet_details: list[dict[str, object]] = []
        self._planet_detail_capture_error: str | None = None

    def capture_planet(self, planet_data):
        if self._capture_enabled:
            try:
                copied_details = planet_snapshot.copy_planet_details(
                    int(planet_data),
                    include_colours=self._capture_planet_colours,
                    include_weather=self._capture_planet_weather,
                    include_metadata=(
                        self._capture_planet_metadata
                        or self._capture_resolved_object_lists
                    ),
                    include_spawn_flags=self._capture_planet_spawn_flags,
                    include_spawns=self._capture_planet_spawns,
                )
                if self._capture_planet_colours:
                    copied_details["selected_colours"] = capture_selected_colours(
                        int(planet_data), self.procedural_planet_generator()
                    )
                if (
                    self._capture_resolved_object_lists
                    and int(copied_details["planet_index"])
                    in self._resolved_object_planet_indices
                ):
                    copied_details["resolved_object_lists"] = (
                        self.resolve_planet_object_lists(int(planet_data))
                    )
                self._captured_planet_details.append(copied_details)
            except Exception as exc:  # noqa: BLE001 - never unwind through an NMS callback
                self._planet_detail_capture_error = f"{type(exc).__name__}: {exc}"

    def event_copy(self, criteria: dict[str, object]) -> ctypes.Array:
        manager = self.scan_event_manager()
        event_name = ctypes.create_string_buffer(0x20)
        event_name.value = EVENT_NAME
        event_data = int(
            self.native.lookup_event(manager, ctypes.addressof(event_name), EVENT_TABLE)
            or 0
        )
        if not event_data:
            raise RuntimeError("R08 scan-event definition was not found in table 3")
        resolved_name = ctypes.string_at(event_data + 0x228, 0x20).split(b"\0", 1)[0]
        if resolved_name != EVENT_NAME:
            raise RuntimeError(f"resolved unexpected scan event: {resolved_name!r}")
        raw = ctypes.string_at(event_data, _PROTOCOL.EVENT_SIZE)
        mutated = bytearray(_PROTOCOL.apply_criteria(raw, criteria))
        # Preserve the native definition's replacement intent. Direct publication enters below
        # NMS's policy layer, so _remove_active_route performs the actual owned teardown first.
        mutated[EVENT_REPLACE_IF_ACTIVE_OFFSET] = 1
        return (ctypes.c_ubyte * _PROTOCOL.EVENT_SIZE).from_buffer_copy(mutated)

    def current_universe_address(self) -> int:
        application = int(
            ctypes.c_void_p.from_address(self.resolve(APPLICATION_SINGLETON)).value or 0
        )
        if not application:
            raise RuntimeError("application singleton is null")
        position = application + APPLICATION_CURRENT_GALACTIC_POSITION_OFFSET
        components = [
            int(ctypes.c_int32.from_address(position + offset).value)
            for offset in range(0, 0x18, 4)
        ]
        return _PROTOCOL.universe_address_from_components(
            planet_index=components[0],
            solar_system_index=components[1],
            voxel_x=components[2],
            voxel_y=components[3],
            voxel_z=components[4],
            reality_index=components[5],
        )

    def current_reality_index(self) -> int:
        application = int(
            ctypes.c_void_p.from_address(self.resolve(APPLICATION_SINGLETON)).value or 0
        )
        if not application:
            raise RuntimeError("application singleton is null")
        reality_index = int(
            ctypes.c_int32.from_address(
                application + APPLICATION_CURRENT_GALACTIC_POSITION_OFFSET + 0x14
            ).value
        )
        if not 0 <= reality_index <= _PROTOCOL.UNIVERSE_ADDRESS_REALITY_MASK:
            raise RuntimeError(f"current galaxy index is invalid: {reality_index}")
        return reality_index

    def require_current_reality(self, address: int, *, operation: str) -> int:
        address_reality = _PROTOCOL.universe_address_reality_index(address)
        current_reality = self.current_reality_index()
        if address_reality != current_reality:
            raise RuntimeError(
                f"{operation} target belongs to galaxy #{address_reality + 1}, but the player is "
                f"currently in galaxy #{current_reality + 1}; run the search again"
            )
        return current_reality

    def candidate_matches(
        self, criteria: dict[str, object], address: int
    ) -> tuple[bool, str | None, dict[str, object] | None]:
        snapshot_criteria = {
            field: value
            for field, value in criteria.items()
            if field in _SNAPSHOT.SNAPSHOT_CRITERIA
        }
        native_only_criteria = set(criteria) - _SNAPSHOT.SNAPSHOT_CRITERIA
        if not criteria or snapshot_criteria:
            object_island_value = snapshot_criteria.get("floating_islands")
            needs_object_resolution = object_island_value not in {None, "Any"}
            inexpensive_criteria = {
                field: value
                for field, value in snapshot_criteria.items()
                if field != "floating_islands"
            }
            needs_colours = _PROTOCOL.uses_primary_colour(inexpensive_criteria)
            needs_weather = _PROTOCOL.uses_planet_weather(inexpensive_criteria)
            needs_metadata = _PROTOCOL.uses_planet_metadata(inexpensive_criteria)
            detail_fields = {
                *_PROTOCOL.PRIMARY_COLOUR_FIELDS,
                *_PROTOCOL.WEATHER_FIELDS,
                *_PROTOCOL.PLANET_METADATA_FIELDS,
                *_PROTOCOL.PLANET_SPAWN_FLAG_FIELDS,
            }
            ordinary_criteria = {
                field: value
                for field, value in snapshot_criteria.items()
                if field not in detail_fields
            }
            if (
                needs_colours
                or needs_weather
                or needs_metadata
                or needs_object_resolution
            ) and ordinary_criteria:
                preliminary = self.system_snapshot(address)
                preliminary_accepted, _target_planet = _SNAPSHOT.matches(
                    ordinary_criteria, preliminary
                )
                if not preliminary_accepted:
                    return False, None, None
            snapshot = self.system_snapshot(
                address,
                include_colours=needs_colours,
                include_weather=needs_weather,
                include_metadata=needs_metadata,
            )
            if needs_object_resolution:
                candidate_planets = _SNAPSHOT.matching_planet_indices(
                    inexpensive_criteria, snapshot
                )
                if not candidate_planets:
                    return False, None, None
                snapshot = self.system_snapshot(
                    address,
                    include_colours=needs_colours,
                    include_weather=needs_weather,
                    include_metadata=needs_metadata,
                    include_resolved_object_lists=True,
                    resolved_object_planet_indices=set(candidate_planets),
                )
            accepted, target_planet = _SNAPSHOT.matches(snapshot_criteria, snapshot)
            if not accepted or target_planet is None:
                return False, None, None
            native_pass = None
            if native_only_criteria:
                native_accepted, native_pass = self.native_candidate_matches(
                    criteria, address
                )
                if not native_accepted:
                    return False, None, None
            snapshot["system_name"] = self.generated_system_name(address)
            evaluation_pass = f"generated_snapshot:planet={target_planet}" + (
                f";native={native_pass}" if native_pass is not None else ""
            )
            return (
                True,
                evaluation_pass,
                _SNAPSHOT.summarize_match(snapshot, target_planet),
            )
        native_accepted, native_pass = self.native_candidate_matches(criteria, address)
        if not native_accepted:
            return False, native_pass, None
        snapshot = self.system_snapshot(address)
        snapshot["system_name"] = self.generated_system_name(address)
        planets = snapshot.get("planets")
        if not isinstance(planets, tuple) or not planets:
            return False, "native match has no planet destination", None
        return True, native_pass, _SNAPSHOT.summarize_match(snapshot, 0)

    def generated_system_name(self, address: int) -> str:
        """Generate the base procedural system name without consulting discoveries."""

        application = int(
            ctypes.c_void_p.from_address(self.resolve(APPLICATION_SINGLETON)).value or 0
        )
        if not application:
            raise RuntimeError("application singleton is null")
        output = (ctypes.c_ubyte * 0x80)()
        self.native.generate_system_name(
            application + 0xCD0, address, ctypes.addressof(output), None
        )
        raw = bytes(output)
        terminator = raw.find(b"\0")
        if terminator < 0:
            raise RuntimeError("generated system name is not NUL-terminated")
        name = raw[:terminator].decode("utf-8")
        if not name:
            raise RuntimeError("generated system name is empty")
        return name

    def native_candidate_matches(
        self, criteria: dict[str, object], address: int
    ) -> tuple[bool, str | None]:
        """Evaluate the proven scan-event predicate, including bounded supplemental passes."""

        owned = self.event_copy(criteria)
        if bool(
            self.native.evaluate_candidate(
                ctypes.addressof(owned),
                address,
                False,
                True,
                None,
                False,
                False,
                False,
            )
        ):
            return True, "primary"
        for supplemental_pass in _PROTOCOL.supplemental_system_passes(criteria):
            if supplemental_pass == "Pirate":
                supplemental_criteria = {**criteria, "conflict_level": "Pirate"}
            else:
                supplemental_criteria = {
                    **criteria,
                    "population_state": supplemental_pass,
                }
            supplemental_owned = self.event_copy(supplemental_criteria)
            if bool(
                self.native.evaluate_candidate(
                    ctypes.addressof(supplemental_owned),
                    address,
                    False,
                    True,
                    None,
                    False,
                    False,
                    False,
                )
            ):
                return True, f"{supplemental_pass.lower()}_supplement"
        return False, None

    def procedural_planet_generator(self) -> int:
        application = int(
            ctypes.c_void_p.from_address(self.resolve(APPLICATION_SINGLETON)).value or 0
        )
        if not application:
            raise RuntimeError("application singleton is null")
        procedural_root = int(
            ctypes.c_void_p.from_address(
                application + APPLICATION_PROCEDURAL_ROOT_OFFSET
            ).value
            or 0
        )
        if not procedural_root:
            raise RuntimeError("procedural-generation root is null")
        return procedural_root + PROCEDURAL_PLANET_GENERATOR_OFFSET

    @staticmethod
    def internal_cache_state(generator: int) -> dict[str, dict[str, int | bool]]:
        result: dict[str, dict[str, int | bool]] = {}
        for name, offset in (
            ("resource_keys", PROCEDURAL_RESOURCE_KEY_CACHE_OFFSET),
            ("resources", PROCEDURAL_RESOURCE_CACHE_OFFSET),
        ):
            capacity = int(ctypes.c_uint32.from_address(generator + offset).value)
            size = int(ctypes.c_uint32.from_address(generator + offset + 4).value)
            pointer = int(
                ctypes.c_void_p.from_address(generator + offset + 8).value or 0
            )
            if size > capacity or (size and not pointer):
                raise RuntimeError(
                    f"procedural {name} cache has an invalid vector header"
                )
            result[name] = {
                "capacity": capacity,
                "size": size,
                "has_allocation": bool(pointer),
            }
        return result

    def destroy_resolved_object_vector(self, vector: InternalVector) -> None:
        pointer = int(vector.pointer or 0)
        size = int(vector.size)
        if pointer and size:
            end = pointer + size * RESOLVED_OBJECT_LIST_ELEMENT_SIZE
            self.native.destroy_object_list_elements(pointer, end, end)
        if pointer:
            # This exact thunk and argument tuple are used by NMS's vector reallocator after it
            # destroys the same 0x70-byte element type.
            self.native.deallocate(ctypes.addressof(vector), pointer, -1)
        ctypes.memset(ctypes.addressof(vector), 0, ctypes.sizeof(vector))

    def resolve_planet_object_lists(self, planet_data: int) -> dict[str, object]:
        """Exercise the isolated resolver with owned outputs and restore generator RNG state."""

        generator = self.procedural_planet_generator()
        rng_address = generator + PROCEDURAL_RNG_OFFSET
        rng_before = ctypes.string_at(rng_address, 8)
        cache_before = self.internal_cache_state(generator)
        outputs = {
            "detail_objects": InternalVector(),
            "objects": InternalVector(),
            "landmarks": InternalVector(),
            "distant_objects": InternalVector(),
        }
        started = time.perf_counter_ns()
        rng_after_call = rng_before
        copied: dict[str, object] = {}
        try:
            self.native.resolve_object_lists(
                generator,
                planet_data,
                ctypes.addressof(outputs["detail_objects"]),
                ctypes.addressof(outputs["objects"]),
                ctypes.addressof(outputs["landmarks"]),
                ctypes.addressof(outputs["distant_objects"]),
            )
            rng_after_call = ctypes.string_at(rng_address, 8)
            copied = {
                name: {
                    "header": planet_snapshot.internal_vector_header(vector),
                    "records": planet_snapshot.copy_resolved_object_list(vector),
                }
                for name, vector in outputs.items()
            }
        finally:
            for vector in outputs.values():
                self.destroy_resolved_object_vector(vector)
            ctypes.memmove(rng_address, rng_before, len(rng_before))
        outputs_released = all(
            int(vector.capacity) == 0
            and int(vector.size) == 0
            and not int(vector.pointer or 0)
            for vector in outputs.values()
        )
        return {
            "collections": copied,
            "elapsed_ns": time.perf_counter_ns() - started,
            "rng_advanced": rng_after_call != rng_before,
            "rng_restored": ctypes.string_at(rng_address, 8) == rng_before,
            "outputs_released": outputs_released,
            "cache_before": cache_before,
            "cache_after": self.internal_cache_state(generator),
        }

    def system_snapshot(
        self,
        address: int,
        *,
        include_colours: bool = False,
        include_weather: bool = False,
        include_metadata: bool = False,
        include_spawn_flags: bool = False,
        include_spawns: bool = False,
        include_resolved_object_lists: bool = False,
        resolved_object_planet_indices: set[int] | None = None,
    ) -> dict[str, object]:
        """Generate, copy, destroy, and then decode one caller-owned native query."""

        owned = (ctypes.c_ubyte * _SNAPSHOT.QUERY_SIZE)()
        owned_address = ctypes.addressof(owned)
        self.native.construct_solar_query(owned_address)
        try:
            # Type 1 is the game's planet-info query. Type 0 leaves the per-planet query records
            # unpopulated and is useful only for the native evaluator's system-only fast path.
            self._capture_enabled = True
            self._captured_planet_details = []
            self._planet_detail_capture_error = None
            self._capture_planet_colours = include_colours
            self._capture_planet_weather = include_weather
            self._capture_planet_metadata = include_metadata or include_spawns
            self._capture_planet_spawn_flags = include_spawn_flags or include_spawns
            self._capture_planet_spawns = include_spawns
            self._capture_resolved_object_lists = include_resolved_object_lists
            self._resolved_object_planet_indices = frozenset(
                resolved_object_planet_indices or ()
            )
            try:
                self.native.run_solar_query(owned_address, address, 1, False)
            finally:
                self._capture_enabled = False
                self._capture_planet_colours = False
                self._capture_planet_weather = False
                self._capture_planet_metadata = False
                self._capture_planet_spawn_flags = False
                self._capture_planet_spawns = False
                self._capture_resolved_object_lists = False
                self._resolved_object_planet_indices = frozenset()
            metadata = int(
                ctypes.c_void_p.from_address(owned_address + 0x1038).value or 0
            )
            expected_metadata = owned_address + _SNAPSHOT.METADATA_OFFSET
            if metadata != expected_metadata:
                raise RuntimeError(
                    "solar query did not retain its caller-owned metadata: "
                    f"expected 0x{expected_metadata:X}, found 0x{metadata:X}"
                )
            copied = ctypes.string_at(owned_address, _SNAPSHOT.QUERY_SIZE)
        finally:
            self.native.destroy_solar_query(owned_address)
        difficulty_offset = int(
            ctypes.c_int32.from_address(self.resolve(PLANET_DIFFICULTY_OFFSET)).value
        )
        snapshot = _SNAPSHOT.decode_query(copied, difficulty_offset=difficulty_offset)
        if self._planet_detail_capture_error is not None:
            raise RuntimeError(
                f"planet detail capture failed: {self._planet_detail_capture_error}"
            )
        captures = self._captured_planet_details
        planets = snapshot["planets"]
        if not isinstance(planets, tuple) or len(captures) != len(planets):
            decoded_count = len(planets) if isinstance(planets, tuple) else "?"
            raise RuntimeError(
                "planet detail capture count disagrees with query: "
                f"captured {len(captures)}, decoded {decoded_count}"
            )
        by_planet_index = {
            int(capture["planet_index"]): capture for capture in captures
        }
        if len(by_planet_index) != len(captures):
            raise RuntimeError(
                "planet detail captures contain duplicate planet indices"
            )
        for planet in planets:
            if not isinstance(planet, dict):
                raise TypeError(
                    "decoded planet must be mutable during detail attachment"
                )
            capture = by_planet_index.get(int(planet["planet_index"]))
            if capture is None:
                raise RuntimeError(
                    f"no detail capture for generated planet index {planet['planet_index']}"
                )
            planet["name"] = str(capture["name"])
            if include_colours:
                palettes = capture["palettes"]
                if not isinstance(palettes, dict):
                    raise TypeError("captured colour palettes must be a mapping")
                planet["colour_palettes"] = palettes
                primary_colours = {
                    name: palette["colours"][0]
                    for name, palette in palettes.items()
                    if isinstance(palette, dict)
                    and isinstance(palette.get("colours"), list)
                    and palette["colours"]
                }
                planet["primary_colours"] = primary_colours
                selected = capture["selected_colours"]
                primary_colours.update(selected["colours"])
                primary_colours.pop("WaterNear", None)
                planet["colour_selection"] = selected
                planet["primary_hues"] = {
                    name: _SNAPSHOT.classify_hue(colour)
                    for name, colour in primary_colours.items()
                }
            if include_weather:
                weather = capture["weather"]
                if not isinstance(weather, dict):
                    raise TypeError("captured weather must be a mapping")
                for field, values in (
                    ("storm_frequency", _PROTOCOL.STORM_FREQUENCIES),
                    ("weather_intensity", _PROTOCOL.WEATHER_INTENSITIES),
                    ("weather_type", _PROTOCOL.WEATHER_TYPES),
                ):
                    value = int(weather[field])
                    if not 0 <= value < len(values):
                        raise RuntimeError(
                            f"generated {field} value {value} is outside current enum"
                        )
                planet["weather"] = weather
                planet["query_storm_frequency_matches"] = int(
                    planet["query_storm_frequency"]
                ) == int(weather["storm_frequency"])
            if include_metadata or include_spawns or include_resolved_object_lists:
                metadata = capture["metadata"]
                if not isinstance(metadata, dict):
                    raise TypeError("captured planet metadata must be a mapping")
                if int(metadata["biome"]) != int(planet["biome"]):
                    raise RuntimeError(
                        "temporary planet biome disagrees with retained query"
                    )
                if int(metadata["biome_subtype"]) != int(planet["biome_subtype"]):
                    raise RuntimeError(
                        "temporary planet biome subtype disagrees with retained query"
                    )
                for field, values in (
                    ("life_level", _PROTOCOL.PLANET_LIFE_LEVELS),
                    ("creature_life_level", _PROTOCOL.PLANET_LIFE_LEVELS),
                    ("building_density", _PROTOCOL.BUILDING_DENSITY_LEVELS),
                    ("resource_level", _PROTOCOL.RESOURCE_LEVELS),
                ):
                    value = int(metadata[field])
                    if not 0 <= value < len(values):
                        raise RuntimeError(
                            f"generated {field} value {value} is outside current enum"
                        )
                    planet[field] = value
                sentinel_levels = metadata["sentinel_levels"]
                if not isinstance(sentinel_levels, list) or len(sentinel_levels) != 4:
                    raise RuntimeError(
                        "temporary planet has invalid sentinel-level vector"
                    )
                if any(not 0 <= int(value) < 4 for value in sentinel_levels):
                    raise RuntimeError(
                        f"temporary planet has invalid sentinel levels: {sentinel_levels!r}"
                    )
                sentinel_level = int(sentinel_levels[difficulty_offset])
                planet["sentinel_level"] = sentinel_level
                planet["inhabiting_race"] = int(metadata["inhabiting_race"])
                planet["name"] = str(metadata["name"])
                planet["has_scrap"] = bool(metadata["has_scrap"])
                planet["planet_info"] = metadata["planet_info"]
                planet["spawn_headers"] = metadata["spawn_headers"]
                planet["is_paradise"] = _SNAPSHOT.is_paradise_planet(
                    biome=int(metadata["biome"]),
                    biome_subtype=int(metadata["biome_subtype"]),
                    weather_intensity=int(metadata["weather_intensity"]),
                    storm_frequency=int(metadata["storm_frequency"]),
                    sentinel_level=sentinel_level,
                )
            if include_spawns:
                spawn_data = capture["spawn_data"]
                if not isinstance(spawn_data, dict):
                    raise TypeError("captured spawn data must be a mapping")
                planet["spawn_data"] = spawn_data
            if include_spawn_flags or include_spawns:
                planet["has_final_spawn_floating_island_objects"] = bool(
                    capture["has_final_spawn_floating_island_objects"]
                )
            if include_resolved_object_lists and int(
                planet["planet_index"]
            ) in frozenset(resolved_object_planet_indices or ()):
                resolved = capture.get("resolved_object_lists")
                if not isinstance(resolved, dict):
                    raise RuntimeError(
                        "target planet did not produce a resolved object-list capture"
                    )
                planet["resolved_object_lists"] = resolved
                planet["has_floating_islands"] = (
                    planet_snapshot.resolved_lists_have_floating_islands(resolved)
                )
        return snapshot

    def enumerate_candidates(
        self, candidate_limit: int = 64, *, maximum: int = _PROTOCOL.MAX_CANDIDATES
    ) -> list[dict[str, object]]:
        if not 1 <= candidate_limit <= maximum:
            raise ValueError(f"candidate limit must be from 1 through {maximum}")
        application = int(
            ctypes.c_void_p.from_address(self.resolve(APPLICATION_SINGLETON)).value or 0
        )
        if not application:
            raise RuntimeError("application singleton is null")
        galaxy = int(
            ctypes.c_void_p.from_address(
                application + APPLICATION_GALAXY_GRAPH_OFFSET
            ).value
            or 0
        )
        if not galaxy:
            raise RuntimeError("galaxy graph is null")
        ready = bool(ctypes.c_uint8.from_address(galaxy + 0x1E1).value)
        updating = bool(ctypes.c_uint8.from_address(galaxy + 0x1E0).value)
        if not ready or updating:
            raise RuntimeError(
                f"galaxy graph is not readable: ready={ready}, updating={updating}"
            )
        system_count = int(ctypes.c_uint32.from_address(galaxy + 0xF4).value)
        addresses = int(ctypes.c_void_p.from_address(galaxy + 0x138).value or 0)
        if system_count <= 0 or not addresses:
            raise RuntimeError("galaxy graph has no indexed system addresses")

        ids = (ctypes.c_uint64 * candidate_limit)()
        distances = (ctypes.c_float * candidate_limit)()
        distances[candidate_limit - 1] = ctypes.c_float.from_buffer_copy(
            bytes.fromhex("FFFF7F7F")
        ).value
        output = SpatialOutput(
            ids=ctypes.addressof(ids),
            distances=ctypes.addressof(distances),
            capacity=candidate_limit,
            count=0,
        )
        coordinate_words = (ctypes.c_uint32 * 6).from_address(
            application + APPLICATION_CURRENT_GALACTIC_POSITION_OFFSET
        )
        current_planet_index = int(
            ctypes.c_int32.from_address(
                application + APPLICATION_CURRENT_GALACTIC_POSITION_OFFSET
            ).value
        )
        if not -1 <= current_planet_index <= 15:
            raise RuntimeError(
                f"current planet index is invalid: {current_planet_index}"
            )
        current_reality_index = int(coordinate_words[5])
        if not 0 <= current_reality_index <= _PROTOCOL.UNIVERSE_ADDRESS_REALITY_MASK:
            raise RuntimeError(
                f"current galaxy index is invalid: {current_reality_index}"
            )
        coordinate = (ctypes.c_uint8 * 7)()
        ctypes.c_uint16.from_buffer(coordinate, 0).value = coordinate_words[2] & 0xFFFF
        ctypes.c_uint16.from_buffer(coordinate, 2).value = coordinate_words[4] & 0xFFFF
        ctypes.c_uint16.from_buffer(coordinate, 4).value = coordinate_words[3] & 0xFFFF
        coordinate[6] = 1
        graph_record = (ctypes.c_uint8 * 0x40)()
        ctypes.c_uint64.from_buffer(graph_record, 0).value = 0xFFFFFFFFFFFFFFFF
        coordinate_result = int(
            self.native.coordinate_to_graph(
                galaxy + 0x50,
                ctypes.addressof(coordinate),
                coordinate_words[1] & 0xFFFF,
                ctypes.addressof(graph_record),
            )
        )
        resolved_address = int(ctypes.c_uint64.from_buffer(graph_record, 0).value)
        if resolved_address == 0xFFFFFFFFFFFFFFFF:
            raise RuntimeError(
                "current galactic coordinate was not present in the graph: "
                f"status={coordinate_result}"
            )
        resolved_reality_index = _PROTOCOL.universe_address_reality_index(
            resolved_address
        )
        if resolved_reality_index != current_reality_index:
            raise RuntimeError(
                f"active galaxy graph belongs to galaxy #{resolved_reality_index + 1}, but the "
                f"player is currently in galaxy #{current_reality_index + 1}"
            )
        center_raw = bytes(graph_record[0x30:0x3C])
        center = (ctypes.c_float * 3).from_buffer_copy(center_raw)
        self.last_enumeration = {
            "coordinate_result": coordinate_result,
            "resolved_address": f"0x{resolved_address:016X}",
            "center_raw": center_raw.hex(),
            "graph_system_count": system_count,
            "current_planet_index": current_planet_index,
            "current_reality_index": current_reality_index,
            "current_galaxy_number": current_reality_index + 1,
        }
        options = (ctypes.c_uint8 * 16)()
        ctypes.c_uint64.from_buffer(options, 0).value = 0x20
        options[8] = 1
        self.native.spatial_nearest(
            galaxy + 0x140,
            ctypes.addressof(output),
            ctypes.addressof(center),
            ctypes.addressof(options),
        )
        if output.count > output.capacity or output.capacity != candidate_limit:
            raise RuntimeError(
                f"spatial query returned invalid count/capacity {output.count}/{output.capacity}"
            )
        result: list[dict[str, object]] = []
        for index in range(int(output.count)):
            graph_id = int(ids[index])
            if graph_id >= system_count:
                raise RuntimeError(
                    f"spatial query returned graph ID {graph_id} outside {system_count} systems"
                )
            address = int(ctypes.c_uint64.from_address(addresses + graph_id * 8).value)
            reality_index = _PROTOCOL.universe_address_reality_index(address)
            if reality_index != current_reality_index:
                raise RuntimeError(
                    f"galaxy graph candidate {graph_id} belongs to galaxy "
                    f"#{reality_index + 1}, expected galaxy #{current_reality_index + 1}"
                )
            result.append(
                {
                    "rank": index,
                    "graph_id": graph_id,
                    "address": f"0x{address:016X}",
                    "reality_index": reality_index,
                    "galaxy_number": reality_index + 1,
                    "distance_squared": float(distances[index]),
                }
            )
        return result

    def scan_event_manager(self) -> int:
        application = int(
            ctypes.c_void_p.from_address(self.resolve(APPLICATION_SINGLETON)).value or 0
        )
        if not application:
            raise RuntimeError("application singleton is null")
        manager = int(
            ctypes.c_void_p.from_address(
                application + APPLICATION_SCAN_EVENT_MANAGER_OFFSET
            ).value
            or 0
        )
        if not manager:
            raise RuntimeError("application scan-event manager is null")
        return manager

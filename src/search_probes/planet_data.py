from __future__ import annotations

import ctypes

from search_probes import native_memory as memory
from search_probes import system_query_snapshot as _SNAPSHOT
from search_probes.native_layout import (
    COLOUR_PALETTES,
    MAX_DYNAMIC_ARRAY_ITEMS,
    OBJECT_LIST_FILENAME_SIZE,
    OBJECT_LIST_SOURCE_SIZE,
    PLANET_BUILDING_LEVEL_OFFSET,
    PLANET_CREATURE_LIFE_OFFSET,
    PLANET_GROUND_COMBAT_STRIDE,
    PLANET_HAS_SCRAP_OFFSET,
    PLANET_IN_ABANDONED_SYSTEM_OFFSET,
    PLANET_IN_EMPTY_SYSTEM_OFFSET,
    PLANET_IN_GAS_GIANT_SYSTEM_OFFSET,
    PLANET_INDEX_OFFSET,
    PLANET_INFO_OFFSET,
    PLANET_INHABITING_RACE_OFFSET,
    PLANET_LIFE_OFFSET,
    PLANET_NAME_OFFSET,
    PLANET_OBJECT_LIST_SOURCE_OFFSET,
    PLANET_RESOURCE_LEVEL_OFFSET,
    PLANET_SENTINEL_LEVEL_BASE_OFFSET,
    PLANET_SPAWN_DATA_OFFSET,
    PLANET_STORM_FREQUENCY_OFFSET,
    PLANET_WEATHER_INTENSITY_OFFSET,
    PLANET_WEATHER_TYPE_OFFSET,
    RESOLVED_OBJECT_LIST_ELEMENT_SIZE,
    SELECTABLE_SPAWN_SIZE,
    SPAWN_COLLECTIONS,
    InternalVector,
)


def copy_resource_element(address: int) -> dict[str, object]:
    return {
        "alt_id": memory.dynamic_string(address),
        "filename": memory.dynamic_string(address + 0x10),
        "seed": int(ctypes.c_uint64.from_address(address + 0x30).value),
        "resource_id": int(ctypes.c_uint32.from_address(address + 0x40).value),
    }


def copy_object_spawn_variant(address: int) -> dict[str, object]:
    return {
        "id": memory.fixed_string(address, 0x10),
        "lod_distances": [
            float(ctypes.c_float.from_address(address + 0x10 + index * 4).value)
            for index in range(5)
        ],
        "coverage": float(ctypes.c_float.from_address(address + 0x24).value),
        "fade_out_end_distance": float(
            ctypes.c_float.from_address(address + 0x28).value
        ),
        "fade_out_offset_distance": float(
            ctypes.c_float.from_address(address + 0x2C).value
        ),
        "fade_out_start_distance": float(
            ctypes.c_float.from_address(address + 0x30).value
        ),
        "flat_density": float(ctypes.c_float.from_address(address + 0x34).value),
        "max_imposter_radius": int(ctypes.c_int32.from_address(address + 0x38).value),
        "max_region_radius": int(ctypes.c_int32.from_address(address + 0x3C).value),
        "slope_density": float(ctypes.c_float.from_address(address + 0x40).value),
        "slope_multiplier": float(ctypes.c_float.from_address(address + 0x44).value),
    }


def copy_object_spawn(address: int) -> dict[str, object]:
    extra_tile_types = memory.dynamic_array_header(address + 0xC0)
    alt_resources = memory.dynamic_array_header(address + 0x90)
    quality_variants = memory.dynamic_array_header(address + 0xE0)
    return {
        "quality_variant_data": copy_object_spawn_variant(address),
        "resource": copy_resource_element(address + 0x48),
        "alt_resource_count": int(alt_resources["size"]),
        "alt_resources": [
            copy_resource_element(int(alt_resources["pointer"]) + index * 0x48)
            for index in range(int(alt_resources["size"]))
        ],
        "debug_name": memory.fixed_string(address + 0xA0, 0x10),
        "destroyed_by_vehicle_effect": memory.fixed_string(address + 0xB0, 0x10),
        "extra_tile_types": [
            int(
                ctypes.c_uint32.from_address(
                    int(extra_tile_types["pointer"]) + index * 4
                ).value
            )
            for index in range(int(extra_tile_types["size"]))
        ],
        "placement": memory.fixed_string(address + 0xD0, 0x10),
        "quality_variant_count": int(quality_variants["size"]),
        "quality_variants": [
            copy_object_spawn_variant(int(quality_variants["pointer"]) + index * 0x48)
            for index in range(int(quality_variants["size"]))
        ],
        "seed": int(ctypes.c_uint64.from_address(address + 0xF0).value),
        "ground_colour_index": int(ctypes.c_uint32.from_address(address + 0x100).value),
        "large_object_coverage": int(
            ctypes.c_uint32.from_address(address + 0x104).value
        ),
        "max_angle": float(ctypes.c_float.from_address(address + 0x108).value),
        "max_height": float(ctypes.c_float.from_address(address + 0x10C).value),
        "max_lower": float(ctypes.c_float.from_address(address + 0x110).value),
        "max_raise": float(ctypes.c_float.from_address(address + 0x114).value),
        "max_scale": float(ctypes.c_float.from_address(address + 0x118).value),
        "max_scale_y": float(ctypes.c_float.from_address(address + 0x11C).value),
        "max_xz_rotation": float(ctypes.c_float.from_address(address + 0x120).value),
        "max_y_rotation": float(ctypes.c_float.from_address(address + 0x124).value),
        "min_angle": float(ctypes.c_float.from_address(address + 0x128).value),
        "min_height": float(ctypes.c_float.from_address(address + 0x12C).value),
        "min_scale": float(ctypes.c_float.from_address(address + 0x130).value),
        "min_scale_y": float(ctypes.c_float.from_address(address + 0x134).value),
        "order": int(ctypes.c_int32.from_address(address + 0x138).value),
        "overlap_style": int(ctypes.c_uint32.from_address(address + 0x13C).value),
        "patch_edge_scaling": float(ctypes.c_float.from_address(address + 0x140).value),
        "placement_priority": int(ctypes.c_uint32.from_address(address + 0x144).value),
        "shear_wind_strength": float(
            ctypes.c_float.from_address(address + 0x148).value
        ),
        "slope_scaling": float(ctypes.c_float.from_address(address + 0x14C).value),
        "type": int(ctypes.c_uint32.from_address(address + 0x150).value),
        "align_to_normal": bool(ctypes.c_uint8.from_address(address + 0x154).value),
        "auto_collision": bool(ctypes.c_uint8.from_address(address + 0x155).value),
        "collide_with_player": bool(ctypes.c_uint8.from_address(address + 0x156).value),
        "collide_with_player_vehicle": bool(
            ctypes.c_uint8.from_address(address + 0x157).value
        ),
        "creatures_can_eat": bool(ctypes.c_uint8.from_address(address + 0x158).value),
        "destroyed_by_player_ship": bool(
            ctypes.c_uint8.from_address(address + 0x159).value
        ),
        "destroyed_by_player_vehicle": bool(
            ctypes.c_uint8.from_address(address + 0x15A).value
        ),
        "destroyed_by_terrain_edit": bool(
            ctypes.c_uint8.from_address(address + 0x15B).value
        ),
        "imposter_activation": int(ctypes.c_uint8.from_address(address + 0x15C).value),
        "imposter_type": int(ctypes.c_uint8.from_address(address + 0x15D).value),
        "invisible_to_camera": bool(ctypes.c_uint8.from_address(address + 0x15E).value),
        "is_floating_island": bool(ctypes.c_uint8.from_address(address + 0x15F).value),
        "match_ground_colour": bool(ctypes.c_uint8.from_address(address + 0x160).value),
        "move_to_ground_on_upgrade": bool(
            ctypes.c_uint8.from_address(address + 0x161).value
        ),
        "relative_to_sea_level": bool(
            ctypes.c_uint8.from_address(address + 0x162).value
        ),
        "supports_scan_to_reveal": bool(
            ctypes.c_uint8.from_address(address + 0x163).value
        ),
        "swap_primary_for_random_colour": bool(
            ctypes.c_uint8.from_address(address + 0x164).value
        ),
        "swap_primary_for_secondary_colour": bool(
            ctypes.c_uint8.from_address(address + 0x165).value
        ),
        "use_multiple_upgrade_rays": bool(
            ctypes.c_uint8.from_address(address + 0x166).value
        ),
    }


def copy_creature_spawn(address: int) -> dict[str, object]:
    return {
        "extra_resource": copy_resource_element(address),
        "female_resource": copy_resource_element(address + 0x48),
        "resource": copy_resource_element(address + 0x90),
        "filter": memory.fixed_string(address + 0xD8, 0x20),
        "creature_id": memory.fixed_string(address + 0xF8, 0x10),
        "groups_per_square_km": float(
            ctypes.c_float.from_address(address + 0x114).value
        ),
        "min_group_size": int(ctypes.c_int32.from_address(address + 0x11C).value),
        "max_group_size": int(ctypes.c_int32.from_address(address + 0x118).value),
        "creature_role": int(ctypes.c_uint32.from_address(address + 0x120).value),
        "creature_type": int(ctypes.c_uint32.from_address(address + 0x128).value),
        "min_scale": float(ctypes.c_float.from_address(address + 0x134).value),
        "max_scale": float(ctypes.c_float.from_address(address + 0x130).value),
        "rarity": int(ctypes.c_uint32.from_address(address + 0x138).value),
    }


def copy_selectable_spawn_list(address: int) -> dict[str, object]:
    objects = memory.dynamic_array_header(address + 0x10)
    return {
        "name": memory.fixed_string(address, 0x10),
        "object_count": int(objects["size"]),
        "objects": [
            copy_resource_element(
                int(objects["pointer"]) + index * SELECTABLE_SPAWN_SIZE
            )
            for index in range(int(objects["size"]))
        ],
    }


def copy_object_list_sources(planet_data: int) -> list[dict[str, object]]:
    """Copy candidate-specific object-list source descriptors before destruction.

    This is the current runtime representation of GcExternalObjectListOptions. NMS's object
    resolver consumes it before selecting concrete 0x168-byte spawn records. Every dynamic
    filename is copied to Python-owned text while the temporary planet still exists; no game
    pointer crosses the callback.
    """

    header = memory.dynamic_array_header(planet_data + PLANET_OBJECT_LIST_SOURCE_OFFSET)
    pointer = int(header["pointer"])
    result: list[dict[str, object]] = []
    for index in range(int(header["size"])):
        address = pointer + index * OBJECT_LIST_SOURCE_SIZE
        options = memory.dynamic_array_header(address + 0x10)
        result.append(
            {
                "name": memory.fixed_string(address, 0x10),
                "options": [
                    memory.dynamic_string(
                        int(options["pointer"])
                        + option_index * OBJECT_LIST_FILENAME_SIZE
                    )
                    for option_index in range(int(options["size"]))
                ],
                "resource_hint": memory.fixed_string(address + 0x20, 0x10),
                "resource_hint_icon": memory.fixed_string(address + 0x30, 0x10),
                "order": int(ctypes.c_int32.from_address(address + 0x40).value),
                "probability": float(ctypes.c_float.from_address(address + 0x44).value),
                "seasonal_probability_override": float(
                    ctypes.c_float.from_address(address + 0x48).value
                ),
                "tile_type": int(ctypes.c_uint32.from_address(address + 0x4C).value),
                "add_to_filename_hashmap_when_optional": bool(
                    ctypes.c_uint8.from_address(address + 0x50).value
                ),
                "allow_limiting": bool(
                    ctypes.c_uint8.from_address(address + 0x51).value
                ),
                "choose_using_life_level": bool(
                    ctypes.c_uint8.from_address(address + 0x52).value
                ),
                "suppress_spawn": bool(
                    ctypes.c_uint8.from_address(address + 0x53).value
                ),
            }
        )
    return result


def internal_vector_header(vector: InternalVector) -> dict[str, int | bool]:
    capacity = int(vector.capacity)
    size = int(vector.size)
    pointer = int(vector.pointer or 0)
    if size > capacity:
        raise RuntimeError(
            f"resolved object-list vector size {size} exceeds capacity {capacity}"
        )
    if size > MAX_DYNAMIC_ARRAY_ITEMS:
        raise RuntimeError(
            f"resolved object-list vector size {size} exceeds investigation bound"
        )
    if size and not pointer:
        raise RuntimeError("non-empty resolved object-list vector has a null pointer")
    return {"capacity": capacity, "size": size, "has_allocation": bool(pointer)}


def copy_resolved_object_list(vector: InternalVector) -> list[dict[str, object]]:
    internal_vector_header(vector)
    pointer = int(vector.pointer or 0)
    result: list[dict[str, object]] = []
    for index in range(int(vector.size)):
        address = pointer + index * RESOLVED_OBJECT_LIST_ELEMENT_SIZE
        source_object = int(ctypes.c_void_p.from_address(address).value or 0)
        if not source_object:
            raise RuntimeError("resolved object-list element has a null source object")
        alternate_resources = memory.dynamic_array_header(address + 0x50)
        result.append(
            {
                # The first native field is a borrowed GcObjectSpawnData pointer. Copy the
                # complete record now and never publish or retain the address itself.
                "source_object": copy_object_spawn(source_object),
                "selected_resource": copy_resource_element(address + 0x08),
                "alternate_resources": [
                    copy_resource_element(
                        int(alternate_resources["pointer"])
                        + alternate_index * SELECTABLE_SPAWN_SIZE
                    )
                    for alternate_index in range(int(alternate_resources["size"]))
                ],
                "tile_type": int(ctypes.c_uint32.from_address(address + 0x60).value),
                # These three fixed fields are retained for repeatability/lifecycle evidence
                # only. Their product meaning remains deliberately unasserted.
                "unknown_u32_64": int(
                    ctypes.c_uint32.from_address(address + 0x64).value
                ),
                "allow_limiting": bool(
                    ctypes.c_uint8.from_address(address + 0x68).value
                ),
                "unknown_flag_69": bool(
                    ctypes.c_uint8.from_address(address + 0x69).value
                ),
                "suppress_spawn": bool(
                    ctypes.c_uint8.from_address(address + 0x6A).value
                ),
            }
        )
    return result


def copy_spawn_data(planet_data: int) -> dict[str, object]:
    result: dict[str, object] = {}
    for name, (offset, stride) in SPAWN_COLLECTIONS.items():
        header = memory.dynamic_array_header(
            planet_data + PLANET_SPAWN_DATA_OFFSET + offset
        )
        pointer = int(header["pointer"])
        size = int(header["size"])
        if name == "creatures":
            records = [
                copy_creature_spawn(pointer + index * stride) for index in range(size)
            ]
        elif name == "selectable_objects":
            records = [
                copy_selectable_spawn_list(pointer + index * stride)
                for index in range(size)
            ]
        else:
            records = [
                copy_object_spawn(pointer + index * stride) for index in range(size)
            ]
        result[name] = records
    return result


def has_final_spawn_floating_island_objects(planet_data: int) -> bool:
    """Inspect the resolved spawn flag without copying dynamic strings or resources."""

    for name, (offset, stride) in SPAWN_COLLECTIONS.items():
        if name in {"creatures", "selectable_objects"}:
            continue
        header = memory.dynamic_array_header(
            planet_data + PLANET_SPAWN_DATA_OFFSET + offset
        )
        pointer = int(header["pointer"])
        if any(
            ctypes.c_uint8.from_address(pointer + index * stride + 0x15F).value
            for index in range(int(header["size"]))
        ):
            return True
    return False


def copy_planet_details(
    planet_data: int,
    *,
    include_colours: bool,
    include_weather: bool,
    include_metadata: bool,
    include_spawn_flags: bool,
    include_spawns: bool,
) -> dict[str, object]:
    """Copy selected primitive fields before NMS destroys temporary planet data."""

    if not planet_data:
        raise RuntimeError("planet-data destructor received a null object")
    details: dict[str, object] = {
        "name": memory.fixed_string(planet_data + PLANET_NAME_OFFSET, 0x80),
        "planet_index": int(
            ctypes.c_int32.from_address(planet_data + PLANET_INDEX_OFFSET).value
        ),
    }
    if include_colours:
        palettes: dict[str, object] = {}
        for name, index in COLOUR_PALETTES.items():
            raw = ctypes.string_at(
                planet_data + index * _SNAPSHOT.COLOUR_PALETTE_SIZE,
                _SNAPSHOT.COLOUR_PALETTE_SIZE,
            )
            palettes[name] = _SNAPSHOT.decode_colour_palette(raw)
        details["palettes"] = palettes
    if include_weather:
        details["weather"] = {
            "storm_frequency": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_STORM_FREQUENCY_OFFSET
                ).value
            ),
            "weather_intensity": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_WEATHER_INTENSITY_OFFSET
                ).value
            ),
            "weather_type": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_WEATHER_TYPE_OFFSET
                ).value
            ),
        }
    if include_metadata or include_spawns:
        sentinel_levels = [
            int(
                ctypes.c_uint32.from_address(
                    planet_data
                    + PLANET_SENTINEL_LEVEL_BASE_OFFSET
                    + index * PLANET_GROUND_COMBAT_STRIDE
                ).value
            )
            for index in range(4)
        ]
        spawn_headers = {
            name: {
                key: value
                for key, value in memory.dynamic_array_header(
                    planet_data + PLANET_SPAWN_DATA_OFFSET + offset
                ).items()
                if key != "pointer"
            }
            for name, (offset, _stride) in SPAWN_COLLECTIONS.items()
        }
        spawn_headers["object_list_sources"] = {
            key: value
            for key, value in memory.dynamic_array_header(
                planet_data + PLANET_OBJECT_LIST_SOURCE_OFFSET
            ).items()
            if key != "pointer"
        }
        planet_info = {
            "sentinels_per_difficulty": [
                memory.fixed_string(
                    planet_data + PLANET_INFO_OFFSET + index * 0x80, 0x80
                )
                for index in range(4)
            ],
            "fauna": memory.fixed_string(
                planet_data + PLANET_INFO_OFFSET + 0x200, 0x80
            ),
            "flora": memory.fixed_string(
                planet_data + PLANET_INFO_OFFSET + 0x280, 0x80
            ),
            "planet_description": memory.fixed_string(
                planet_data + PLANET_INFO_OFFSET + 0x300, 0x80
            ),
            "planet_type": memory.fixed_string(
                planet_data + PLANET_INFO_OFFSET + 0x380, 0x80
            ),
            "resources": memory.fixed_string(
                planet_data + PLANET_INFO_OFFSET + 0x400, 0x80
            ),
            "weather": memory.fixed_string(
                planet_data + PLANET_INFO_OFFSET + 0x480, 0x80
            ),
            "is_weather_extreme": bool(
                ctypes.c_uint8.from_address(
                    planet_data + PLANET_INFO_OFFSET + 0x504
                ).value
            ),
            "special_fauna": bool(
                ctypes.c_uint8.from_address(
                    planet_data + PLANET_INFO_OFFSET + 0x505
                ).value
            ),
        }
        details["metadata"] = {
            "biome": int(ctypes.c_uint32.from_address(planet_data + 0x32B8).value),
            "biome_subtype": int(
                ctypes.c_uint32.from_address(planet_data + 0x32BC).value
            ),
            "storm_frequency": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_STORM_FREQUENCY_OFFSET
                ).value
            ),
            "weather_intensity": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_WEATHER_INTENSITY_OFFSET
                ).value
            ),
            "building_density": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_BUILDING_LEVEL_OFFSET
                ).value
            ),
            "creature_life_level": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_CREATURE_LIFE_OFFSET
                ).value
            ),
            "inhabiting_race": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_INHABITING_RACE_OFFSET
                ).value
            ),
            "life_level": int(
                ctypes.c_uint32.from_address(planet_data + PLANET_LIFE_OFFSET).value
            ),
            "resource_level": int(
                ctypes.c_uint32.from_address(
                    planet_data + PLANET_RESOURCE_LEVEL_OFFSET
                ).value
            ),
            "sentinel_levels": sentinel_levels,
            "name": details["name"],
            "has_scrap": bool(
                ctypes.c_uint8.from_address(planet_data + PLANET_HAS_SCRAP_OFFSET).value
            ),
            "in_abandoned_system": bool(
                ctypes.c_uint8.from_address(
                    planet_data + PLANET_IN_ABANDONED_SYSTEM_OFFSET
                ).value
            ),
            "in_empty_system": bool(
                ctypes.c_uint8.from_address(
                    planet_data + PLANET_IN_EMPTY_SYSTEM_OFFSET
                ).value
            ),
            "in_gas_giant_system": bool(
                ctypes.c_uint8.from_address(
                    planet_data + PLANET_IN_GAS_GIANT_SYSTEM_OFFSET
                ).value
            ),
            "planet_info": planet_info,
            "spawn_headers": spawn_headers,
        }
    if include_spawns:
        details["spawn_data"] = copy_spawn_data(planet_data)
        details["spawn_data"]["object_list_sources"] = copy_object_list_sources(
            planet_data
        )
    if include_spawn_flags or include_spawns:
        details["has_final_spawn_floating_island_objects"] = (
            has_final_spawn_floating_island_objects(planet_data)
        )
    return details


def resolved_lists_have_floating_islands(resolved: dict[str, object]) -> bool:
    """Read the exact native IsFloatingIsland flag from owned resolver output."""

    if not resolved.get("rng_restored") or not resolved.get("outputs_released"):
        raise RuntimeError(
            "object-list resolver did not restore all caller-owned state"
        )
    collections = resolved.get("collections")
    if not isinstance(collections, dict):
        raise TypeError("resolved object-list result has no collection mapping")
    for collection in collections.values():
        if not isinstance(collection, dict):
            raise TypeError("resolved object-list collection must be a mapping")
        records = collection.get("records")
        if not isinstance(records, list):
            raise TypeError("resolved object-list collection must contain records")
        for record in records:
            if not isinstance(record, dict):
                raise TypeError("resolved object-list record must be a mapping")
            source = record.get("source_object")
            if isinstance(source, dict) and source.get("is_floating_island") is True:
                return True
    return False

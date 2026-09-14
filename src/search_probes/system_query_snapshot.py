"""Decode and filter caller-owned NMS 7.01 solar-system query snapshots.

The native bridge constructs a 0x36c0-byte ``cGcSolarSystemQuery`` object, runs it on the
gameplay thread, copies the complete object while it is alive, and then calls its destructor.
This module is deliberately pure: it parses only fixed primitive fields from that immutable copy
and can therefore be tested without NMS or Windows.
"""

from __future__ import annotations

import struct
from collections.abc import Mapping
from colorsys import rgb_to_hsv

from search_probes.criteria import (
    BIOME_SUBTYPES,
    BIOMES,
    BUILDING_DENSITY_LEVELS,
    CONFLICT_LEVELS,
    HUE_FAMILIES,  # noqa: F401 - retained for diagnostic enum-contract consumers
    PLANET_LIFE_LEVELS,
    PLANET_SIZES,
    PRIMARY_COLOUR_CRITERIA,
    RACE_ENUM,
    RESOURCE_LEVELS,
    STAR_TYPES,
    STORM_FREQUENCIES,
    SYSTEM_CLASSES,
    TERRAIN_SETTINGS,
    TRADING_CLASSES,
    WEALTH_CLASSES,
    WEATHER_INTENSITIES,
    WEATHER_TYPES,
)

QUERY_SIZE = 0x36C0
METADATA_OFFSET = 0x1050
PLANET_QUERY_OFFSET = 0x790
PLANET_QUERY_SIZE = 0x170
PLANET_INPUT_OFFSET = METADATA_OFFSET + 0x2180
PLANET_INPUT_SIZE = 0x58
MAX_PLANETS = 8
COLOUR_PALETTE_SIZE = 0x70

RACES = RACE_ENUM
FLOATING_ISLAND_TERRAIN_SETTINGS = frozenset(
    {"FloatingIslands", "FloatingIslandsPrime", "FloatingIslandsPurple"}
)


def is_paradise_planet(
    *,
    biome: int,
    biome_subtype: int,
    weather_intensity: int,
    storm_frequency: int,
    sentinel_level: int,
) -> bool:
    """Reproduce the current executable's player-facing ``UI_PARADISE_PLANET`` predicate."""

    excluded_subtypes = {
        BIOME_SUBTYPES.index("Structure") + 1,
        BIOME_SUBTYPES.index("Infested") + 1,
        BIOME_SUBTYPES.index("Swamp") + 1,
    }
    return (
        biome == BIOMES.index("Lush")
        and biome_subtype not in excluded_subtypes
        and weather_intensity == WEATHER_INTENSITIES.index("Default")
        and storm_frequency == STORM_FREQUENCIES.index("None")
        and sentinel_level == 0
    )


def classify_hue(colour: object) -> str:
    """Classify one linear RGB palette value into a documented broad hue family."""

    if not isinstance(colour, (list, tuple)) or len(colour) < 3:
        raise ValueError("colour must contain at least three RGB components")
    red, green, blue = (float(colour[index]) for index in range(3))
    hue, saturation, _value = rgb_to_hsv(red, green, blue)
    if saturation < 0.12:
        return "Neutral"
    degrees = hue * 360.0
    if degrees < 15.0 or degrees >= 345.0:
        return "Red"
    if degrees < 45.0:
        return "Orange"
    if degrees < 75.0:
        return "Yellow"
    if degrees < 165.0:
        return "Green"
    if degrees < 195.0:
        return "Cyan"
    if degrees < 255.0:
        return "Blue"
    if degrees < 285.0:
        return "Purple"
    return "Magenta"


def decode_colour_palette(raw: bytes) -> dict[str, object]:
    """Decode one copied ``GcColourPaletteData`` record into owned primitives."""

    if len(raw) != COLOUR_PALETTE_SIZE:
        raise ValueError(
            f"colour palette snapshot must be exactly {COLOUR_PALETTE_SIZE:#x} bytes"
        )
    return {
        "colours": [
            list(struct.unpack_from("<4f", raw, slot * 0x10)) for slot in range(5)
        ],
        "indices": list(struct.unpack_from("<5i", raw, 0x50)),
    }


def _u32(raw: bytes, offset: int) -> int:
    return struct.unpack_from("<I", raw, offset)[0]


def _i32(raw: bytes, offset: int) -> int:
    return struct.unpack_from("<i", raw, offset)[0]


def _id(raw: bytes, offset: int) -> str:
    return raw[offset : offset + 0x10].split(b"\0", 1)[0].decode("ascii", "strict")


def _fixed_string(raw: bytes, offset: int, size: int) -> str:
    return (
        raw[offset : offset + size]
        .split(b"\0", 1)[0]
        .decode("utf-8", "backslashreplace")
    )


def decode_query(raw: bytes, *, difficulty_offset: int = 0) -> dict[str, object]:
    """Decode the fixed fields used by the finder from one live-owned query copy."""

    if len(raw) != QUERY_SIZE:
        raise ValueError(f"query snapshot must be exactly {QUERY_SIZE:#x} bytes")
    if not 0 <= difficulty_offset <= 3:
        raise ValueError(
            f"difficulty offset must be from 0 through 3, got {difficulty_offset}"
        )
    planets = _i32(raw, METADATA_OFFSET + 0x2544)
    if not 0 <= planets <= MAX_PLANETS:
        raise ValueError(f"query snapshot has invalid planet count {planets}")

    orbits = tuple(
        _i32(raw, METADATA_OFFSET + 0x24B0 + index * 4) for index in range(MAX_PLANETS)
    )
    decoded_planets: list[dict[str, object]] = []
    for index in range(planets):
        generation = PLANET_INPUT_OFFSET + index * PLANET_INPUT_SIZE
        query = PLANET_QUERY_OFFSET + index * PLANET_QUERY_SIZE
        resources = tuple(
            value
            for value in (
                _id(raw, query + 0x88),
                _id(raw, query + 0x98),
                _id(raw, query + 0xA8),
            )
            if value
        )
        planet_index = _i32(raw, generation + 0x3C)
        moon_indices = tuple(
            moon
            for moon in range(planets)
            if _u32(raw, PLANET_INPUT_OFFSET + moon * PLANET_INPUT_SIZE + 0x40) == 3
            and orbits[moon] == planet_index
        )
        terrain_setting_index = _i32(raw, query + 0x134)
        terrain_setting = (
            TERRAIN_SETTINGS[terrain_setting_index]
            if 0 <= terrain_setting_index < len(TERRAIN_SETTINGS)
            else None
        )
        decoded_planets.append(
            {
                "index": index,
                "planet_index": planet_index,
                "biome": _u32(raw, query + 0x80),
                "biome_subtype": _u32(raw, query + 0x84),
                "size": _u32(raw, generation + 0x40),
                "has_rings": bool(raw[generation + 0x4D]),
                "moon_indices": moon_indices,
                "resources": resources,
                "has_water": bool(raw[query + 0x120]),
                "has_deep_water": bool(raw[query + 0x121]),
                "has_sentinels": bool(raw[query + 0x124 + difficulty_offset]),
                "has_extreme_sentinels": bool(raw[query + 0x128 + difficulty_offset]),
                "has_corrupt_sentinels": bool(raw[query + 0x12C + difficulty_offset]),
                "has_extreme_weather": bool(raw[query + 0x130]),
                "has_extreme_hazard": bool(raw[query + 0x131]),
                # The native system-finalizer tests this retained field for nonzero before it
                # accepts the system-level extreme-storm predicate. Runtime capture verifies it
                # against temporary GcPlanetWeatherData.StormFrequency.
                "query_storm_frequency": _u32(raw, query + 0x168),
                "is_prime": bool(raw[query + 0x132]),
                "terrain_setting_index": terrain_setting_index,
                "terrain_setting": terrain_setting,
                "has_floating_islands": terrain_setting
                in FLOATING_ISLAND_TERRAIN_SETTINGS,
                "is_infested": _u32(raw, query + 0x84) == 24,
                "is_relic": (
                    _u32(raw, query + 0x80) == BIOMES.index("Weird")
                    and _u32(raw, query + 0x84) == BIOME_SUBTYPES.index("Structure") + 1
                ),
                "is_rgb": _u32(raw, query + 0x80)
                in {BIOMES.index("Red"), BIOMES.index("Green"), BIOMES.index("Blue")},
                "is_normal": (
                    _u32(raw, query + 0x80)
                    not in {
                        BIOMES.index("Dead"),
                        BIOMES.index("Weird"),
                    }
                    and _u32(raw, query + 0x84) != BIOME_SUBTYPES.index("Infested") + 1
                ),
                "suitable_creature_discovery": bool(raw[query + 0x138]),
                "suitable_weird_creature_discovery": bool(raw[query + 0x139]),
                "suitable_creature_taming": bool(raw[query + 0x13A]),
                "suitable_robot_creature_discovery": bool(raw[query + 0x13B]),
            }
        )

    population = "Inhabited"
    if any(
        raw[PLANET_INPUT_OFFSET + index * PLANET_INPUT_SIZE + 0x4F]
        for index in range(planets)
    ):
        population = "Empty"
    elif any(
        raw[PLANET_INPUT_OFFSET + index * PLANET_INPUT_SIZE + 0x4E]
        for index in range(planets)
    ):
        population = "Abandoned"

    has_giant = any(
        planet["size"] == PLANET_SIZES.index("Giant") for planet in decoded_planets
    )
    has_gas_giant = any(
        planet["biome"] == BIOMES.index("GasGiant") for planet in decoded_planets
    )
    has_non_gas_giant = any(
        planet["size"] == PLANET_SIZES.index("Giant")
        and planet["biome"] != BIOMES.index("GasGiant")
        for planet in decoded_planets
    )
    has_relic = any(
        planet["biome"] == BIOMES.index("Weird")
        and planet["biome_subtype"] == BIOME_SUBTYPES.index("Structure") + 1
        for planet in decoded_planets
    )
    has_rgb = any(
        planet["biome"]
        in {BIOMES.index("Red"), BIOMES.index("Green"), BIOMES.index("Blue")}
        for planet in decoded_planets
    )
    has_normal = any(
        planet["biome"] not in {BIOMES.index("Dead"), BIOMES.index("Weird")}
        and planet["biome_subtype"] != BIOME_SUBTYPES.index("Infested") + 1
        for planet in decoded_planets
    )
    return {
        "planets": tuple(decoded_planets),
        "planet_count": planets,
        "prime_planets": _i32(raw, METADATA_OFFSET + 0x2548),
        "trading_class": _u32(raw, METADATA_OFFSET + 0x2520),
        "wealth_class": _u32(raw, METADATA_OFFSET + 0x2524),
        "system_class": _u32(raw, METADATA_OFFSET + 0x252C),
        "conflict_level": _u32(raw, METADATA_OFFSET + 0x2530),
        "race": _u32(raw, METADATA_OFFSET + 0x2534),
        "star_type": _u32(raw, METADATA_OFFSET + 0x2550),
        "system_name": _fixed_string(raw, METADATA_OFFSET + 0x2554, 0x80),
        "population_state": population,
        "is_pirate": any(
            raw[PLANET_INPUT_OFFSET + index * PLANET_INPUT_SIZE + 0x51]
            for index in range(planets)
        ),
        "has_giant_planet": has_giant,
        "has_gas_giant": has_gas_giant,
        "has_non_gas_giant": has_non_gas_giant,
        "has_waterworld": any(
            planet["biome"] == BIOMES.index("Waterworld") for planet in decoded_planets
        ),
        "has_water_planet": any(planet["has_water"] for planet in decoded_planets),
        "has_deep_water_planet": any(
            planet["has_deep_water"] for planet in decoded_planets
        ),
        "has_weird_planet": any(
            planet["biome"] == BIOMES.index("Weird") for planet in decoded_planets
        ),
        "has_infested_planet": any(planet["is_infested"] for planet in decoded_planets),
        "has_relic_planet": has_relic,
        "has_rgb_planet": has_rgb,
        "has_normal_planet": has_normal,
        "has_corrupt_sentinel_planet": any(
            planet["has_corrupt_sentinels"] for planet in decoded_planets
        ),
        "has_extreme_storm_planet": any(
            planet["has_extreme_weather"] and planet["query_storm_frequency"] != 0
            for planet in decoded_planets
        ),
        "orbits": orbits,
    }


def _tristate_matches(value: object, actual: bool) -> bool:
    return value in {None, "Any"} or (value == "Require") == actual


def _first_match(
    criteria: Mapping[str, object], snapshot: Mapping[str, object]
) -> tuple[bool, int | None]:
    """Return whether a snapshot matches and the first matching target-planet index."""

    if criteria.get("star_type") not in {None, "Any"} and snapshot[
        "star_type"
    ] != STAR_TYPES.index(criteria["star_type"]):
        return False, None
    if (
        "minimum_planets" in criteria
        and snapshot["planet_count"] < criteria["minimum_planets"]
    ):
        return False, None
    if criteria.get("wealth_class") not in {None, "Any"} and snapshot[
        "wealth_class"
    ] != WEALTH_CLASSES.index(criteria["wealth_class"]):
        return False, None
    if criteria.get("trading_class") not in {None, "Any"} and (
        snapshot["is_pirate"]
        or snapshot["trading_class"] != TRADING_CLASSES.index(criteria["trading_class"])
    ):
        return False, None
    if (
        criteria.get("conflict_level") not in {None, "Any"}
        and snapshot["conflict_level"] != CONFLICT_LEVELS[criteria["conflict_level"]]
    ):
        return False, None
    if (
        criteria.get("population_state") not in {None, "Any"}
        and snapshot["population_state"] != criteria["population_state"]
    ):
        return False, None
    if criteria.get("race") not in {None, "Any"} and snapshot["race"] != RACES.index(
        criteria["race"]
    ):
        return False, None
    if not _tristate_matches(
        criteria.get("pirate_system"), bool(snapshot["is_pirate"])
    ):
        return False, None
    if not _tristate_matches(
        criteria.get("gas_giant_system"), bool(snapshot["has_gas_giant"])
    ):
        return False, None
    for criterion, field in (
        ("giant_planet_system", "has_giant_planet"),
        ("non_gas_giant_system", "has_non_gas_giant"),
        ("waterworld_system", "has_waterworld"),
        ("system_water", "has_water_planet"),
        ("deep_water_system", "has_deep_water_planet"),
        ("system_weird_planet", "has_weird_planet"),
        ("system_infested_planet", "has_infested_planet"),
        ("system_normal_planet", "has_normal_planet"),
        ("system_relic_planet", "has_relic_planet"),
        ("system_rgb_planet", "has_rgb_planet"),
        ("system_corrupt_sentinel_planet", "has_corrupt_sentinel_planet"),
        ("system_extreme_storm_planet", "has_extreme_storm_planet"),
    ):
        if not _tristate_matches(criteria.get(criterion), bool(snapshot[field])):
            return False, None

    for planet in snapshot["planets"]:
        if criteria.get("target_biome") not in {None, "Any"} and planet[
            "biome"
        ] != BIOMES.index(criteria["target_biome"]):
            continue
        if (
            criteria.get("biome_subtype") not in {None, "Any"}
            and planet["biome_subtype"]
            != BIOME_SUBTYPES.index(criteria["biome_subtype"]) + 1
        ):
            continue
        if criteria.get("planet_size") not in {None, "Any"} and planet[
            "size"
        ] != PLANET_SIZES.index(criteria["planet_size"]):
            continue
        if (
            criteria.get("terrain_setting") not in {None, "Any"}
            and planet["terrain_setting"] != criteria["terrain_setting"]
        ):
            continue
        fixed_metadata_mismatch = False
        for criterion, field, values in (
            ("life_level", "life_level", PLANET_LIFE_LEVELS),
            ("creature_life_level", "creature_life_level", PLANET_LIFE_LEVELS),
            ("building_density", "building_density", BUILDING_DENSITY_LEVELS),
            ("resource_level", "resource_level", RESOURCE_LEVELS),
        ):
            selected = criteria.get(criterion)
            if selected in {None, "Any"}:
                continue
            if field not in planet or planet[field] != values.index(selected):
                fixed_metadata_mismatch = True
                break
        if fixed_metadata_mismatch:
            continue
        if not _tristate_matches(
            criteria.get("paradise_planet"), bool(planet.get("is_paradise"))
        ):
            continue
        if not _tristate_matches(
            criteria.get("floating_islands"), bool(planet["has_floating_islands"])
        ):
            continue
        object_islands = criteria.get("floating_island_objects")
        if object_islands not in {None, "Any"}:
            if "has_floating_island_objects" not in planet:
                continue
            if not _tristate_matches(
                object_islands, bool(planet["has_floating_island_objects"])
            ):
                continue
        if not _tristate_matches(
            criteria.get("non_gas_giant_planet"),
            planet["size"] == PLANET_SIZES.index("Giant")
            and planet["biome"] != BIOMES.index("GasGiant"),
        ):
            continue
        if not _tristate_matches(
            criteria.get("planet_rings"), bool(planet["has_rings"])
        ):
            continue
        if not _tristate_matches(
            criteria.get("planet_has_moons"), bool(planet["moon_indices"])
        ):
            continue
        if not _tristate_matches(
            criteria.get("water_planet"), bool(planet["has_water"])
        ):
            continue
        if not _tristate_matches(
            criteria.get("deep_water_planet"), bool(planet["has_deep_water"])
        ):
            continue
        if not _tristate_matches(
            criteria.get("extreme_sentinel_planet"),
            bool(planet["has_extreme_sentinels"]),
        ):
            continue
        if not _tristate_matches(
            criteria.get("extreme_weather_planet"), bool(planet["has_extreme_weather"])
        ):
            continue
        if not _tristate_matches(
            criteria.get("extreme_hazard_planet"), bool(planet["has_extreme_hazard"])
        ):
            continue
        if not _tristate_matches(
            criteria.get("corrupt_sentinel_planet"),
            bool(planet["has_corrupt_sentinels"]),
        ):
            continue
        weather = planet.get("weather", {})
        if not isinstance(weather, Mapping):
            weather = {}
        if any(
            criteria.get(criterion) not in {None, "Any"}
            and weather.get(field) != values.index(criteria[criterion])
            for criterion, field, values in (
                ("storm_frequency", "storm_frequency", STORM_FREQUENCIES),
                ("weather_intensity", "weather_intensity", WEATHER_INTENSITIES),
                ("weather_type", "weather_type", WEATHER_TYPES),
            )
        ):
            continue
        primary_hues = planet.get("primary_hues", {})
        if not isinstance(primary_hues, Mapping):
            primary_hues = {}
        if any(
            criteria.get(criterion) not in {None, "Any"}
            and primary_hues.get(palette) != criteria[criterion]
            for criterion, palette in PRIMARY_COLOUR_CRITERIA.items()
        ):
            continue
        for criterion, field in (
            ("prime_planet", "is_prime"),
            ("infested_planet", "is_infested"),
            ("normal_planet", "is_normal"),
            ("relic_planet", "is_relic"),
            ("rgb_planet", "is_rgb"),
            ("has_scrap", "has_scrap"),
            ("sentinels", "has_sentinels"),
            ("suitable_creature_discovery", "suitable_creature_discovery"),
            ("suitable_weird_creature_discovery", "suitable_weird_creature_discovery"),
            ("suitable_creature_taming", "suitable_creature_taming"),
            ("suitable_robot_creature_discovery", "suitable_robot_creature_discovery"),
        ):
            selected = criteria.get(criterion)
            if selected not in {None, "Any"} and field not in planet:
                break
            if not _tristate_matches(selected, bool(planet.get(field))):
                break
        else:
            required_resources = criteria.get("required_resources", ())
            if not isinstance(required_resources, (list, tuple)):
                continue
            if required_resources and not set(required_resources) <= set(
                planet["resources"]
            ):
                continue
            if (
                criteria.get("resource") not in {None, "Any"}
                and criteria["resource"] not in planet["resources"]
            ):
                continue
            return True, int(planet["index"])
    return False, None


def matching_planet_indices(
    criteria: Mapping[str, object], snapshot: Mapping[str, object]
) -> tuple[int, ...]:
    """Return every planet satisfying one same-planet criterion conjunction.

    The system fields in ``snapshot`` remain unchanged for every evaluation. Restricting only the
    planet sequence lets the resident probe cheaply identify exactly which planets need an
    expensive second-stage native object-list resolution.
    """

    planets = snapshot.get("planets", ())
    if not isinstance(planets, (list, tuple)):
        raise TypeError("decoded snapshot planets must be a sequence")
    matching: list[int] = []
    for planet in planets:
        if not isinstance(planet, Mapping):
            raise TypeError("decoded planet must be a mapping")
        accepted, target = _first_match(criteria, {**snapshot, "planets": (planet,)})
        if accepted:
            if target is None:
                raise RuntimeError("accepted planet match had no target index")
            matching.append(int(target))
    return tuple(matching)


def matches(
    criteria: Mapping[str, object], snapshot: Mapping[str, object]
) -> tuple[bool, int | None]:
    """Return whether a snapshot matches and the first matching target-planet index."""

    return _first_match(criteria, snapshot)


def summarize_system(snapshot: Mapping[str, object]) -> dict[str, object]:
    """Return stable user-facing fields that describe the generated star system."""

    def label(values: tuple[str, ...], value: object) -> str:
        index = int(value)
        return values[index] if 0 <= index < len(values) else f"Unknown ({index})"

    conflict_by_value = {value: name for name, value in CONFLICT_LEVELS.items()}
    conflict = int(snapshot["conflict_level"])
    race = int(snapshot["race"])
    return {
        "name": str(snapshot.get("system_name", "")),
        "star_type": label(STAR_TYPES, snapshot["star_type"]),
        "planet_count": int(snapshot["planet_count"]),
        "wealth_class": label(WEALTH_CLASSES, snapshot["wealth_class"]),
        "trading_class": label(TRADING_CLASSES, snapshot["trading_class"]),
        "conflict_level": conflict_by_value.get(conflict, f"Unknown ({conflict})"),
        "population_state": str(snapshot["population_state"]),
        "race": label(RACES, race),
        "system_class": label(SYSTEM_CLASSES, snapshot["system_class"]),
        "is_pirate": bool(snapshot["is_pirate"]),
        "has_giant_planet": bool(snapshot["has_giant_planet"]),
        "has_gas_giant": bool(snapshot["has_gas_giant"]),
        "has_non_gas_giant": bool(snapshot["has_non_gas_giant"]),
        "has_waterworld": bool(snapshot["has_waterworld"]),
        "has_water_planet": bool(snapshot["has_water_planet"]),
        "has_deep_water_planet": bool(snapshot["has_deep_water_planet"]),
        "has_weird_planet": bool(snapshot["has_weird_planet"]),
        "has_infested_planet": bool(snapshot["has_infested_planet"]),
        "has_normal_planet": bool(snapshot["has_normal_planet"]),
        "has_relic_planet": bool(snapshot["has_relic_planet"]),
        "has_rgb_planet": bool(snapshot["has_rgb_planet"]),
        "has_corrupt_sentinel_planet": bool(snapshot["has_corrupt_sentinel_planet"]),
        "has_extreme_storm_planet": bool(snapshot["has_extreme_storm_planet"]),
    }


def summarize_match(
    snapshot: Mapping[str, object], target_planet: int
) -> dict[str, object]:
    """Return stable, user-facing primitive details for one exact match."""

    planets = snapshot["planets"]
    if not isinstance(planets, tuple) or not 0 <= target_planet < len(planets):
        raise ValueError(
            f"target planet index is outside the decoded snapshot: {target_planet}"
        )
    planet = planets[target_planet]
    if not isinstance(planet, Mapping):
        raise TypeError("decoded target planet must be a mapping")

    def label(values: tuple[str, ...], value: object) -> str:
        index = int(value)
        return values[index] if 0 <= index < len(values) else f"Unknown ({index})"

    subtype = int(planet["biome_subtype"])
    subtype_label = (
        BIOME_SUBTYPES[subtype - 1]
        if 1 <= subtype <= len(BIOME_SUBTYPES)
        else f"Unknown ({subtype})"
    )
    return {
        "system": summarize_system(snapshot),
        "planet": {
            "name": str(planet.get("name", "")),
            "index": int(planet["index"]),
            "planet_index": int(planet["planet_index"]),
            "biome": label(BIOMES, planet["biome"]),
            "biome_subtype": subtype_label,
            "size": label(PLANET_SIZES, planet["size"]),
            "terrain_setting": planet["terrain_setting"]
            or f"Unknown ({planet['terrain_setting_index']})",
            "has_floating_islands": bool(planet["has_floating_islands"]),
            **(
                {
                    "has_floating_island_objects": bool(
                        planet["has_floating_island_objects"]
                    )
                }
                if "has_floating_island_objects" in planet
                else {}
            ),
            "has_rings": bool(planet["has_rings"]),
            "moon_count": len(planet["moon_indices"]),
            "resources": list(planet["resources"]),
            "has_water": bool(planet["has_water"]),
            "has_deep_water": bool(planet["has_deep_water"]),
            "has_extreme_weather": bool(planet["has_extreme_weather"]),
            "has_extreme_hazard": bool(planet["has_extreme_hazard"]),
            "has_extreme_sentinels": bool(planet["has_extreme_sentinels"]),
            "has_corrupt_sentinels": bool(planet["has_corrupt_sentinels"]),
            "has_sentinels": bool(planet["has_sentinels"]),
            "is_prime": bool(planet["is_prime"]),
            "is_infested": bool(planet["is_infested"]),
            "is_normal": bool(planet["is_normal"]),
            "is_relic": bool(planet["is_relic"]),
            "is_rgb": bool(planet["is_rgb"]),
            **(
                {
                    "life_level": label(PLANET_LIFE_LEVELS, planet["life_level"]),
                    "creature_life_level": label(
                        PLANET_LIFE_LEVELS, planet["creature_life_level"]
                    ),
                    "building_density": label(
                        BUILDING_DENSITY_LEVELS, planet["building_density"]
                    ),
                    "resource_level": label(RESOURCE_LEVELS, planet["resource_level"]),
                    "sentinel_level": int(planet["sentinel_level"]),
                    "is_paradise": bool(planet["is_paradise"]),
                    "has_scrap": bool(planet["has_scrap"]),
                    "planet_info": dict(planet.get("planet_info", {})),
                }
                if all(
                    field in planet
                    for field in (
                        "life_level",
                        "creature_life_level",
                        "building_density",
                        "resource_level",
                        "sentinel_level",
                        "is_paradise",
                        "has_scrap",
                    )
                )
                else {}
            ),
            "suitable_creature_discovery": bool(planet["suitable_creature_discovery"]),
            "suitable_weird_creature_discovery": bool(
                planet["suitable_weird_creature_discovery"]
            ),
            "suitable_creature_taming": bool(planet["suitable_creature_taming"]),
            "suitable_robot_creature_discovery": bool(
                planet["suitable_robot_creature_discovery"]
            ),
            **(
                {
                    "primary_colours": dict(planet["primary_colours"]),
                    "primary_hues": dict(planet["primary_hues"]),
                }
                if isinstance(planet.get("primary_colours"), Mapping)
                and isinstance(planet.get("primary_hues"), Mapping)
                else {}
            ),
            **(
                {
                    "storm_frequency": label(
                        STORM_FREQUENCIES, planet["weather"]["storm_frequency"]
                    ),
                    "weather_intensity": label(
                        WEATHER_INTENSITIES, planet["weather"]["weather_intensity"]
                    ),
                    "weather_type": label(
                        WEATHER_TYPES, planet["weather"]["weather_type"]
                    ),
                }
                if isinstance(planet.get("weather"), Mapping)
                else {}
            ),
        },
    }


SNAPSHOT_CRITERIA = frozenset(
    {
        "target_biome",
        "star_type",
        "minimum_planets",
        "wealth_class",
        "trading_class",
        "conflict_level",
        "population_state",
        "biome_subtype",
        "race",
        "pirate_system",
        "gas_giant_system",
        "giant_planet_system",
        "non_gas_giant_system",
        "waterworld_system",
        "system_water",
        "deep_water_system",
        "system_weird_planet",
        "system_infested_planet",
        "system_normal_planet",
        "system_relic_planet",
        "system_rgb_planet",
        "system_corrupt_sentinel_planet",
        "system_extreme_storm_planet",
        "planet_size",
        "terrain_setting",
        "life_level",
        "creature_life_level",
        "building_density",
        "resource_level",
        "paradise_planet",
        "floating_islands",
        "floating_island_objects",
        "non_gas_giant_planet",
        "planet_rings",
        "planet_has_moons",
        "water_planet",
        "deep_water_planet",
        "extreme_sentinel_planet",
        "extreme_weather_planet",
        "extreme_hazard_planet",
        "corrupt_sentinel_planet",
        "prime_planet",
        "infested_planet",
        "normal_planet",
        "relic_planet",
        "rgb_planet",
        "has_scrap",
        "sentinels",
        "suitable_creature_discovery",
        "suitable_weird_creature_discovery",
        "suitable_creature_taming",
        "suitable_robot_creature_discovery",
        "resource",
        "required_resources",
        "storm_frequency",
        "weather_intensity",
        "weather_type",
        *PRIMARY_COLOUR_CRITERIA,
    }
)

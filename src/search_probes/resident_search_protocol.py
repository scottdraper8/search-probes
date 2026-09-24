"""Pure protocol and criteria validation shared by the resident NMS probe and host control."""

from __future__ import annotations

import json
import struct
from collections.abc import Mapping
from pathlib import Path

from search_probes.criteria import (
    ANOMALIES,
    BIOME_SUBTYPES,
    BIOMES,
    BUILDING_DENSITY_LEVELS,
    CONFLICT_LEVELS,
    HUE_FAMILIES,
    PLANET_LIFE_LEVELS,
    PLANET_SIZES,
    POPULATION_STATES,
    PRIMARY_COLOUR_CRITERIA,
    RACE_ENUM,
    RESOURCE_IDS,
    RESOURCE_LEVELS,
    STAR_TYPES,
    STORM_FREQUENCIES,
    SYSTEM_CLASSES,  # noqa: F401 - retained for diagnostic enum-contract consumers
    TERRAIN_SETTINGS,
    TRADING_CLASSES,
    WEALTH_CLASSES,
    WEATHER_CONDITIONS,
    WEATHER_INTENSITIES,
    WEATHER_TYPES,
)

PROTOCOL_VERSION = 1
EVENT_SIZE = 0x4C0
MAX_QUEUE_SIZE = 64
MAX_ACTIVE_SEARCHES = 8
MAX_CANDIDATES = 20000
MAX_SURVEY_CANDIDATES = 20000
MAX_SEARCH_CANDIDATES = 1_000_000_000
MAX_RESULTS = 16
MAX_GUIDE_PROBE_TOPICS = 128
LAUNCH_MODE_FILE = "launch-mode.json"
PROVEN_MINIMUM_PLANETS = (1, 2, 3, 4, 5)
UNIVERSE_ADDRESS_REALITY_SHIFT = 32
UNIVERSE_ADDRESS_REALITY_MASK = 0xFF
UNIVERSE_ADDRESS_PLANET_SHIFT = 52
UNIVERSE_ADDRESS_PLANET_MASK = 0xFFF


def universe_address_reality_index(address: int) -> int:
    """Return NMS's zero-based RealityIndex (galaxy index) from a 64-bit universe address."""

    if (
        not isinstance(address, int)
        or isinstance(address, bool)
        or not 0 <= address <= 0xFFFFFFFFFFFFFFFF
    ):
        raise ValueError("universe address must be an unsigned 64-bit integer")
    return (address >> UNIVERSE_ADDRESS_REALITY_SHIFT) & UNIVERSE_ADDRESS_REALITY_MASK


def universe_address_planet_index(address: int) -> int:
    """Return NMS's one-based planet field, where zero denotes system space."""

    universe_address_reality_index(address)
    return (address >> UNIVERSE_ADDRESS_PLANET_SHIFT) & UNIVERSE_ADDRESS_PLANET_MASK


def universe_address_system(address: int) -> int:
    """Return an address with its planet field cleared."""

    universe_address_reality_index(address)
    planet_bits = UNIVERSE_ADDRESS_PLANET_MASK << UNIVERSE_ADDRESS_PLANET_SHIFT
    return address & ~planet_bits


def universe_address_from_components(
    *,
    planet_index: int,
    solar_system_index: int,
    voxel_x: int,
    voxel_y: int,
    voxel_z: int,
    reality_index: int,
) -> int:
    """Encode the current ``cGcUniverseAddressData`` scalar layout.

    Runtime X/Z coordinates are signed views of the address's 12-bit fields.  Y is an
    eight-bit field and is accepted in either its signed or unsigned representation.
    ``planet_index`` is already NMS's one-based address value, unlike generated planet data.
    """

    values = {
        "planet_index": (planet_index, 0, UNIVERSE_ADDRESS_PLANET_MASK),
        "solar_system_index": (solar_system_index, 0, 0xFFF),
        "voxel_x": (voxel_x, -0x800, 0x7FF),
        "voxel_y": (voxel_y, -0x80, 0xFF),
        "voxel_z": (voxel_z, -0x800, 0x7FF),
        "reality_index": (reality_index, 0, UNIVERSE_ADDRESS_REALITY_MASK),
    }
    for name, (value, minimum, maximum) in values.items():
        if (
            not isinstance(value, int)
            or isinstance(value, bool)
            or not minimum <= value <= maximum
        ):
            raise ValueError(f"{name} must be from {minimum} through {maximum}")
    return (
        ((planet_index & 0xFFF) << UNIVERSE_ADDRESS_PLANET_SHIFT)
        | ((solar_system_index & 0xFFF) << 40)
        | ((reality_index & 0xFF) << UNIVERSE_ADDRESS_REALITY_SHIFT)
        | ((voxel_y & 0xFF) << 24)
        | ((voxel_z & 0xFFF) << 12)
        | (voxel_x & 0xFFF)
    )


def universe_address_with_planet_index(address: int, planet_index: int) -> int:
    """Target one zero-based generated planet using NMS's one-based address field."""

    universe_address_reality_index(address)
    if (
        not isinstance(planet_index, int)
        or isinstance(planet_index, bool)
        or not 0 <= planet_index < UNIVERSE_ADDRESS_PLANET_MASK
    ):
        raise ValueError(
            f"planet index must be from 0 through {UNIVERSE_ADDRESS_PLANET_MASK - 1}"
        )
    planet_bits = UNIVERSE_ADDRESS_PLANET_MASK << UNIVERSE_ADDRESS_PLANET_SHIFT
    # GcPlanetData.PlanetIndex is zero-based.  cGcUniverseAddressData reserves zero for the
    # system/no-specific-planet address and represents generated planet n as n + 1.  This was
    # verified live: generated index 3 corresponded to the loaded planet address value 4.
    address_planet_index = planet_index + 1
    return (address & ~planet_bits) | (
        address_planet_index << UNIVERSE_ADDRESS_PLANET_SHIFT
    )


def read_launch_overlay_enabled(root: Path) -> bool:
    """Read host-written launch mode from the shared mailbox boundary.

    The injected module executes inside NMS and therefore sees NMS's original process
    environment, not the injector environment.  Missing configuration preserves the normal
    interactive form; malformed configuration fails before either mode starts.
    """

    path = root / LAUNCH_MODE_FILE
    if not path.is_file():
        return True
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict) or payload.get("protocol") != PROTOCOL_VERSION:
        raise ValueError("resident launch-mode file has an invalid protocol")
    overlay_enabled = payload.get("overlay_enabled")
    if not isinstance(overlay_enabled, bool):
        raise TypeError("resident launch-mode overlay_enabled must be boolean")
    return overlay_enabled


PROVEN_BIOMES = tuple(value for value in BIOMES if value != "Test")
# These subsets are the values actually produced by the current off-screen query over the complete
# local graph. The broader schema domains remain accepted by the diagnostic protocol, while the
# owned form advertises only values for which a positive runtime control exists.
PROVEN_PLANET_LIFE_LEVELS = ("Dead", "Full")
PROVEN_CREATURE_LIFE_LEVELS = PLANET_LIFE_LEVELS
PROVEN_BUILDING_DENSITY_LEVELS = (
    "Dead",
    "Mid",
    "Full",
    "Weird",
    "HalfWeird",
    "Waterworld",
    "GasGiant",
)
PROVEN_RESOURCE_LEVELS = RESOURCE_LEVELS
PROVEN_TERRAIN_SETTINGS = tuple(
    value for value in TERRAIN_SETTINGS if value != "WaterworldPrime"
)
PROVEN_PRIMARY_HUES = {
    "grass_primary_hue": HUE_FAMILIES,
    "plant_primary_hue": ("Red", "Orange", "Yellow", "Green", "Cyan", "Blue", "Purple"),
    "leaf_primary_hue": HUE_FAMILIES,
    "water_primary_hue": ("Red", "Orange", "Green", "Cyan", "Blue", "Purple"),
    "cloud_primary_hue": HUE_FAMILIES,
    "sky_primary_hue": HUE_FAMILIES,
    "horizon_primary_hue": HUE_FAMILIES,
    "sky_fog_primary_hue": HUE_FAMILIES,
    "sky_height_fog_primary_hue": ("Orange", "Yellow", "Green", "Cyan", "Blue", "Magenta", "Neutral"),
    "sky_sunset_primary_hue": ("Orange",),
    "sky_night_primary_hue": ("Blue",),
}
PROVEN_STORM_FREQUENCIES = STORM_FREQUENCIES[:3]
PROVEN_WEATHER_TYPES = WEATHER_TYPES[:7] + WEATHER_TYPES[10:]
WEATHER_FIELDS = ("storm_frequency", "weather_intensity", "weather_type", "weather_conditions")
PLANET_METADATA_FIELDS = (
    "life_level",
    "creature_life_level",
    "building_density",
    "resource_level",
    "paradise_planet",
    "has_scrap",
)
PLANET_SPAWN_FLAG_FIELDS = ("floating_islands",)
PRIMARY_COLOUR_FIELDS = tuple(PRIMARY_COLOUR_CRITERIA)
if set(PROVEN_PRIMARY_HUES) != set(PRIMARY_COLOUR_FIELDS):
    raise RuntimeError(
        "proven primary-hue controls do not cover every colour criterion"
    )
PRIMARY_COLOUR_PALETTE_KEYS = PRIMARY_COLOUR_CRITERIA
if set(PRIMARY_COLOUR_PALETTE_KEYS) != set(PRIMARY_COLOUR_FIELDS):
    raise RuntimeError("primary-hue palette keys do not cover every colour criterion")
PROVEN_RACES = RACE_ENUM[:3]
RESOURCE_LABELS = {
    "ASTEROID1": "Silver",
    "ASTEROID2": "Gold",
    "BLUE2": "Indium",
    "CATALYST1": "Sodium",
    "CAVE1": "Cobalt",
    "COLD1": "Dioxite",
    "DUSTY1": "Pyrite",
    "EX_BLUE": "Activated Indium",
    "EX_GREEN": "Activated Emeril",
    "EX_PURPLE": "Activated Quartzite",
    "EX_RED": "Activated Cadmium",
    "EX_YELLOW": "Activated Copper",
    "GAS1": "Sulphurine",
    "GASGIANT1": "Crystallised Helium",
    "GREEN2": "Emeril",
    "HOT1": "Phosphorus",
    "LAND3": "Magnetised Ferrite",
    "LAVA1": "Basalt",
    "LUSH1": "Paraffinium",
    "PLANT_POOP": "Faecium",
    "PURPLE2": "Quartzite",
    "RADIO1": "Uranium",
    "RED2": "Cadmium",
    "SPACEGUNK3": "Rusted Metal",
    "TOXIC1": "Ammonia",
    "WATER1": "Salt",
    "WATERWORLD1": "Lithium",
    "YELLOW2": "Copper",
}
PROVEN_BIOME_SUBTYPES = BIOME_SUBTYPES[:29]
PROVEN_ANOMALIES = ("AtlasStation", "BlackHole")
RACE_VALUES = {name: value for value, name in enumerate(RACE_ENUM)}
REQUIREMENT_OFFSETS = {
    "any_biome_not_weird_or_dead": 0x9B,
    "any_infested_biome": 0x9C,
    "any_rgb_biome": 0x9D,
    "corrupt_sentinel_planet": 0xA0,
    "deep_water_planet": 0xA1,
    "extreme_hazard_planet": 0xA3,
    "prime_planet": 0xA6,
    "sentinels": 0xA7,
    "water_planet": 0xA8,
    "undiscovered": 0xAE,
    "suitable_creature_discovery": 0xAF,
    "suitable_creature_taming": 0xB0,
    "suitable_robot_creature_discovery": 0xB1,
    "suitable_weird_creature_discovery": 0xB2,
    "system_corrupt_sentinel_planet": 0xB3,
    "system_extreme_storm_planet": 0xB4,
    "system_infested_planet": 0xB6,
    "system_non_gas_giant": 0xB7,
    "system_relic_planet": 0xB8,
    "system_water": 0xB9,
    "system_weird_planet": 0xBA,
}
DUAL_REQUIREMENT_OFFSETS = {
    "extreme_sentinel_planet": (0xA4, 0xAB),
    "extreme_weather_planet": (0xA5, 0xAC),
    "gas_giant_system": (0xB5, 0xAD),
}
SNAPSHOT_TRISTATE_FIELDS = (
    "corrupt_sentinel_planet",
    "deep_water_planet",
    "extreme_hazard_planet",
    "floating_islands",
    "infested_planet",
    "relic_planet",
    "has_scrap",
    "normal_planet",
    "non_gas_giant_planet",
    "planet_rings",
    "planet_has_moons",
    "prime_planet",
    "rgb_planet",
    "sentinels",
    "suitable_creature_discovery",
    "suitable_creature_taming",
    "suitable_robot_creature_discovery",
    "suitable_weird_creature_discovery",
    "water_planet",
)
SYSTEM_TRISTATE_FIELDS = (
    "pirate_system",
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
)


def uses_primary_colour(criteria: Mapping[str, object]) -> bool:
    """Return whether a sparse query requires temporary full planet-palette capture."""

    return any(
        criteria.get(field) not in {None, "Any"} for field in PRIMARY_COLOUR_FIELDS
    )


def uses_planet_weather(criteria: Mapping[str, object]) -> bool:
    """Return whether a sparse query requires temporary full planet-weather capture."""

    return any(criteria.get(field) not in {None, "Any"} for field in WEATHER_FIELDS)


def uses_planet_metadata(criteria: Mapping[str, object]) -> bool:
    """Return whether a sparse query requires temporary fixed planet metadata."""

    return any(
        criteria.get(field) not in {None, "Any"} for field in PLANET_METADATA_FIELDS
    )


def uses_planet_spawn_flags(criteria: Mapping[str, object]) -> bool:
    """Return whether a sparse query must inspect resolved object-spawn records."""

    return any(
        criteria.get(field) not in {None, "Any"} for field in PLANET_SPAWN_FLAG_FIELDS
    )


def normalize_command(raw: object) -> dict[str, object]:
    """Validate a mailbox command without accepting arbitrary native-memory mutations."""

    if not isinstance(raw, Mapping):
        raise TypeError("command must be a JSON object")
    if raw.get("protocol") != PROTOCOL_VERSION:
        raise ValueError(f"protocol must be {PROTOCOL_VERSION}")
    command_id = raw.get("id")
    if not isinstance(command_id, str) or not command_id or len(command_id) > 128:
        raise ValueError("id must be a non-empty string of at most 128 characters")
    generation = raw.get("generation", 0)
    if (
        not isinstance(generation, int)
        or isinstance(generation, bool)
        or generation < 0
    ):
        raise ValueError("generation must be a non-negative integer")
    action = raw.get("action")
    if action not in {
        "candidate",
        "nearest",
        "search",
        "enumerate",
        "region_enumerate",
        "cancel",
        "shutdown",
        "ping",
        "snapshot",
        "survey",
        "colour_survey",
        "spawn_survey",
        "system_name_probe",
        "object_resolver_probe",
        "wiki_snapshot",
        "wiki_mirror_probe",
        "scan_event_observations",
        "navigate",
    }:
        raise ValueError(f"unsupported action: {action!r}")

    normalized: dict[str, object] = {
        "protocol": PROTOCOL_VERSION,
        "id": command_id,
        "generation": generation,
        "action": action,
    }
    if action in {"candidate", "nearest", "search"}:
        criteria = raw.get("criteria", {})
        if not isinstance(criteria, Mapping):
            raise ValueError("criteria must be a JSON object")
        normalized["criteria"] = normalize_criteria(criteria)
    if action == "search":
        name = raw.get("name", "System search")
        if not isinstance(name, str) or not name.strip() or len(name.strip()) > 80:
            raise ValueError("search name must be 1 through 80 characters")
        normalized["name"] = name.strip()
        result_limit = raw.get("result_limit", 5)
        if (
            not isinstance(result_limit, int)
            or isinstance(result_limit, bool)
            or not 1 <= result_limit <= MAX_RESULTS
        ):
            raise ValueError(
                f"result_limit must be an integer from 1 through {MAX_RESULTS}"
            )
        normalized["result_limit"] = result_limit
    if action in {"search", "enumerate", "survey", "colour_survey", "spawn_survey"}:
        candidate_limit = raw.get("candidate_limit", 64)
        maximum = (
            MAX_SURVEY_CANDIDATES
            if action in {"survey", "colour_survey", "spawn_survey"}
            else MAX_SEARCH_CANDIDATES if action == "search" else MAX_CANDIDATES
        )
        if (
            not isinstance(candidate_limit, int)
            or isinstance(candidate_limit, bool)
            or not 1 <= candidate_limit <= maximum
        ):
            raise ValueError(
                f"candidate_limit must be an integer from 1 through {maximum}"
            )
        normalized["candidate_limit"] = candidate_limit
    if action in {
        "candidate",
        "snapshot",
        "system_name_probe",
        "object_resolver_probe",
        "navigate",
        "region_enumerate",
    }:
        address = raw.get("address")
        if isinstance(address, str):
            try:
                address = int(address, 0)
            except ValueError as exc:
                raise ValueError("address string must be an integer literal") from exc
        if not isinstance(address, int) or isinstance(address, bool):
            raise ValueError(f"{action} action requires an integer address")
        if address < 0 or address > 0xFFFFFFFFFFFFFFFF:
            raise ValueError("address is outside uint64 range")
        normalized["address"] = address
    if action == "object_resolver_probe":
        planet_index = raw.get("planet_index", 0)
        if (
            not isinstance(planet_index, int)
            or isinstance(planet_index, bool)
            or not 0 <= planet_index <= 15
        ):
            raise ValueError("planet_index must be an integer from 0 through 15")
        normalized["planet_index"] = planet_index
    if action == "wiki_mirror_probe":
        topic_count = raw.get("topic_count")
        if (
            not isinstance(topic_count, int)
            or isinstance(topic_count, bool)
            or not 1 <= topic_count <= MAX_GUIDE_PROBE_TOPICS
        ):
            raise ValueError(
                "topic_count must be an integer from 1 through "
                f"{MAX_GUIDE_PROBE_TOPICS}"
            )
        label_prefix = raw.get("label_prefix", "")
        if not isinstance(label_prefix, str):
            raise TypeError("label_prefix must be a string")
        if len(label_prefix.encode("utf-8")) > 20:
            raise ValueError("label_prefix must be at most 20 UTF-8 bytes")
        normalized["topic_count"] = topic_count
        normalized["label_prefix"] = label_prefix
    if action == "cancel":
        target_id = raw.get("target_id")
        if not isinstance(target_id, str) or not target_id:
            raise ValueError("cancel action requires target_id")
        normalized["target_id"] = target_id
    return normalized


def normalize_criteria(raw: Mapping[object, object]) -> dict[str, object]:
    """Admit only criteria already exercised by the fixed-field runtime matrix."""

    raw = dict(raw)
    for old, current in (
        ("floating_island_objects", "floating_islands"),
        ("near_water_primary_hue", "water_primary_hue"),
    ):
        if old not in raw:
            continue
        value = raw.pop(old)
        if value == "Any":
            continue
        if raw.get(current, "Any") not in ("Any", value):
            raise ValueError(f"conflicting {old} and {current} filters; keep one setting")
        raw[current] = value
    allowed = {
        "target_biome",
        "star_type",
        "minimum_planets",
        "wealth_class",
        "trading_class",
        "conflict_level",
        "population_state",
        "biome_subtype",
        "anomaly",
        "race",
        "planet_size",
        "terrain_setting",
        "life_level",
        "creature_life_level",
        "building_density",
        "resource_level",
        "paradise_planet",
        "planet_rings",
        "planet_has_moons",
        "resource",
        "required_resources",
        *WEATHER_FIELDS,
        *PRIMARY_COLOUR_FIELDS,
        *REQUIREMENT_OFFSETS,
        *DUAL_REQUIREMENT_OFFSETS,
        *SNAPSHOT_TRISTATE_FIELDS,
        *SYSTEM_TRISTATE_FIELDS,
    }
    unknown = sorted(str(key) for key in raw if key not in allowed)
    if unknown:
        raise ValueError(f"unproven or unsupported criteria: {', '.join(unknown)}")
    result: dict[str, object] = {}
    if "target_biome" in raw:
        value = raw["target_biome"]
        if value != "Any" and value not in BIOMES:
            raise ValueError(f"target_biome must be Any or one of {', '.join(BIOMES)}")
        result["target_biome"] = value
    if "star_type" in raw:
        value = raw["star_type"]
        if value != "Any" and value not in STAR_TYPES:
            raise ValueError(f"star_type must be Any or one of {', '.join(STAR_TYPES)}")
        result["star_type"] = value
    if "minimum_planets" in raw:
        value = raw["minimum_planets"]
        if not isinstance(value, int) or isinstance(value, bool) or not 1 <= value <= 8:
            raise ValueError("minimum_planets must be an integer from 1 through 8")
        result["minimum_planets"] = value
    if "wealth_class" in raw:
        value = raw["wealth_class"]
        if value != "Any" and value not in WEALTH_CLASSES:
            raise ValueError(
                f"wealth_class must be Any or one of {', '.join(WEALTH_CLASSES)}"
            )
        result["wealth_class"] = value
    if "trading_class" in raw:
        value = raw["trading_class"]
        if value != "Any" and value not in TRADING_CLASSES:
            raise ValueError(
                f"trading_class must be Any or one of {', '.join(TRADING_CLASSES)}"
            )
        result["trading_class"] = value
    if "conflict_level" in raw:
        value = raw["conflict_level"]
        if value != "Any" and value not in CONFLICT_LEVELS:
            raise ValueError(
                f"conflict_level must be Any or one of {', '.join(CONFLICT_LEVELS)}"
            )
        result["conflict_level"] = value
    if "population_state" in raw:
        value = raw["population_state"]
        if value != "Any" and value not in POPULATION_STATES:
            raise ValueError(
                f"population_state must be Any or one of {', '.join(POPULATION_STATES)}"
            )
        result["population_state"] = value
    if "biome_subtype" in raw:
        value = raw["biome_subtype"]
        if value != "Any" and value not in PROVEN_BIOME_SUBTYPES:
            raise ValueError(
                f"biome_subtype must be Any or one of {', '.join(PROVEN_BIOME_SUBTYPES)}"
            )
        result["biome_subtype"] = value
    if "anomaly" in raw:
        value = raw["anomaly"]
        if value != "Any" and value not in ANOMALIES:
            raise ValueError(f"anomaly must be Any or one of {', '.join(ANOMALIES)}")
        result["anomaly"] = value
    if "race" in raw:
        value = raw["race"]
        if value != "Any" and value not in PROVEN_RACES:
            raise ValueError(f"race must be Any or one of {', '.join(PROVEN_RACES)}")
        result["race"] = value
    if "planet_size" in raw:
        value = raw["planet_size"]
        if value != "Any" and value not in PLANET_SIZES:
            raise ValueError(
                f"planet_size must be Any or one of {', '.join(PLANET_SIZES)}"
            )
        result["planet_size"] = value
    if "terrain_setting" in raw:
        value = raw["terrain_setting"]
        if value != "Any" and value not in TERRAIN_SETTINGS:
            raise ValueError(
                f"terrain_setting must be Any or one of {', '.join(TERRAIN_SETTINGS)}"
            )
        result["terrain_setting"] = value
    for field, values in (
        ("life_level", PLANET_LIFE_LEVELS),
        ("creature_life_level", PLANET_LIFE_LEVELS),
        ("building_density", BUILDING_DENSITY_LEVELS),
        ("resource_level", RESOURCE_LEVELS),
    ):
        if field not in raw:
            continue
        value = raw[field]
        if value != "Any" and value not in values:
            raise ValueError(f"{field} must be Any or one of {', '.join(values)}")
        result[field] = value
    if "paradise_planet" in raw:
        value = raw["paradise_planet"]
        if value not in {"Any", "Require", "Exclude"}:
            raise ValueError("paradise_planet must be Any, Require, or Exclude")
        result["paradise_planet"] = value
    if "resource" in raw:
        value = raw["resource"]
        if value != "Any" and value not in RESOURCE_IDS:
            raise ValueError(
                f"resource must be Any or one of {', '.join(RESOURCE_IDS)}"
            )
        result["resource"] = value
    if "required_resources" in raw:
        value = raw["required_resources"]
        if not isinstance(value, (list, tuple)) or isinstance(value, (str, bytes)):
            raise ValueError("required_resources must be a list of resource IDs")
        if not 1 <= len(value) <= 3:
            raise ValueError(
                "required_resources must contain from 1 through 3 resource IDs"
            )
        if any(resource not in RESOURCE_IDS for resource in value):
            raise ValueError(
                f"required_resources entries must be one of {', '.join(RESOURCE_IDS)}"
            )
        if len(set(value)) != len(value):
            raise ValueError("required_resources must not contain duplicates")
        result["required_resources"] = list(value)
    for field, values in (
        ("storm_frequency", STORM_FREQUENCIES),
        ("weather_intensity", WEATHER_INTENSITIES),
        ("weather_type", WEATHER_TYPES),
        ("weather_conditions", WEATHER_CONDITIONS),
    ):
        if field not in raw:
            continue
        value = raw[field]
        if value != "Any" and value not in values:
            raise ValueError(f"{field} must be Any or one of {', '.join(values)}")
        result[field] = value
    for field in PRIMARY_COLOUR_FIELDS:
        if field not in raw:
            continue
        value = raw[field]
        if value != "Any" and value not in HUE_FAMILIES:
            raise ValueError(f"{field} must be Any or one of {', '.join(HUE_FAMILIES)}")
        result[field] = value
    for field in REQUIREMENT_OFFSETS:
        if field in SNAPSHOT_TRISTATE_FIELDS or field in SYSTEM_TRISTATE_FIELDS:
            continue
        if field not in raw:
            continue
        value = raw[field]
        if value not in {"Any", "Require"}:
            raise ValueError(f"{field} must be Any or Require")
        result[field] = value
    for field in DUAL_REQUIREMENT_OFFSETS:
        if field not in raw:
            continue
        value = raw[field]
        if value not in {"Any", "Require", "Exclude"}:
            raise ValueError(f"{field} must be Any, Require, or Exclude")
        result[field] = value
    for field in SNAPSHOT_TRISTATE_FIELDS:
        if field not in raw:
            continue
        value = raw[field]
        if value not in {"Any", "Require", "Exclude"}:
            raise ValueError(f"{field} must be Any, Require, or Exclude")
        result[field] = value
    for field in SYSTEM_TRISTATE_FIELDS:
        if field not in raw:
            continue
        value = raw[field]
        if value not in {"Any", "Require", "Exclude"}:
            raise ValueError(f"{field} must be Any, Require, or Exclude")
        result[field] = value
    return result


def apply_criteria(event_raw: bytes, criteria: Mapping[str, object]) -> bytes:
    """Return an owned event copy with only proven primitive fields changed."""

    if len(event_raw) != EVENT_SIZE:
        raise ValueError(f"event record must be exactly {EVENT_SIZE} bytes")
    normalized = normalize_criteria(criteria)
    owned = bytearray(event_raw)
    # R08 is storage, not a semantic default. Neutralize all product-owned guarded fields first.
    struct.pack_into("<B", owned, 0x9F, 0)  # NeedsBiome
    struct.pack_into("<B", owned, 0xBB, 0)  # UseStarType
    struct.pack_into("<B", owned, 0xBC, 0)  # UseTradingClass
    struct.pack_into("<B", owned, 0xBD, 0)  # UseWealthClass
    struct.pack_into("<I", owned, 0x90, 1)  # Default means no conflict comparison
    struct.pack_into("<B", owned, 0x9E, 0)  # NeedsAbandonedSystem
    struct.pack_into("<B", owned, 0xA2, 0)  # NeedsEmptySystem
    struct.pack_into("<B", owned, 0xA9, 0)  # NeverAllowAbandoned
    struct.pack_into("<B", owned, 0xAA, 0)  # NeverAllowEmpty
    struct.pack_into("<I", owned, 0x78, 0xFFFFFFFF)  # MinPlanets disabled
    struct.pack_into("<I", owned, 0x88, 0)  # No anomaly comparison
    struct.pack_into("<I", owned, 0x8C, 0)  # No biome-subtype comparison
    struct.pack_into("<I", owned, 0x94, 7)  # Race None means no comparison
    for offset in REQUIREMENT_OFFSETS.values():
        struct.pack_into("<B", owned, offset, 0)
    for require_offset, exclude_offset in DUAL_REQUIREMENT_OFFSETS.values():
        struct.pack_into("<B", owned, require_offset, 0)
        struct.pack_into("<B", owned, exclude_offset, 0)
    if normalized.get("target_biome") not in {None, "Any"}:
        struct.pack_into("<I", owned, 0x7C, BIOMES.index(normalized["target_biome"]))
        struct.pack_into("<B", owned, 0x9F, 1)
    if normalized.get("star_type") not in {None, "Any"}:
        struct.pack_into("<I", owned, 0x84, STAR_TYPES.index(normalized["star_type"]))
        struct.pack_into("<B", owned, 0xBB, 1)
    if "minimum_planets" in normalized:
        struct.pack_into("<I", owned, 0x78, normalized["minimum_planets"])
    if normalized.get("wealth_class") not in {None, "Any"}:
        struct.pack_into(
            "<I", owned, 0x74, WEALTH_CLASSES.index(normalized["wealth_class"])
        )
        struct.pack_into("<B", owned, 0xBD, 1)
    if normalized.get("trading_class") not in {None, "Any"}:
        struct.pack_into(
            "<I", owned, 0x70, TRADING_CLASSES.index(normalized["trading_class"])
        )
        struct.pack_into("<B", owned, 0xBC, 1)
    if normalized.get("conflict_level") not in {None, "Any"}:
        struct.pack_into(
            "<I", owned, 0x90, CONFLICT_LEVELS[normalized["conflict_level"]]
        )
    population_state = normalized.get("population_state")
    if population_state == "Inhabited":
        struct.pack_into("<B", owned, 0xA9, 1)
        struct.pack_into("<B", owned, 0xAA, 1)
    elif population_state == "Empty":
        struct.pack_into("<B", owned, 0xA2, 1)
    elif population_state == "Abandoned":
        struct.pack_into("<B", owned, 0x9E, 1)
    biome_subtype = normalized.get("biome_subtype")
    if biome_subtype not in {None, "Any"}:
        struct.pack_into("<I", owned, 0x8C, BIOME_SUBTYPES.index(biome_subtype) + 1)
    anomaly = normalized.get("anomaly")
    if anomaly not in {None, "Any"}:
        struct.pack_into("<I", owned, 0x88, ANOMALIES.index(anomaly) + 1)
    race = normalized.get("race")
    if race not in {None, "Any"}:
        struct.pack_into("<I", owned, 0x94, RACE_VALUES[race])
    for field, offset in REQUIREMENT_OFFSETS.items():
        if normalized.get(field) == "Require":
            struct.pack_into("<B", owned, offset, 1)
    for field, (require_offset, exclude_offset) in DUAL_REQUIREMENT_OFFSETS.items():
        value = normalized.get(field)
        if value == "Require":
            struct.pack_into("<B", owned, require_offset, 1)
        elif value == "Exclude":
            struct.pack_into("<B", owned, exclude_offset, 1)
    return bytes(owned)


def supplemental_system_passes(criteria: Mapping[str, object]) -> tuple[str, ...]:
    """Return opt-in native categories needed to implement truthful Any semantics.

    The native evaluator's default path excludes pirate, empty, and abandoned systems. Those are
    separate exact passes when no economy selector rules them out. An explicit Inhabited query
    still needs Pirate because pirate systems are inhabited.
    """

    normalized = normalize_criteria(criteria)
    unconstrained_economy = (
        normalized.get("wealth_class", "Any") == "Any"
        and normalized.get("conflict_level", "Any") == "Any"
        and normalized.get("trading_class", "Any") == "Any"
        and normalized.get("race", "Any") == "Any"
        and normalized.get("anomaly", "Any") == "Any"
    )
    if not unconstrained_economy:
        return ()
    population_state = normalized.get("population_state", "Any")
    if population_state == "Inhabited":
        return ("Pirate",)
    if population_state == "Any":
        return ("Pirate", "Empty", "Abandoned")
    return ()

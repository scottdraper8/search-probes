from __future__ import annotations

from collections.abc import Mapping

from . import resident_search_presets as _PRESETS

SYSTEM_SEARCH_BUDGETS = _PRESETS.SYSTEM_SEARCH_BUDGETS

FORM_CRITERIA_FIELDS = (
    "target_biome",
    "biome_subtype",
    "planet_size",
    "terrain_setting",
    "resource",
    "floating_islands",
    "non_gas_giant_planet",
    "planet_rings",
    "planet_has_moons",
    "water_planet",
    "deep_water_planet",
    "paradise_planet",
    "life_level",
    "creature_life_level",
    "building_density",
    "resource_level",
    "weather_conditions",
    "weather_type",
    "extreme_hazard_planet",
    "extreme_sentinel_planet",
    "corrupt_sentinel_planet",
    "sentinels",
    "prime_planet",
    "infested_planet",
    "normal_planet",
    "relic_planet",
    "rgb_planet",
    "has_scrap",
    "suitable_creature_discovery",
    "suitable_weird_creature_discovery",
    "suitable_creature_taming",
    "suitable_robot_creature_discovery",
    "grass_primary_hue",
    "plant_primary_hue",
    "leaf_primary_hue",
    "water_primary_hue",
    "sky_primary_hue",
    "horizon_primary_hue",
    "sky_fog_primary_hue",
    "sky_height_fog_primary_hue",
    "star_type",
    "wealth_class",
    "trading_class",
    "conflict_level",
    "population_state",
    "race",
    "anomaly",
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
)
FORM_WIDGET_FIELDS = tuple(
    "population_label" if field == "population_state" else field
    for field in FORM_CRITERIA_FIELDS
) + ("minimum_planets", "resource_label_1", "resource_label_2", "resource_label_3")
BUILTIN_PRESETS = _PRESETS.DEFAULT_PRESETS
MAX_PRESETS = _PRESETS.MAX_PRESETS
PROVEN_PRESETS = BUILTIN_PRESETS
POPULATION_LABELS = {
    "Settled / inhabited": "Inhabited",
    "Uncharted / empty": "Empty",
    "Abandoned": "Abandoned",
}


def split_criteria(
    criteria: Mapping[str, object], resource_labels: Mapping[str, str] | None = None
) -> tuple[dict[str, str], dict[str, object]]:
    values = criteria_form_values(criteria, resource_labels or {})
    extras = {
        key: value
        for key, value in criteria.items()
        if key not in FORM_WIDGET_FIELDS
        and key not in {"population_state", "required_resources"}
    }
    return values, extras


def merge_criteria(
    values: Mapping[str, object],
    extras: Mapping[str, object],
) -> dict[str, object]:
    result = {
        key: value for key, value in extras.items() if key not in FORM_WIDGET_FIELDS
    }
    result.update(build_criteria(values))
    return result


def preset_form_values(name: str) -> dict[str, str]:
    if name not in BUILTIN_PRESETS:
        raise ValueError(f"unknown preset: {name}")
    return criteria_form_values(BUILTIN_PRESETS[name], {})


def criteria_form_values(
    criteria: Mapping[str, object], resource_labels: Mapping[str, str]
) -> dict[str, str]:
    result = dict.fromkeys(FORM_WIDGET_FIELDS, "Any")
    population_values = {value: label for label, value in POPULATION_LABELS.items()}
    for field, value in criteria.items():
        if field == "population_state":
            result["population_label"] = population_values.get(str(value), "Any")
        elif field == "required_resources":
            if not isinstance(value, (list, tuple)):
                raise TypeError("stored required_resources must be a sequence")
            for index, resource in enumerate(value[:3], 1):
                result[f"resource_label_{index}"] = resource_labels.get(
                    str(resource), str(resource)
                )
        elif field in result:
            result[field] = str(value)
    return result


def build_criteria(values: Mapping[str, object]) -> dict[str, object]:
    result: dict[str, object] = {}
    for field in FORM_CRITERIA_FIELDS:
        value = values.get(field, "Any")
        if value != "Any":
            result[field] = value
    required_resources = values.get("required_resources", ())
    if isinstance(required_resources, (list, tuple)):
        selected = list(
            dict.fromkeys(value for value in required_resources if value != "Any")
        )
        if selected:
            result["required_resources"] = selected
    minimum_planets = values.get("minimum_planets", "Any")
    if minimum_planets != "Any":
        result["minimum_planets"] = int(minimum_planets)
    return result

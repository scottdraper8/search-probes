from __future__ import annotations

from search_probes import system_query_snapshot as _SNAPSHOT
from search_probes.native_layout import MAX_SURVEY_UNIQUE_VALUES


def record_survey_snapshot(
    state: dict[str, object],
    candidate: dict[str, object],
    snapshot: dict[str, object],
) -> None:
    counts = state["counts"]
    samples = state["samples"]
    if not isinstance(counts, dict) or not isinstance(samples, dict):
        raise TypeError("corrupt survey aggregates")

    def record(category: str, value: object, planet: int | None = None) -> None:
        category_counts = counts.setdefault(category, {})
        category_samples = samples.setdefault(category, {})
        if not isinstance(category_counts, dict) or not isinstance(
            category_samples, dict
        ):
            raise TypeError("corrupt survey category")
        key = str(value)
        if (
            key not in category_counts
            and len(category_counts) >= MAX_SURVEY_UNIQUE_VALUES
        ):
            key = "<unique-value-overflow>"
        category_counts[key] = int(category_counts.get(key, 0)) + 1
        category_samples.setdefault(
            key,
            {
                "address": candidate["address"],
                "rank": candidate["rank"],
                **({"planet": planet} if planet is not None else {}),
            },
        )

    for field in (
        "planet_count",
        "star_type",
        "wealth_class",
        "trading_class",
        "conflict_level",
        "race",
        "population_state",
        "system_class",
        "has_giant_planet",
        "has_gas_giant",
        "has_non_gas_giant",
        "has_waterworld",
        "has_water_planet",
        "has_deep_water_planet",
        "has_weird_planet",
        "has_infested_planet",
        "has_normal_planet",
        "has_relic_planet",
        "has_rgb_planet",
        "has_corrupt_sentinel_planet",
        "has_extreme_storm_planet",
        "is_pirate",
    ):
        record(field, snapshot[field])
    native_diagnostics = snapshot.get("native_diagnostics")
    if isinstance(native_diagnostics, dict):
        for name, raw_diagnostic in native_diagnostics.items():
            if not isinstance(raw_diagnostic, dict):
                raise TypeError("native survey diagnostic must be a mapping")
            record(f"native_{name}", bool(raw_diagnostic["native"]))
            if "matches" in raw_diagnostic:
                record(f"native_generated_parity_{name}", raw_diagnostic["matches"])
    name_samples = state["name_samples"]
    if not isinstance(name_samples, list):
        raise TypeError("corrupt survey name samples")
    system_name = str(snapshot.get("system_name", ""))
    record("system_name_empty", not bool(system_name))
    planets = snapshot["planets"]
    if not isinstance(planets, tuple):
        raise TypeError("corrupt decoded planets")
    for planet in planets:
        if not isinstance(planet, dict):
            raise TypeError("corrupt decoded planet")
        planet_index = int(planet["index"])
        state["planets_checked"] = int(state["planets_checked"]) + 1
        for field in (
            "biome",
            "biome_subtype",
            "size",
            "terrain_setting_index",
            "terrain_setting",
            "has_floating_islands",
            "has_final_spawn_floating_island_objects",
            "has_rings",
            "has_water",
            "has_deep_water",
            "has_extreme_sentinels",
            "has_corrupt_sentinels",
            "has_extreme_weather",
            "has_extreme_hazard",
            "is_prime",
            "is_infested",
            "is_normal",
            "is_relic",
            "is_rgb",
            "has_sentinels",
            "suitable_creature_discovery",
            "suitable_weird_creature_discovery",
            "suitable_creature_taming",
            "suitable_robot_creature_discovery",
        ):
            record(field, planet[field], planet_index)
        record(
            "biome_and_subtype",
            f"{planet['biome']}:{planet['biome_subtype']}",
            planet_index,
        )
        record(
            "terrain_and_object_floating_islands",
            f"terrain={bool(planet['has_floating_islands'])};"
            f"objects={bool(planet['has_final_spawn_floating_island_objects'])}",
            planet_index,
        )
        record(
            "is_non_gas_giant",
            planet["size"] == _SNAPSHOT.PLANET_SIZES.index("Giant")
            and planet["biome"] != _SNAPSHOT.BIOMES.index("GasGiant"),
            planet_index,
        )
        weather = planet.get("weather")
        if isinstance(weather, dict):
            for field in ("storm_frequency", "weather_intensity", "weather_type"):
                record(field, weather[field], planet_index)
            record(
                "query_storm_frequency_matches",
                planet.get("query_storm_frequency_matches", False),
                planet_index,
            )
        primary_hues = planet.get("primary_hues")
        if isinstance(primary_hues, dict):
            for palette, hue in primary_hues.items():
                record(f"primary_hue_{palette}", hue, planet_index)
        record("has_moons", bool(planet["moon_indices"]), planet_index)
        for resource in planet["resources"]:
            record("resource", resource, planet_index)
        for field in (
            "life_level",
            "creature_life_level",
            "building_density",
            "resource_level",
            "sentinel_level",
            "inhabiting_race",
            "is_paradise",
            "has_scrap",
        ):
            if field in planet:
                record(field, planet[field], planet_index)
        spawn_headers = planet.get("spawn_headers")
        if isinstance(spawn_headers, dict):
            for collection, raw_header in spawn_headers.items():
                if not isinstance(raw_header, dict):
                    raise TypeError("captured spawn header must be a mapping")
                record(
                    f"spawn_header_{collection}_count",
                    int(raw_header["size"]),
                    planet_index,
                )
        planet_name = str(planet.get("name", ""))
        record("planet_name_empty", not bool(planet_name), planet_index)
        planet_info = planet.get("planet_info")
        if isinstance(planet_info, dict):
            for field in (
                "fauna",
                "flora",
                "planet_description",
                "planet_type",
                "resources",
                "weather",
                "is_weather_extreme",
                "special_fauna",
            ):
                record(f"planet_info_{field}", planet_info.get(field, ""), planet_index)
            record(
                "paradise_and_description",
                f"predicate={bool(planet.get('is_paradise'))};"
                f"description={planet_info.get('planet_description', '')}",
                planet_index,
            )
            if "life_level" in planet:
                record(
                    "life_and_flora_description",
                    f"life={planet['life_level']};flora={planet_info.get('flora', '')}",
                    planet_index,
                )
            if "creature_life_level" in planet:
                record(
                    "creature_life_and_fauna_description",
                    f"creature_life={planet['creature_life_level']};"
                    f"fauna={planet_info.get('fauna', '')}",
                    planet_index,
                )
            if "resource_level" in planet:
                record(
                    "resource_level_and_description",
                    f"resource_level={planet['resource_level']};"
                    f"resources={planet_info.get('resources', '')}",
                    planet_index,
                )
            sentinel_descriptions = planet_info.get("sentinels_per_difficulty", [])
            if isinstance(sentinel_descriptions, list) and "sentinel_level" in planet:
                record(
                    "sentinel_level_and_description",
                    f"sentinel_level={planet['sentinel_level']};"
                    f"descriptions={'|'.join(str(value) for value in sentinel_descriptions)}",
                    planet_index,
                )
        if len(name_samples) < 32 and (system_name or planet_name):
            name_samples.append(
                {
                    "address": candidate["address"],
                    "rank": candidate["rank"],
                    "planet": planet_index,
                    "system_name": system_name,
                    "planet_name": planet_name,
                }
            )

        spawn_data = planet.get("spawn_data")
        if not isinstance(spawn_data, dict):
            continue
        has_floating_island_spawn = False
        for collection, raw_records in spawn_data.items():
            if not isinstance(raw_records, list):
                raise TypeError("captured spawn collection must be a list")
            record(f"spawn_{collection}_count", len(raw_records), planet_index)
            for raw_record in raw_records:
                if not isinstance(raw_record, dict):
                    raise TypeError("captured spawn record must be a mapping")
                if collection == "object_list_sources":
                    record(
                        "object_list_source_name",
                        raw_record.get("name", ""),
                        planet_index,
                    )
                    options = raw_record.get("options", [])
                    if not isinstance(options, list):
                        raise TypeError("object-list source options must be a list")
                    for option in options:
                        record(
                            "object_list_source_option",
                            option,
                            planet_index,
                        )
                    continue
                if collection == "selectable_objects":
                    record(
                        "selectable_list_name", raw_record.get("name", ""), planet_index
                    )
                    resources = raw_record.get("objects", [])
                    if not isinstance(resources, list):
                        raise TypeError("captured selectable objects must be a list")
                    for resource in resources:
                        if isinstance(resource, dict):
                            record(
                                "selectable_resource_filename",
                                resource.get("filename", ""),
                                planet_index,
                            )
                    continue
                if collection == "creatures":
                    record(
                        "creature_id", raw_record.get("creature_id", ""), planet_index
                    )
                    record(
                        "creature_filter", raw_record.get("filter", ""), planet_index
                    )
                    for resource_field in (
                        "resource",
                        "female_resource",
                        "extra_resource",
                    ):
                        resource = raw_record.get(resource_field)
                        if isinstance(resource, dict):
                            record(
                                f"creature_{resource_field}_filename",
                                resource.get("filename", ""),
                                planet_index,
                            )
                    continue
                record(
                    f"spawn_{collection}_debug_name",
                    raw_record.get("debug_name", ""),
                    planet_index,
                )
                record(
                    f"spawn_{collection}_placement",
                    raw_record.get("placement", ""),
                    planet_index,
                )
                resource = raw_record.get("resource")
                if isinstance(resource, dict):
                    record(
                        f"spawn_{collection}_resource_filename",
                        resource.get("filename", ""),
                        planet_index,
                    )
                record(
                    f"spawn_{collection}_is_floating_island",
                    raw_record.get("is_floating_island", False),
                    planet_index,
                )
                if raw_record.get("is_floating_island"):
                    has_floating_island_spawn = True
                    record(
                        "floating_island_spawn_resource",
                        resource.get("filename", "")
                        if isinstance(resource, dict)
                        else "",
                        planet_index,
                    )
        record("has_floating_island_spawn", has_floating_island_spawn, planet_index)
        if has_floating_island_spawn != bool(
            planet["has_final_spawn_floating_island_objects"]
        ):
            raise RuntimeError(
                "deep spawn copy disagrees with bounded floating-island scan"
            )
        record(
            "terrain_and_spawn_floating",
            f"terrain={bool(planet['has_floating_islands'])};spawn="
            f"{has_floating_island_spawn}",
            planet_index,
        )

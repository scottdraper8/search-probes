from __future__ import annotations

from collections.abc import Mapping


def format_result_label(match: Mapping[str, object], index: int) -> str:
    summary = match.get("summary", {})
    system = summary.get("system", {}) if isinstance(summary, Mapping) else {}
    planet = summary.get("planet", {}) if isinstance(summary, Mapping) else {}
    if not isinstance(system, Mapping) or not isinstance(planet, Mapping):
        system, planet = {}, {}
    generated_system_name = str(system.get("name", "")).strip()
    if not planet:
        name = generated_system_name or "Unnamed generated system"
        return (
            f"{index + 1}. {name} · {system.get('star_type', 'Unknown star')} · "
            f"{system.get('wealth_class', 'Unknown economy')} · "
            f"{system.get('conflict_level', 'Unknown conflict')} · "
            f"rank {match.get('rank', '?')}"
        )
    target = str(match.get("evaluation_pass", "")).partition("planet=")[2]
    planet_number = (
        int(planet.get("index", target)) + 1
        if str(planet.get("index", target)).isdigit()
        else "?"
    )
    attributes = [
        str(planet.get("biome", "Unknown biome")),
        str(planet.get("size", "Unknown size")),
        str(system.get("star_type", "Unknown star")),
        str(system.get("wealth_class", "Unknown economy")),
        str(system.get("conflict_level", "Unknown conflict")),
    ]
    generated_name = str(planet.get("name", "")).strip()
    name_prefix = f"{generated_name} · " if generated_name else ""
    return (
        f"{index + 1}. {name_prefix}{' / '.join(attributes[:2])} · {attributes[2]} · "
        f"{attributes[3]} · {attributes[4]} · rank {match.get('rank', '?')} · "
        f"planet {planet_number}"
    )


def format_result_details(
    match: Mapping[str, object], resource_labels: Mapping[str, str]
) -> str:
    galaxy_number = match.get("galaxy_number")
    galaxy_text = (
        f"Galaxy: #{galaxy_number}\n" if isinstance(galaxy_number, int) else ""
    )
    summary = match.get("summary", {})
    system = summary.get("system", {}) if isinstance(summary, Mapping) else {}
    planet = summary.get("planet", {}) if isinstance(summary, Mapping) else {}
    if not isinstance(system, Mapping) or not isinstance(planet, Mapping):
        return f"Portal address: {match.get('address', 'unknown')}"
    generated_system_name = str(system.get("name", "")).strip() or "not captured"
    system_text = (
        f"Generated system name: {generated_system_name}\n"
        f"System: {system.get('star_type', '?')} star · "
        f"{system.get('wealth_class', '?')} economy · "
        f"{system.get('trading_class', '?')} · {system.get('conflict_level', '?')} conflict · "
        f"{system.get('population_state', '?')} · {system.get('race', '?')} · "
        f"pirate {'yes' if system.get('is_pirate') else 'no'}\n"
        f"System contains: giant {'yes' if system.get('has_giant_planet') else 'no'}, "
        f"gas giant {'yes' if system.get('has_gas_giant') else 'no'}, "
        f"non-gas giant {'yes' if system.get('has_non_gas_giant') else 'no'}, "
        f"waterworld {'yes' if system.get('has_waterworld') else 'no'}, "
        f"wet planet {'yes' if system.get('has_water_planet') else 'no'}, "
        f"deep water {'yes' if system.get('has_deep_water_planet') else 'no'}, "
        f"weird planet {'yes' if system.get('has_weird_planet') else 'no'}, "
        f"infested planet {'yes' if system.get('has_infested_planet') else 'no'}, "
        f"ordinary planet {'yes' if system.get('has_normal_planet') else 'no'}, "
        f"relic planet {'yes' if system.get('has_relic_planet') else 'no'}, "
        f"RGB biome {'yes' if system.get('has_rgb_planet') else 'no'}, "
        f"corrupt sentinels "
        f"{'yes' if system.get('has_corrupt_sentinel_planet') else 'no'}, "
        f"extreme storms {'yes' if system.get('has_extreme_storm_planet') else 'no'}"
    )
    if not planet:
        return f"Portal address: {match.get('address', 'unknown')}\n{galaxy_text}{system_text}"
    resources = planet.get("resources", [])
    if not isinstance(resources, list):
        resources = []
    resource_text = ", ".join(
        resource_labels.get(str(value), str(value)) for value in resources
    )
    primary_hues = planet.get("primary_hues", {})
    if not isinstance(primary_hues, Mapping):
        primary_hues = {}
    hue_text = ", ".join(f"{name}: {value}" for name, value in primary_hues.items())
    generated_name = str(planet.get("name", "")).strip() or "not captured"
    metadata = []
    if "is_paradise" in planet:
        metadata.append(f"paradise {'yes' if planet.get('is_paradise') else 'no'}")
    for key, label in (
        ("life_level", "generated life"),
        ("creature_life_level", "generated creatures"),
        ("building_density", "generated buildings"),
        ("resource_level", "generated resources"),
    ):
        if key in planet:
            metadata.append(f"{label} {planet[key]}")
    metadata_text = " · ".join(metadata) or "not captured for this query"
    object_islands = planet.get("has_floating_island_objects")
    object_islands_text = (
        "yes"
        if object_islands is True
        else "no"
        if object_islands is False
        else "not captured"
    )
    return (
        f"Portal address: {match.get('address', 'unknown')}\n"
        f"{galaxy_text}"
        f"{system_text}\n"
        f"Planet {int(planet.get('index', 0)) + 1} — generated name: {generated_name}\n"
        f"Planet type: {planet.get('biome', '?')} / "
        f"{planet.get('biome_subtype', '?')} · {planet.get('size', '?')} · "
        f"terrain {planet.get('terrain_setting', '?')} · "
        f"floating-island terrain "
        f"{'yes' if planet.get('has_floating_islands') else 'no'} · "
        f"floating-island objects {object_islands_text} · "
        f"rings {'yes' if planet.get('has_rings') else 'no'} · "
        f"moons {planet.get('moon_count', '?')} · "
        f"water {'yes' if planet.get('has_water') else 'no'} · "
        f"deep water {'yes' if planet.get('has_deep_water') else 'no'}\n"
        f"Generated environment: {metadata_text}\n"
        f"Weather: {planet.get('weather_type', 'not captured')} · "
        f"storm frequency {planet.get('storm_frequency', 'not captured')} · "
        f"intensity {planet.get('weather_intensity', 'not captured')}\n"
        f"Prime {'yes' if planet.get('is_prime') else 'no'} · "
        f"infested {'yes' if planet.get('is_infested') else 'no'} · "
        f"ordinary {'yes' if planet.get('is_normal') else 'no'} · "
        f"relic {'yes' if planet.get('is_relic') else 'no'} · "
        f"RGB {'yes' if planet.get('is_rgb') else 'no'} · "
        f"scrap {'yes' if planet.get('has_scrap') else 'no'} · "
        f"sentinels {'yes' if planet.get('has_sentinels') else 'no'}\n"
        f"Creature suitability: discovery "
        f"{'yes' if planet.get('suitable_creature_discovery') else 'no'}, "
        f"weird discovery {'yes' if planet.get('suitable_weird_creature_discovery') else 'no'}, "
        f"taming {'yes' if planet.get('suitable_creature_taming') else 'no'}, "
        f"robot discovery {'yes' if planet.get('suitable_robot_creature_discovery') else 'no'}\n"
        f"Resources: {resource_text or 'none in generated query'}\n"
        f"Primary palette hues: {hue_text or 'not captured for this query'}\n"
        f"Extreme: storms {'yes' if planet.get('has_extreme_weather') else 'no'}, "
        f"hazards {'yes' if planet.get('has_extreme_hazard') else 'no'}, "
        f"sentinels {'yes' if planet.get('has_extreme_sentinels') else 'no'}, "
        f"corrupt sentinels {'yes' if planet.get('has_corrupt_sentinels') else 'no'}"
    )

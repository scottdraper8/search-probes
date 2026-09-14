from __future__ import annotations

import xml.etree.ElementTree as ET

from .adapter_definitions import (
    REFERENCE_SCAN_EVENT_ID,
    SOURCE_MISSION_ID,
)
from .adapter_errors import AdapterError


def _child(parent: ET.Element, name: str) -> ET.Element:
    matches = [node for node in parent if node.attrib.get("name") == name]
    if len(matches) != 1:
        raise AdapterError(
            "probe_source_shape_changed",
            f"Expected one direct {name!r} property, found {len(matches)}",
        )
    return matches[0]


def _set(parent: ET.Element, name: str, value: str) -> None:
    _child(parent, name).set("value", value)


def _set_texture(parent: ET.Element, name: str, filename: str) -> None:
    _set(_child(parent, name), "Filename", filename)


def _replace_values(root: ET.Element, old: str, new: str) -> None:
    for node in root.iter():
        value = node.attrib.get("value")
        if value:
            node.set("value", value.replace(old, new))


def _source_mission(root: ET.Element) -> ET.Element:
    missions = _child(root, "Missions")
    candidates = []
    for candidate in missions:
        try:
            mission_id = _child(candidate, "MissionID").attrib.get("value")
        except AdapterError:
            continue
        if mission_id == SOURCE_MISSION_ID:
            candidates.append(candidate)
    if len(candidates) != 1:
        raise AdapterError(
            "probe_source_shape_changed",
            f"Expected one {SOURCE_MISSION_ID} mission, found {len(candidates)}",
        )
    return candidates[0]


def _reference_scan_event(root: ET.Element) -> ET.Element:
    candidates = [
        node
        for node in root.iter("Property")
        if node.attrib.get("name") == "ScanEvents"
        and node.attrib.get("value") == "GcScanEventData"
        and any(
            child.attrib.get("name") == "Name"
            and child.attrib.get("value") == REFERENCE_SCAN_EVENT_ID
            for child in node
        )
    ]
    if len(candidates) != 1:
        raise AdapterError(
            "probe_source_shape_changed",
            f"Expected one {REFERENCE_SCAN_EVENT_ID} scan event, found {len(candidates)}",
        )
    event = candidates[0]
    checks = {
        "BuildingLocation": "PlanetSearch",
        "SearchType": "Any",
        "ForceWideRandom": "true",
        "MustFindSystem": "false",
        "SolarSystemLocation": "LocalOrNear",
    }
    for name, expected in checks.items():
        actual = _child(event, name).attrib.get("value")
        if actual != expected:
            raise AdapterError(
                "probe_source_shape_changed",
                f"{REFERENCE_SCAN_EVENT_ID} {name} changed from {expected!r} to {actual!r}",
            )
    building = _child(_child(event, "SpecificBuildingClass"), "BuildingClass")
    lookup = _child(event, "SolarSystemAttributes")
    if (
        building.attrib.get("value") != "None"
        or _child(lookup, "NeedsBiome").attrib.get("value") != "true"
        or _child(_child(lookup, "NeedsBiomeType"), "Biome").attrib.get("value")
        != "Lush"
    ):
        raise AdapterError(
            "probe_source_shape_changed",
            f"{REFERENCE_SCAN_EVENT_ID} is no longer a direct Lush-biome planet search",
        )
    return event


def _reference_near_scan_events(root: ET.Element) -> tuple[str, ...]:
    matches = []
    for event in root.iter("Property"):
        if (
            event.attrib.get("name") != "ScanEvents"
            or event.attrib.get("value") != "GcScanEventData"
        ):
            continue
        values = {
            child.attrib.get("name"): child.attrib.get("value") for child in event
        }
        if (
            values.get("SolarSystemLocation") == "Near"
            and values.get("ForceWideRandom") == "false"
            and values.get("MustFindSystem") == "false"
        ):
            name = values.get("Name")
            if name:
                matches.append(name)
    if not matches:
        raise AdapterError(
            "probe_source_shape_changed",
            "Current missions no longer contain a bounded Near scan-event reference",
        )
    return tuple(matches)

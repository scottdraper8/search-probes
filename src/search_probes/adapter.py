from __future__ import annotations

import re
import xml.etree.ElementTree as ET
from dataclasses import dataclass

from .adapter_definitions import (
    FORM_NAVIGATION_TARGET_EVENT_ID,
    FORM_NAVIGATION_TARGET_MISSION_ID,
    MAX_PROBE_TAG_LENGTH,
    MISSION_ID,
    REFERENCE_SCAN_EVENT_ID,
    SCAN_EVENT_ID,
    SOURCE_MISSION_ID,
)
from .adapter_errors import AdapterError
from .guide import _wiki_patch
from .localization import _localisation
from .mission_builder import _mission_patch, _move_mission
from .xml_helpers import _reference_near_scan_events, _reference_scan_event


@dataclass(frozen=True)
class AdapterComposition:
    source_mission_id: str
    mission_id: str
    scan_event_id: str
    form_navigation_target_mission_id: str
    form_navigation_target_event_id: str
    location_references: tuple[str, ...]
    files: tuple[tuple[str, ET.Element], ...]
    predicate: dict[str, object]
    search_policy: dict[str, object]
    targeting_reference: str
    quick_warp_present: bool


def probe_ids(probe_tag: str | None) -> tuple[str, str]:
    if probe_tag is None:
        return MISSION_ID, SCAN_EVENT_ID
    normalized = probe_tag.upper()
    if (
        not re.fullmatch(r"[A-Z0-9_]+", normalized)
        or len(normalized) > MAX_PROBE_TAG_LENGTH
    ):
        raise AdapterError(
            "invalid_probe_tag",
            "Probe tag must contain "
            f"1-{MAX_PROBE_TAG_LENGTH} ASCII letters, digits, or underscores",
            probe_tag=probe_tag,
        )
    return f"{MISSION_ID}_{normalized}", f"{SCAN_EVENT_ID}_{normalized}"


def compose_adapter(
    wiki_root: ET.Element,
    mission_root: ET.Element,
    reference_root: ET.Element,
    *,
    probe_tag: str | None = None,
    guide_preset_slots: int = 0,
) -> AdapterComposition:
    _reference_scan_event(reference_root)
    location_references = _reference_near_scan_events(reference_root)
    mission_id, scan_event_id = probe_ids(probe_tag)
    wiki_missions = _mission_patch(
        mission_root,
        mission_id,
        scan_event_id,
        guide_preset_slots=guide_preset_slots,
    )
    files: list[tuple[str, ET.Element]] = [
        ("METADATA/REALITY/WIKI.EXML", _wiki_patch(wiki_root, mission_id)),
        (
            "METADATA/SIMULATION/MISSIONS/TABLES/WIKIMISSIONTABLE.EXML",
            wiki_missions,
        ),
    ]
    if guide_preset_slots:
        files.append(
            (
                "METADATA/SIMULATION/MISSIONS/TABLES/NPCMISSIONTABLE.EXML",
                _move_mission(
                    wiki_missions,
                    reference_root.attrib["template"],
                    FORM_NAVIGATION_TARGET_MISSION_ID,
                ),
            )
        )
    files.append(("LocTable.MXML", _localisation()))
    return AdapterComposition(
        source_mission_id=SOURCE_MISSION_ID,
        mission_id=mission_id,
        scan_event_id=scan_event_id,
        form_navigation_target_mission_id=FORM_NAVIGATION_TARGET_MISSION_ID,
        form_navigation_target_event_id=FORM_NAVIGATION_TARGET_EVENT_ID,
        location_references=location_references,
        files=tuple(files),
        predicate={"NeedsBiome": True, "NeedsBiomeType": "Lush"},
        search_policy={
            "BuildingLocation": "PlanetSearch",
            "SearchType": "Any",
            "ForceWideRandom": False,
            "MustFindSystem": False,
            "SolarSystemLocation": "Near",
        },
        targeting_reference=REFERENCE_SCAN_EVENT_ID,
        quick_warp_present=False,
    )


__all__ = [
    "AdapterComposition",
    "compose_adapter",
]

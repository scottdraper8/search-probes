from __future__ import annotations

import copy
import xml.etree.ElementTree as ET

from .adapter_definitions import (
    FORM_NAVIGATION_TARGET_EVENT_ID,
    FORM_NAVIGATION_TARGET_MISSION_ID,
    FROM_LIST_PLACEHOLDER,
    GUIDE_MISSION_PREFIX,
    GUIDE_READY_EVENT_PREFIX,
    GUIDE_REQUEST_EVENT_PREFIX,
    GUIDE_TARGET_EVENT_PREFIX,
    LOCALISATION,
    MAX_GUIDE_PRESET_SLOTS,
    MISSION_ID,
    SCAN_EVENT_ID,
    SOURCE_MISSION_ID,
)
from .adapter_errors import AdapterError
from .mission_sequences import (
    _configure_deferred_scan_event_lifecycle,
    _configure_direct_scan_event_lifecycle,
    _configure_lookup,
)
from .xml_helpers import _child, _replace_values, _set, _source_mission


def _mission_patch(
    source_root: ET.Element,
    mission_id: str = MISSION_ID,
    scan_event_id: str = SCAN_EVENT_ID,
    *,
    guide_preset_slots: int = 0,
) -> ET.Element:
    if not 0 <= guide_preset_slots <= MAX_GUIDE_PRESET_SLOTS:
        raise ValueError(
            f"guide_preset_slots must be from 0 through {MAX_GUIDE_PRESET_SLOTS}"
        )

    def mission_entry(
        entry_mission_id: str,
        entry_scan_event_id: str,
        *,
        from_list_target: bool = False,
        deferred_event_ids: tuple[str, str, str] | None = None,
    ) -> ET.Element:
        mission = copy.deepcopy(_source_mission(source_root))
        mission.set("_id", entry_mission_id)
        mission.attrib.pop("_index", None)
        # Localisation keys stay shared across trials and dispatch slots; only state-bearing
        # mission and scan-event identities are unique.
        _replace_values(mission, "WIKI_MISSION_TRADE5", MISSION_ID)
        _replace_values(mission, SOURCE_MISSION_ID, entry_mission_id)
        if entry_mission_id != MISSION_ID:
            trial_localisation = {
                identifier.replace(MISSION_ID, entry_mission_id, 1): identifier
                for identifier in LOCALISATION
                if identifier.startswith(f"{MISSION_ID}_")
            }
            for node in mission.iter():
                value = node.attrib.get("value")
                if value in trial_localisation:
                    node.set("value", trial_localisation[value])
        # Preserve the current vanilla route-bearing source mission's class.  The Guide owns the
        # launch surface; the resulting objective remains an ordinary Secondary mission so NMS's
        # Log and Galaxy Map treat it like the source navigation contract.
        _set(mission, "MissionClass", "Secondary")
        _set(mission, "MessageComplete", "Never")
        _set(mission, "MessageStart", "Never")
        _set(mission, "CanRenounce", "true")

        scan_events = _child(mission, "ScanEvents")
        scan_event = _child(scan_events, "ScanEvents")
        _set(scan_event, "Name", entry_scan_event_id)
        _set(_child(scan_event, "ForceInteractionType"), "InteractionType", "None")
        _set(scan_event, "AlwaysShow", "true")
        # The route must survive entry into its target system so the following current-planet
        # condition can resolve it.  The inherited Wiki interaction lifecycle consumes the event
        # at system arrival and strands the mission before the planet stage.
        _set(scan_event, "EventEndType", "None")
        _set(scan_event, "DisableMultiplayerSync", "true")
        _set(scan_event, "ReplaceEventIfAlreadyActive", "true")
        _set(scan_event, "BuildingLocation", "PlanetSearch")
        _set(scan_event, "SearchType", "Any")
        _set(_child(scan_event, "SpecificBuildingClass"), "BuildingClass", "None")
        _set(scan_event, "ForceWideRandom", "false")
        _set(scan_event, "MustFindSystem", "false")
        _set(scan_event, "SolarSystemLocation", "Near")
        _set(scan_event, "OSDMessage", "SQN_SS_LUSH_OSD")
        _set(scan_event, "MarkerLabel", "SQN_SS_LUSH_MARKER")
        _configure_lookup(_child(scan_event, "SolarSystemAttributes"))
        _configure_lookup(_child(scan_event, "SolarSystemAttributesFallback"))
        if from_list_target:
            target_uas = _child(scan_event, "UAsList")
            target_uas[:] = []
            ET.SubElement(
                target_uas,
                "Property",
                {"name": "UAsList", "value": FROM_LIST_PLACEHOLDER},
            )
            _set(scan_event, "SolarSystemLocation", "FromList")
            _set(scan_event, "MustFindSystem", "true")

        stage_types = [
            node.attrib.get("value")
            for node in mission.iter()
            if node.attrib.get("name") == "Stage"
        ]
        expected = {
            "GcMissionSequenceGroup",
            "GcMissionSequenceStartScanEvent",
            "GcMissionSequenceGetToScanEvent",
        }
        if set(stage_types) != expected or any(
            "QuickWarp" in (value or "") for value in stage_types
        ):
            raise AdapterError(
                "probe_source_shape_changed",
                "Source mission no longer has the expected no-quick-warp stage set",
                stage_types=stage_types,
            )
        if deferred_event_ids is None:
            _configure_direct_scan_event_lifecycle(mission, entry_scan_event_id)
            for node in mission.iter("Property"):
                if node.attrib.get("name") == "Event":
                    node.set("value", entry_scan_event_id)
        else:
            request_event_id, ready_event_id, target_event_id = deferred_event_ids
            target_event = scan_event
            request_event = copy.deepcopy(target_event)
            ready_event = copy.deepcopy(target_event)
            for event, name in (
                (request_event, request_event_id),
                (ready_event, ready_event_id),
                (target_event, target_event_id),
            ):
                event.attrib.pop("_index", None)
                _set(event, "Name", name)
            # A fixed-size placeholder gives the resident runtime one owned string buffer to
            # overwrite before NMS resolves the target. The high-level resolver accepts exactly
            # 16 hexadecimal digits for SolarSystemLocation=FromList.
            target_uas = _child(target_event, "UAsList")
            target_uas[:] = []
            ET.SubElement(
                target_uas,
                "Property",
                {"name": "UAsList", "value": FROM_LIST_PLACEHOLDER},
            )
            _set(target_event, "SolarSystemLocation", "FromList")
            _set(target_event, "MustFindSystem", "true")
            _child(request_event, "UAsList")[:] = []
            _child(ready_event, "UAsList")[:] = []
            scan_events[:] = [request_event, ready_event, target_event]
            _configure_deferred_scan_event_lifecycle(
                mission,
                request_event_id=request_event_id,
                ready_event_id=ready_event_id,
                target_event_id=target_event_id,
            )
            for node in mission.iter("Property"):
                if node.attrib.get("name") == "Event" and node.attrib.get(
                    "value"
                ) not in {request_event_id, ready_event_id, target_event_id}:
                    node.set("value", target_event_id)
        return mission

    result = ET.Element("Data", {"template": source_root.attrib["template"]})
    container = ET.SubElement(result, "Property", {"name": "Missions"})
    container.append(mission_entry(mission_id, scan_event_id))
    if guide_preset_slots:
        container.append(
            mission_entry(
                FORM_NAVIGATION_TARGET_MISSION_ID,
                FORM_NAVIGATION_TARGET_EVENT_ID,
                from_list_target=True,
            )
        )
    for index in range(guide_preset_slots):
        container.append(
            mission_entry(
                f"{GUIDE_MISSION_PREFIX}{index:02d}",
                f"{GUIDE_TARGET_EVENT_PREFIX}{index:02d}",
                deferred_event_ids=(
                    f"{GUIDE_REQUEST_EVENT_PREFIX}{index:02d}",
                    f"{GUIDE_READY_EVENT_PREFIX}{index:02d}",
                    f"{GUIDE_TARGET_EVENT_PREFIX}{index:02d}",
                ),
            )
        )
    return result


def _move_mission(
    source_patch: ET.Element,
    destination_template: str,
    mission_id: str,
) -> ET.Element:
    """Move one generated mission into a second mission-table patch.

    Wiki topics can launch missions from WIKIMISSIONTABLE, but the ordinary native mission-start
    path resolves missions from NPCMISSIONTABLE.  Keep the Guide dispatch missions in their native
    Wiki registry and publish the form's disposable target in the registry its launcher uses.
    """

    source = _child(source_patch, "Missions")
    matches = [
        mission
        for mission in source
        if _child(mission, "MissionID").attrib.get("value") == mission_id
    ]
    if len(matches) != 1:
        raise AdapterError(
            "probe_source_shape_changed",
            f"Expected one generated {mission_id} mission, found {len(matches)}",
        )
    mission = matches[0]
    source.remove(mission)
    result = ET.Element("Data", {"template": destination_template})
    ET.SubElement(result, "Property", {"name": "Missions"}).append(mission)
    return result

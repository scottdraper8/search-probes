from __future__ import annotations

import copy
import xml.etree.ElementTree as ET

from .adapter_definitions import LOOKUP_BOOLEAN_FIELDS
from .adapter_errors import AdapterError
from .xml_helpers import _child, _set


def _configure_lookup(lookup: ET.Element) -> None:
    for name in LOOKUP_BOOLEAN_FIELDS:
        _set(lookup, name, "true" if name == "NeedsBiome" else "false")
    _set(_child(lookup, "UseRace"), "AlienRace", "None")
    _set(_child(lookup, "UseAnomaly"), "GalaxyStarAnomaly", "None")
    _set(_child(lookup, "UseConflict"), "ConflictLevel", "Default")
    _set(_child(lookup, "NeedsBiomeType"), "Biome", "Lush")
    _set(_child(lookup, "UseBiomeSubType"), "BiomeSubType", "None")
    _set(lookup, "NeedsResourceHint", "")
    _set(lookup, "NeedsSpecificCreature", "")
    _set(lookup, "SamePlanetAsEvent", "")
    _set(lookup, "SamePlanetAsSeasonParty", "0")
    _set(lookup, "SystemNeedsResourceHint", "")
    _set(lookup, "MinPlanets", "-1")


def _configure_direct_scan_event_lifecycle(mission: ET.Element, event_id: str) -> None:
    """Clear an old route, retain it through system arrival, then finish at its planet."""

    stage_containers: list[ET.Element] = []
    for container in mission.iter("Property"):
        if container.attrib.get("name") != "Stages" or container.attrib.get("value"):
            continue
        direct_types = [
            stage.attrib.get("value")
            for wrapper in container
            for stage in wrapper
            if stage.attrib.get("name") == "Stage"
        ]
        if (
            "GcMissionSequenceStartScanEvent" in direct_types
            and "GcMissionSequenceGetToScanEvent" in direct_types
        ):
            stage_containers.append(container)
    if len(stage_containers) != 1:
        raise AdapterError(
            "probe_source_shape_changed",
            "Expected one StartScanEvent/GetToScanEvent stage container, "
            f"found {len(stage_containers)}",
        )

    container = stage_containers[0]
    start_index = next(
        index
        for index, wrapper in enumerate(container)
        if any(
            child.attrib.get("name") == "Stage"
            and child.attrib.get("value") == "GcMissionSequenceStartScanEvent"
            for child in wrapper
        )
    )
    cleanup = copy.deepcopy(container[start_index])
    for child in list(cleanup):
        if child.attrib.get("name") != "Versions":
            cleanup.remove(child)
    stage = ET.SubElement(
        cleanup,
        "Property",
        {"name": "Stage", "value": "GcMissionSequenceEndScanEvent"},
    )
    payload = ET.SubElement(
        stage, "Property", {"name": "GcMissionSequenceEndScanEvent"}
    )
    ET.SubElement(payload, "Property", {"name": "Event", "value": event_id})
    ET.SubElement(
        payload,
        "Property",
        {"name": "DebugText", "value": "Clearing previous Search Probes target"},
    )
    container.insert(start_index, cleanup)

    get_to_index = next(
        index
        for index, wrapper in enumerate(container)
        if any(
            child.attrib.get("name") == "Stage"
            and child.attrib.get("value") == "GcMissionSequenceGetToScanEvent"
            for child in wrapper
        )
    )
    get_to = container[get_to_index]

    def wait_stage(
        *,
        message: str,
        condition_type: str,
        condition_fields: tuple[tuple[str, str], ...],
        debug_text: str,
    ) -> ET.Element:
        wrapper = copy.deepcopy(get_to)
        for child in list(wrapper):
            if child.attrib.get("name") != "Versions":
                wrapper.remove(child)
        stage = ET.SubElement(
            wrapper,
            "Property",
            {"name": "Stage", "value": "GcMissionSequenceWaitForConditions"},
        )
        payload = ET.SubElement(
            stage,
            "Property",
            {"name": "GcMissionSequenceWaitForConditions"},
        )
        ET.SubElement(payload, "Property", {"name": "Message", "value": message})
        condition_test = ET.SubElement(
            payload,
            "Property",
            {"name": "ConditionTest", "value": "GcMissionConditionTest"},
        )
        ET.SubElement(
            condition_test,
            "Property",
            {"name": "ConditionTest", "value": "AnyTrue"},
        )
        conditions = ET.SubElement(payload, "Property", {"name": "Conditions"})
        condition = ET.SubElement(
            conditions,
            "Property",
            {"name": "Conditions", "value": condition_type},
        )
        condition_payload = ET.SubElement(
            condition,
            "Property",
            {"name": condition_type},
        )
        for name, value in condition_fields:
            ET.SubElement(condition_payload, "Property", {"name": name, "value": value})
        ET.SubElement(
            payload,
            "Property",
            {"name": "AllowedToFormatObjectives", "value": "true"},
        )
        ET.SubElement(
            payload,
            "Property",
            {"name": "ForceAllowMissionRestart", "value": "false"},
        )
        ET.SubElement(
            payload,
            "Property",
            {"name": "ForceAllowMissionRestartEvent", "value": ""},
        )
        ET.SubElement(payload, "Property", {"name": "DebugText", "value": debug_text})
        markup = ET.SubElement(
            payload,
            "Property",
            {
                "name": "StatusMessageMissionMarkup",
                "value": "GcStatusMessageMissionMarkup",
            },
        )
        ET.SubElement(markup, "Property", {"name": "MissionMarkup", "value": "None"})
        return wrapper

    wait_for_system = wait_stage(
        message="SQN_SS_LUSH_SYSTEM_MSG",
        condition_type="GcMissionConditionIsScanEventLocal",
        condition_fields=(
            ("Event", event_id),
            ("BlockMissionRestart", "false"),
            ("RequiresFullFireteam", "false"),
        ),
        debug_text="Waiting for the search probe target system",
    )
    wait_for_planet = wait_stage(
        message="SQN_SS_LUSH_PLANET_MSG",
        condition_type="GcMissionConditionIsScanEventOnCurrentPlanet",
        condition_fields=(("Event", event_id), ("AllowInShip", "true")),
        debug_text="Waiting for the search probe target planet",
    )

    finish = copy.deepcopy(get_to)
    for child in list(finish):
        if child.attrib.get("name") != "Versions":
            finish.remove(child)
    stage = ET.SubElement(
        finish,
        "Property",
        {"name": "Stage", "value": "GcMissionSequenceEndScanEvent"},
    )
    payload = ET.SubElement(
        stage, "Property", {"name": "GcMissionSequenceEndScanEvent"}
    )
    ET.SubElement(payload, "Property", {"name": "Event", "value": event_id})
    ET.SubElement(
        payload,
        "Property",
        {"name": "DebugText", "value": "Completing the reached Search Probes target"},
    )

    complete = copy.deepcopy(get_to)
    for child in list(complete):
        if child.attrib.get("name") != "Versions":
            complete.remove(child)
    stage = ET.SubElement(
        complete,
        "Property",
        {"name": "Stage", "value": "GcMissionSequenceShowMissionUpdateMessage"},
    )
    payload = ET.SubElement(
        stage,
        "Property",
        {"name": "GcMissionSequenceShowMissionUpdateMessage"},
    )
    for name, value in (
        ("MissionUpdateMessage", "End"),
        ("CustomMessageLocID", ""),
        ("CustomObjectiveLocID", ""),
        ("SetMissionSelected", "false"),
        ("WaitForMessageOver", "false"),
        ("ShowChangeMissionNotify", "false"),
        ("SuppressNotificationsNotFromThisMission", "false"),
        ("PlayMusicSting", "End"),
        ("DebugText", "Completing the Search Probes mission"),
    ):
        ET.SubElement(payload, "Property", {"name": name, "value": value})

    container.remove(get_to)
    container.insert(get_to_index, wait_for_system)
    container.insert(get_to_index + 1, wait_for_planet)
    container.insert(get_to_index + 2, finish)
    container.insert(get_to_index + 3, complete)

    for progress, wrapper in enumerate(container, 1):
        wrapper.set("_index", str(progress - 1))
        versions = next(
            (child for child in wrapper if child.attrib.get("name") == "Versions"),
            None,
        )
        if versions is not None:
            for version in versions:
                _set(version, "Progress", str(progress))


def _configure_deferred_scan_event_lifecycle(
    mission: ET.Element,
    *,
    request_event_id: str,
    ready_event_id: str,
    target_event_id: str,
) -> None:
    """Gate one native FromList target until the resident search has supplied its address.

    The request event is deliberately consumed by the resident hook.  The ready event is an
    internal, local scan-event signal.  Crucially, the target event is then started by NMS's own
    GcMissionSequenceStartScanEvent stage; the DLL never publishes the navigation target itself.
    """

    _configure_direct_scan_event_lifecycle(mission, target_event_id)
    containers = [
        container
        for container in mission.iter("Property")
        if container.attrib.get("name") == "Stages"
        and not container.attrib.get("value")
        and any(
            child.attrib.get("name") == "Stage"
            and child.attrib.get("value") == "GcMissionSequenceStartScanEvent"
            for wrapper in container
            for child in wrapper
        )
    ]
    if len(containers) != 1:
        raise AdapterError(
            "probe_source_shape_changed",
            f"Expected one configured scan-event stage container, found {len(containers)}",
        )
    container = containers[0]
    target_start_index = next(
        index
        for index, wrapper in enumerate(container)
        if any(
            child.attrib.get("name") == "Stage"
            and child.attrib.get("value") == "GcMissionSequenceStartScanEvent"
            for child in wrapper
        )
    )
    target_start = container[target_start_index]
    request_start = copy.deepcopy(target_start)
    for node in request_start.iter("Property"):
        if node.attrib.get("name") == "Event":
            node.set("value", request_event_id)

    wait_for_system = container[target_start_index + 1]
    wait_for_ready = copy.deepcopy(wait_for_system)
    ready_conditions = [
        node
        for node in wait_for_ready.iter("Property")
        if node.attrib.get("name") == "Conditions"
        and node.attrib.get("value") == "GcMissionConditionIsScanEventLocal"
    ]
    if len(ready_conditions) != 1 or len(ready_conditions[0]) != 1:
        raise AdapterError(
            "probe_source_shape_changed",
            "Expected one local condition in the deferred ready gate",
        )
    ready_condition = ready_conditions[0]
    ready_condition.set("value", "GcMissionConditionIsScanEventActive")
    ready_payload = ready_condition[0]
    ready_payload.set("name", "GcMissionConditionIsScanEventActive")
    ready_payload[:] = []
    ET.SubElement(ready_payload, "Property", {"name": "Event", "value": ready_event_id})
    ET.SubElement(
        ready_payload,
        "Property",
        {"name": "MustMatchThisMissionIDSeed", "value": "true"},
    )
    for node in wait_for_ready.iter("Property"):
        if node.attrib.get("name") == "Message":
            node.set("value", "")
        elif node.attrib.get("name") == "DebugText":
            node.set("value", "Waiting for the resident search result")

    ready_end = copy.deepcopy(container[0])
    for node in ready_end.iter("Property"):
        if node.attrib.get("name") == "Event":
            node.set("value", ready_event_id)
        elif node.attrib.get("name") == "DebugText":
            node.set("value", "Clearing the Search Probes ready signal")
    initial_ready_end = copy.deepcopy(ready_end)

    container.insert(0, initial_ready_end)
    target_start_index += 1
    container.insert(target_start_index, request_start)
    container.insert(target_start_index + 1, wait_for_ready)
    container.insert(target_start_index + 2, ready_end)

    for progress, wrapper in enumerate(container, 1):
        wrapper.set("_index", str(progress - 1))
        versions = next(
            (child for child in wrapper if child.attrib.get("name") == "Versions"),
            None,
        )
        if versions is not None:
            for version in versions:
                _set(version, "Progress", str(progress))

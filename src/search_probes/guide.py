from __future__ import annotations

import copy
import xml.etree.ElementTree as ET

from .adapter_definitions import (
    CATEGORY_ICON_OFF,
    CATEGORY_ICON_ON,
    MISSION_ID,
    PRESET_NOTIFY_ICON,
    PRESET_TOPIC_ICON,
)
from .adapter_errors import AdapterError
from .xml_helpers import _child, _set, _set_texture


def _wiki_patch(source_root: ET.Element, mission_id: str = MISSION_ID) -> ET.Element:
    categories = _child(source_root, "Categories")
    if not list(categories):
        raise AdapterError(
            "probe_source_shape_changed", "Current Wiki has no categories"
        )
    category = copy.deepcopy(next(iter(categories)))
    category.attrib.pop("_index", None)
    _set(category, "CategoryID", "SQN_SS_CATEGORY")
    _set(category, "CategoryIDUpper", "SQN_SS_CATEGORY_U")
    _set_texture(category, "IconOn", CATEGORY_ICON_ON)
    _set_texture(category, "IconOff", CATEGORY_ICON_OFF)
    topics = _child(category, "Topics")
    if not list(topics):
        raise AdapterError(
            "probe_source_shape_changed", "Source Wiki category has no topics"
        )
    topic = next(iter(topics))
    for extra in list(topics)[1:]:
        topics.remove(extra)
    _set(topic, "TopicID", "SQN_SS_TOPIC")
    _set(topic, "ShortDescriptionID", "SQN_SS_SHORT")
    _set_texture(topic, "Icon", PRESET_TOPIC_ICON)
    _set_texture(topic, "NotifyIcon", PRESET_NOTIFY_ICON)
    page = _child(_child(topic, "Pages"), "Pages")
    _set(page, "PageID", "SQN_SS_TOPIC")
    _set_texture(page, "Icon", PRESET_TOPIC_ICON)
    _set(page, "Content", "SQN_SS_BODY")
    _set(topic, "Mission", mission_id)
    _set(topic, "MissionButtonText", "SQN_SS_BUTTON")
    _set(topic, "Seen", "false")
    _set(topic, "Unlocked", "true")
    _set(_child(topic, "ActionSet"), "ActionSetType", "OnFootControls")
    _set(_child(category, "Type"), "WikiTopicType", "Guide")
    _child(category, "Items")[:] = []
    _set(category, "UnseenCount", "0")
    _set(category, "UnlockedCount", "0")

    result = ET.Element("Data", {"template": source_root.attrib["template"]})
    container = ET.SubElement(result, "Property", {"name": "Categories"})
    container.append(category)
    return result

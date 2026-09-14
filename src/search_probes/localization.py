from __future__ import annotations

import xml.etree.ElementTree as ET

from .adapter_definitions import LANGUAGES, LOCALISATION


def _localisation() -> ET.Element:
    root = ET.Element("Data", {"template": "cTkLocalisationTable"})
    table = ET.SubElement(root, "Property", {"name": "Table"})
    for identifier, value in LOCALISATION.items():
        entry = ET.SubElement(
            table, "Property", {"name": "Table", "value": "TkLocalisationEntry"}
        )
        ET.SubElement(entry, "Property", {"name": "Id", "value": identifier})
        for language in LANGUAGES:
            ET.SubElement(entry, "Property", {"name": language, "value": value})
    return root

from __future__ import annotations

import ctypes

from search_probes import native_memory as memory
from search_probes import resident_search_presets as _PRESETS
from search_probes import resident_search_protocol as _PROTOCOL
from search_probes.native_layout import (
    APPLICATION_REALITY_MANAGER_OFFSET,
    APPLICATION_SINGLETON,
    MAX_DYNAMIC_ARRAY_ITEMS,
    MAX_WIKI_CATEGORIES,
    MAX_WIKI_TOPICS_PER_CATEGORY,
    REALITY_MANAGER_WIKI_MISSION_TABLE_OFFSET,
    REALITY_MANAGER_WIKI_OFFSET,
    WIKI_CATEGORY_ITEMS_OFFSET,
    WIKI_CATEGORY_SIZE,
    WIKI_CATEGORY_TOPICS_OFFSET,
    WIKI_TOPIC_SIZE,
)


class GuideCatalog:
    def __init__(self, store, resolve, set_status, emit):
        self.store = store
        self.resolve = resolve
        self.set_status = set_status
        self.emit = emit
        self.original_header = None
        self.header_address = 0
        self.source_topic = None
        self.owned_buffers = []
        self.records = ()
        self.installed = False

    def wiki_snapshot(self) -> dict[str, object]:
        """Deep-copy the fixed Wiki catalogue shape without retaining an NMS pointer."""

        application = memory.read_uint(self.resolve(APPLICATION_SINGLETON), 8)
        memory.validate_pointer("application", application)
        reality_manager = application + APPLICATION_REALITY_MANAGER_OFFSET
        wiki = memory.read_uint(reality_manager + REALITY_MANAGER_WIKI_OFFSET, 8)
        wiki_mission_table = memory.read_uint(
            reality_manager + REALITY_MANAGER_WIKI_MISSION_TABLE_OFFSET, 8
        )
        memory.validate_pointer("Wiki", wiki)
        memory.validate_pointer("Wiki mission table", wiki_mission_table)

        category_header = memory.safe_dynamic_array_header(
            wiki, MAX_WIKI_CATEGORIES, name="Wiki categories"
        )
        categories: list[dict[str, object]] = []
        category_pointer = int(category_header["pointer"])
        for category_index in range(int(category_header["size"])):
            category = category_pointer + category_index * WIKI_CATEGORY_SIZE
            item_header = memory.safe_dynamic_array_header(
                category + WIKI_CATEGORY_ITEMS_OFFSET,
                MAX_DYNAMIC_ARRAY_ITEMS,
                name=f"Wiki category {category_index} items",
            )
            topic_header = memory.safe_dynamic_array_header(
                category + WIKI_CATEGORY_TOPICS_OFFSET,
                MAX_WIKI_TOPICS_PER_CATEGORY,
                name=f"Wiki category {category_index} topics",
            )
            topics: list[dict[str, object]] = []
            topic_pointer = int(topic_header["pointer"])
            for topic_index in range(int(topic_header["size"])):
                topic = topic_pointer + topic_index * WIKI_TOPIC_SIZE
                page_header = memory.safe_dynamic_array_header(
                    topic + 0xA0,
                    256,
                    name=f"Wiki category {category_index} topic {topic_index} pages",
                )
                topics.append(
                    {
                        "index": topic_index,
                        "address": f"0x{topic:016X}",
                        "mission_button_text": memory.safe_fixed_string(topic, 0x20),
                        "short_description_id": memory.safe_fixed_string(
                            topic + 0x20, 0x20
                        ),
                        "topic_id": memory.safe_fixed_string(topic + 0x40, 0x20),
                        "mission": memory.safe_fixed_string(topic + 0x90, 0x10),
                        "pages": page_header,
                        "action_set": memory.read_uint(topic + 0xB0, 4),
                        "seen": bool(memory.read_uint(topic + 0xB4, 1)),
                        "unlocked": bool(memory.read_uint(topic + 0xB5, 1)),
                    }
                )
            categories.append(
                {
                    "index": category_index,
                    "address": f"0x{category:016X}",
                    "category_id": memory.safe_fixed_string(category, 0x20),
                    "category_id_upper": memory.safe_fixed_string(
                        category + 0x20, 0x20
                    ),
                    "items": item_header,
                    "topics": topic_header,
                    "topic_records": topics,
                    "type": memory.read_uint(category + 0x90, 4),
                    "unlocked_count": memory.read_uint(category + 0x94, 4),
                    "unseen_count": memory.read_uint(category + 0x98, 4),
                }
            )
        return {
            "application": f"0x{application:016X}",
            "reality_manager": f"0x{reality_manager:016X}",
            "wiki": f"0x{wiki:016X}",
            "wiki_mission_table": f"0x{wiki_mission_table:016X}",
            "wiki_mission_count": memory.read_uint(wiki_mission_table + 8, 4),
            "categories": category_header,
            "category_records": categories,
        }

    def restore_wiki_probe(self) -> None:
        """Restore the exact source-owned Guide header before injected shutdown."""

        if self.original_header is None:
            return
        memory.write_memory(
            self.header_address,
            self.original_header,
        )
        self.original_header = None
        self.header_address = 0
        self.source_topic = None

    def ensure_wiki_source(self) -> tuple[int, bytes]:
        if self.original_header is not None and self.source_topic is not None:
            return self.header_address, self.source_topic
        snapshot = self.wiki_snapshot()
        records = [
            record
            for record in snapshot["category_records"]
            if record["category_id"] == "SQN_SS_CATEGORY"
        ]
        if len(records) != 1:
            raise RuntimeError(
                f"expected one System Search Guide category, found {len(records)}"
            )
        category = records[0]
        source_pointer = int(category["topics"]["pointer"])
        source_count = int(category["topics"]["size"])
        if source_count < 1:
            raise RuntimeError("System Search Guide category has no source topic")
        header_address = int(category["address"], 16) + WIKI_CATEGORY_TOPICS_OFFSET
        source = memory.read_memory(source_pointer, WIKI_TOPIC_SIZE)
        self.original_header = memory.read_memory(header_address, 16)
        self.header_address = header_address
        self.source_topic = source
        return header_address, source

    @staticmethod
    def replace_fixed_string(
        records: bytearray, record_index: int, field_offset: int, size: int, value: str
    ) -> None:
        encoded = value.encode("utf-8")
        if len(encoded) >= size:
            raise ValueError(f"{value!r} exceeds a {size - 1}-byte NMS fixed string")
        offset = record_index * WIKI_TOPIC_SIZE + field_offset
        records[offset : offset + size] = encoded.ljust(size, b"\0")

    def install_wiki_topics(
        self, topics: list[tuple[str, str | None]]
    ) -> dict[str, object]:
        """Atomically redirect the category header to one retained owned topic array."""

        if not 1 <= len(topics) <= _PROTOCOL.MAX_GUIDE_PROBE_TOPICS:
            raise ValueError(
                "Guide mirror requires 1 through "
                f"{_PROTOCOL.MAX_GUIDE_PROBE_TOPICS} topics"
            )
        header_address, source = self.ensure_wiki_source()
        raw_topics = bytearray(source * len(topics))
        for index, (label, mission) in enumerate(topics):
            self.replace_fixed_string(raw_topics, index, 0x40, 0x20, label)
            if mission is None:
                self.replace_fixed_string(raw_topics, index, 0x00, 0x20, "")
                self.replace_fixed_string(raw_topics, index, 0x90, 0x10, "")
            else:
                self.replace_fixed_string(raw_topics, index, 0x90, 0x10, mission)
        owned = (ctypes.c_ubyte * len(raw_topics)).from_buffer_copy(raw_topics)
        original_header = self.original_header
        if original_header is None:
            raise RuntimeError(
                "Wiki source header disappeared during mirror construction"
            )
        replacement_header = bytearray(original_header)
        replacement_header[:8] = ctypes.addressof(owned).to_bytes(8, "little")
        replacement_header[8:12] = len(topics).to_bytes(4, "little")
        # Runtime-owned memory must never be deallocated by cTkDynamicArray.
        replacement_header[12] = 1
        memory.write_memory(header_address, bytes(replacement_header))
        # NMS can retain a topic address while a Guide article is open. Keep every published
        # array alive until the original header is restored during orderly injected shutdown.
        self.owned_buffers.append(owned)
        return memory.safe_dynamic_array_header(
            header_address,
            _PROTOCOL.MAX_GUIDE_PROBE_TOPICS,
            name="System Search Guide topics",
        )

    def refresh_guide_mirror_if_pending(self) -> None:
        if self.installed:
            return
        try:
            records = self.store.records()
            if records:
                topics = [
                    (
                        _PRESETS.guide_topic_label(str(record["name"]), index),
                        _PRESETS.guide_slot_mission(index),
                    )
                    for index, record in enumerate(records)
                ]
            else:
                # A present category with a zero-length topic array reproducibly crashes NMS.
                # Keep one inert row when the user intentionally deletes every preset.
                topics = [(_PRESETS.GUIDE_EMPTY_LABEL, None)]
            installed = self.install_wiki_topics(topics)
        finally:
            self.installed = True
        # Topic labels, missions, and activation criteria must be one atomic semantic snapshot.
        # Form edits are deliberately deferred to the next launch, so dispatch must never reread
        # the live store after publishing this array.
        self.records = records
        self.set_status(
            "ready",
            f"Guide presets ready: {len(records)} saved",
            preset_count=len(records),
            topic_count=int(installed["size"]),
        )
        self.emit(
            "guide_mirror_updated",
            preset_count=len(records),
            topic_count=int(installed["size"]),
        )

    def wiki_mirror_probe(
        self, topic_count: int, label_prefix: str
    ) -> dict[str, object]:
        """Install a reversible, runtime-owned clone array for Guide capacity testing."""

        topics = [
            (
                f"{label_prefix} {index + 1:02d}" if label_prefix else "SQN_SS_TOPIC",
                None,
            )
            for index in range(topic_count)
        ]
        observed = self.install_wiki_topics(topics)
        self.installed = True
        return {
            "category": "SQN_SS_CATEGORY",
            "topic_count": topic_count,
            "label_prefix": label_prefix,
            "header_address": f"0x{self.header_address:016X}",
            "source_pointer": f"0x{int.from_bytes(self.original_header[:8], 'little'):016X}",
            "installed": observed,
        }

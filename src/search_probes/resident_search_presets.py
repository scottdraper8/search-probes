"""Persistent, bounded user preset storage for the owned System Search window."""

from __future__ import annotations

import json
import os
import threading
import uuid
from collections.abc import Callable, Mapping
from pathlib import Path

PRESET_SCHEMA = 1
PRESET_FILE = "presets.json"
PRESET_BACKUP_FILE = "presets.backup.json"
# The native Guide topic panel renders nine entries and exposes no scrolling path.  A larger
# runtime-owned array remains memory-safe, but entries after nine are unreachable in the UI.
MAX_PRESETS = 9
MAX_PRESET_NAME_LENGTH = 80
MAX_PRESET_FILE_BYTES = 1_048_576
SYSTEM_SEARCH_BUDGETS = tuple(10**exponent for exponent in range(3, 10))
GUIDE_FIXED_STRING_BYTES = 0x20
GUIDE_SLOT_PREFIX = "SQN_SS9_P"
GUIDE_EVENT_PREFIX = "SE_SQN_SS9_P"
GUIDE_READY_EVENT_PREFIX = "SE_SQN_SS9_R"
GUIDE_TARGET_EVENT_PREFIX = "SE_SQN_SS9_T"
GUIDE_EMPTY_LABEL = "No search probes saved"

DEFAULT_PRESETS: dict[str, dict[str, str]] = {
    "Earthlike": {
        # Lush is a cheap native prefilter. Paradise then applies the current executable's exact
        # player-facing predicate, including no generated storms and non-hostile sentinels.
        "target_biome": "Lush",
        "paradise_planet": "Require",
        # These are generated primary-palette hue families, not rendered-pixel guarantees.
        "grass_primary_hue": "Green",
        "sky_primary_hue": "Blue",
        # The proved relationship is one-or-more moons; exact moon count is not yet exposed.
        "planet_has_moons": "Require",
    },
}


def guide_slot_mission(index: int) -> str:
    """Return the precompiled mission identity for one bounded Guide slot."""

    if (
        not isinstance(index, int)
        or isinstance(index, bool)
        or not 0 <= index < MAX_PRESETS
    ):
        raise ValueError(f"Guide slot must be from 0 through {MAX_PRESETS - 1}")
    return f"{GUIDE_SLOT_PREFIX}{index:02d}"


def guide_slot_event(index: int) -> str:
    """Return the precompiled request-event identity for one bounded Guide slot."""

    guide_slot_mission(index)
    return f"{GUIDE_EVENT_PREFIX}{index:02d}"


def guide_slot_ready_event(index: int) -> str:
    """Return the local gate event for one bounded Guide slot."""

    guide_slot_mission(index)
    return f"{GUIDE_READY_EVENT_PREFIX}{index:02d}"


def guide_slot_target_event(index: int) -> str:
    """Return the native FromList target event for one bounded Guide slot."""

    guide_slot_mission(index)
    return f"{GUIDE_TARGET_EVENT_PREFIX}{index:02d}"


def guide_event_slot(name: str) -> int | None:
    """Decode only exact product-owned Guide scan-event identities."""

    if not name.startswith(GUIDE_EVENT_PREFIX):
        return None
    suffix = name[len(GUIDE_EVENT_PREFIX) :]
    if len(suffix) != 2 or not suffix.isascii() or not suffix.isdigit():
        return None
    index = int(suffix)
    return index if index < MAX_PRESETS else None


def guide_topic_label(name: str, index: int) -> str:
    """Fit a preset name into NMS's 31-byte Wiki topic label without splitting UTF-8."""

    guide_slot_mission(index)
    encoded = name.encode("utf-8")
    maximum = GUIDE_FIXED_STRING_BYTES - 1
    if len(encoded) <= maximum:
        return name
    suffix = f"~{index + 1:02d}".encode("ascii")
    prefix = encoded[: maximum - len(suffix)]
    while True:
        try:
            return prefix.decode("utf-8") + suffix.decode("ascii")
        except UnicodeDecodeError:
            prefix = prefix[:-1]


class PresetStore:
    """Own a complete effective preset list, including editable built-ins."""

    def __init__(
        self,
        path: Path,
        *,
        defaults: Mapping[str, Mapping[str, object]],
        normalize_criteria: Callable[[Mapping[object, object]], dict[str, object]],
        default_candidate_limit: int,
        max_candidate_limit: int,
        default_result_limit: int,
        max_result_limit: int,
    ) -> None:
        self.path = path
        self.backup_path = path.with_name(PRESET_BACKUP_FILE)
        self._lock = threading.RLock()
        self._normalize_criteria = normalize_criteria
        self._max_candidate_limit = max_candidate_limit
        self._max_result_limit = max_result_limit
        self._candidate_budgets = tuple(
            value for value in SYSTEM_SEARCH_BUDGETS if value <= max_candidate_limit
        )
        if not self._candidate_budgets:
            raise ValueError("preset maximum must allow at least 1,000 systems")
        self._defaults = tuple(
            self._normalize_record(
                {
                    "name": name,
                    "criteria": criteria,
                    "candidate_limit": default_candidate_limit,
                    "result_limit": default_result_limit,
                }
            )
            for name, criteria in defaults.items()
        )
        if len(self._defaults) > MAX_PRESETS:
            raise ValueError("built-in presets exceed the preset-store limit")
        self._records = list(self._defaults) if not path.exists() else self._load()

    @staticmethod
    def _normalize_name(value: object) -> str:
        if not isinstance(value, str):
            raise TypeError("preset name must be text")
        name = value.strip()
        if not name:
            raise ValueError("preset name must not be empty")
        if len(name) > MAX_PRESET_NAME_LENGTH:
            raise ValueError(
                f"preset name must contain at most {MAX_PRESET_NAME_LENGTH} characters"
            )
        if any(ord(character) < 32 or ord(character) == 127 for character in name):
            raise ValueError("preset name must not contain control characters")
        return name

    def _normalize_record(self, raw: Mapping[str, object]) -> dict[str, object]:
        unknown = sorted(
            set(raw) - {"name", "criteria", "candidate_limit", "result_limit"}
        )
        if unknown:
            raise ValueError(f"preset contains unknown fields: {', '.join(unknown)}")
        name = self._normalize_name(raw.get("name"))
        criteria = raw.get("criteria")
        if not isinstance(criteria, Mapping):
            raise TypeError("preset criteria must be a JSON object")
        normalized_criteria = {
            field: value
            for field, value in self._normalize_criteria(criteria).items()
            if value != "Any"
        }
        candidate_limit = raw.get("candidate_limit")
        if (
            not isinstance(candidate_limit, int)
            or isinstance(candidate_limit, bool)
            or not 1 <= candidate_limit <= self._max_candidate_limit
        ):
            raise ValueError(
                f"preset candidate_limit must be from 1 through {self._max_candidate_limit}"
            )
        result_limit = raw.get("result_limit")
        if (
            not isinstance(result_limit, int)
            or isinstance(result_limit, bool)
            or not 1 <= result_limit <= self._max_result_limit
        ):
            raise ValueError(
                f"preset result_limit must be from 1 through {self._max_result_limit}"
            )
        return {
            "name": name,
            "criteria": normalized_criteria,
            # Round by absolute system count; exact midpoints choose the larger budget.
            "candidate_limit": min(
                self._candidate_budgets,
                key=lambda value: (abs(value - candidate_limit), -value),
            ),
            "result_limit": result_limit,
        }

    def _load(self) -> list[dict[str, object]]:
        if self.path.stat().st_size > MAX_PRESET_FILE_BYTES:
            raise ValueError(f"preset file exceeds {MAX_PRESET_FILE_BYTES} bytes")
        payload = json.loads(self.path.read_text(encoding="utf-8"))
        if not isinstance(payload, Mapping) or payload.get("schema") != PRESET_SCHEMA:
            raise ValueError(f"preset file schema must be {PRESET_SCHEMA}")
        raw_records = payload.get("presets")
        if not isinstance(raw_records, list):
            raise TypeError("preset file presets must be a JSON array")
        if len(raw_records) > MAX_PRESETS:
            raise ValueError(f"preset file contains more than {MAX_PRESETS} entries")
        records: list[dict[str, object]] = []
        names: set[str] = set()
        for raw in raw_records:
            if not isinstance(raw, Mapping):
                raise TypeError("each preset must be a JSON object")
            record = self._normalize_record(raw)
            folded = str(record["name"]).casefold()
            if folded in names:
                raise ValueError(f"duplicate preset name: {record['name']}")
            names.add(folded)
            records.append(record)
        if any(
            record["candidate_limit"] != raw["candidate_limit"]
            for record, raw in zip(records, raw_records, strict=True)
        ):
            # Validate the entire library before migrating it. _write retains the original.
            self._write(records)
        return records

    def _write(self, records: list[dict[str, object]]) -> None:
        """Persist a complete candidate state before it becomes visible in memory."""

        self.path.parent.mkdir(parents=True, exist_ok=True)
        if self.path.exists():
            backup_temporary = self.backup_path.with_name(
                f".{self.backup_path.name}.{os.getpid()}.{uuid.uuid4().hex}.tmp"
            )
            backup_temporary.write_bytes(self.path.read_bytes())
            backup_temporary.replace(self.backup_path)
        payload = {"schema": PRESET_SCHEMA, "presets": records}
        encoded = (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8")
        if len(encoded) > MAX_PRESET_FILE_BYTES:
            raise ValueError(
                f"serialized preset file exceeds {MAX_PRESET_FILE_BYTES} bytes"
            )
        temporary = self.path.with_name(
            f".{self.path.name}.{os.getpid()}.{uuid.uuid4().hex}.tmp"
        )
        temporary.write_bytes(encoded)
        temporary.replace(self.path)

    def records(self) -> tuple[dict[str, object], ...]:
        """Return detached records in their user-visible order."""

        with self._lock:
            return tuple(
                {
                    **record,
                    "criteria": dict(record["criteria"]),
                }
                for record in self._records
            )

    def names(self) -> tuple[str, ...]:
        with self._lock:
            return tuple(str(record["name"]) for record in self._records)

    def get(self, name: str) -> dict[str, object] | None:
        folded = name.casefold()
        with self._lock:
            return next(
                (
                    {
                        **record,
                        "criteria": dict(record["criteria"]),
                    }
                    for record in self._records
                    if str(record["name"]).casefold() == folded
                ),
                None,
            )

    def save(
        self,
        name: str,
        criteria: Mapping[str, object],
        *,
        candidate_limit: int,
        result_limit: int,
    ) -> dict[str, object]:
        with self._lock:
            record = self._normalize_record(
                {
                    "name": name,
                    "criteria": criteria,
                    "candidate_limit": candidate_limit,
                    "result_limit": result_limit,
                }
            )
            folded = str(record["name"]).casefold()
            index = next(
                (
                    index
                    for index, existing in enumerate(self._records)
                    if str(existing["name"]).casefold() == folded
                ),
                None,
            )
            if index is None:
                if len(self._records) >= MAX_PRESETS:
                    raise ValueError(f"at most {MAX_PRESETS} presets may be saved")
                records = [*self._records, record]
            else:
                records = list(self._records)
                records[index] = record
            self._write(records)
            self._records = records
            return {**record, "criteria": dict(record["criteria"])}

    def delete(self, name: str) -> bool:
        with self._lock:
            folded = name.casefold()
            retained = [
                record
                for record in self._records
                if str(record["name"]).casefold() != folded
            ]
            if len(retained) == len(self._records):
                return False
            self._write(retained)
            self._records = retained
            return True

    def restore_defaults(self) -> None:
        with self._lock:
            default_names = {
                str(record["name"]).casefold() for record in self._defaults
            }
            custom = [
                record
                for record in self._records
                if str(record["name"]).casefold() not in default_names
            ]
            if len(self._defaults) + len(custom) > MAX_PRESETS:
                raise ValueError(
                    f"restore would exceed {MAX_PRESETS} presets; delete a custom preset first"
                )
            records = [
                {**record, "criteria": dict(record["criteria"])}
                for record in self._defaults
            ] + custom
            self._write(records)
            self._records = records

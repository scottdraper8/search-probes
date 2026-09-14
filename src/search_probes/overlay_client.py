from __future__ import annotations

import json
import threading
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Any

from search_probes import resident_search_protocol as protocol


class OverlayClient:
    def __init__(
        self,
        command_dir: Path,
        result_dir: Path,
        io_lock: threading.Lock,
        generation_getter: Callable[[], int],
        guide_status_getter: Callable[[], dict[str, object]],
        preset_store: Any,
        emit: Callable[..., None],
    ) -> None:
        self.command_dir = command_dir
        self.result_dir = result_dir
        self._io_lock = io_lock
        self._generation_getter = generation_getter
        self.guide_status = guide_status_getter
        self.preset_store = preset_store
        self.emit = emit
        self._generation = int(generation_getter())

    def next_generation(self) -> int:
        self._generation = max(self._generation + 1, int(self._generation_getter()) + 1)
        return self._generation

    def write_command(self, command: dict[str, object]) -> None:
        normalized = protocol.normalize_command(command)
        temporary = self.command_dir / f".{normalized['id']}.{uuid.uuid4().hex}.tmp"
        temporary.write_text(
            json.dumps(normalized, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        temporary.replace(self.command_dir / f"{normalized['id']}.json")

    def read_result(self, path: Path) -> dict[str, object]:
        with self._io_lock:
            value = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(value, dict):
            raise TypeError(f"expected JSON object in {path}")
        return value

    def result_path(self, command_id: str) -> Path:
        return self.result_dir / f"{command_id}.json"

    def progress_path(self, command_id: str) -> Path:
        return self.result_dir / f"{command_id}.progress.json"

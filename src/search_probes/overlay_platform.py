from __future__ import annotations

from collections.abc import Mapping


def compute_ui_layout(window_rect: tuple[int, int, int, int]) -> dict[str, float | int]:
    left, top, right, bottom = (int(value) for value in window_rect)
    screen_width = right - left
    screen_height = bottom - top
    if screen_width < 640 or screen_height < 480:
        raise ValueError(
            f"NMS window is too small for the search form: {window_rect!r}"
        )
    target_scale = max(0.8, min(2.0, min(screen_width / 1920, screen_height / 1080)))
    viewport_width = min(round(760 * target_scale), screen_width - 48)
    viewport_height = min(round(850 * target_scale), screen_height - 96)
    content_scale = min(viewport_width / 760, viewport_height / 850)
    return {
        "target_scale": target_scale,
        "content_scale": content_scale,
        "font_scale": max(1.0, target_scale),
        "viewport_width": viewport_width,
        "viewport_height": viewport_height,
        "viewport_x": max(left + 24, right - viewport_width - 24),
        "viewport_y": top + 48,
    }


def progress_requires_passive_window(
    active_command_id: str | None, guide_status: Mapping[str, object]
) -> bool:
    return active_command_id is not None or guide_status.get("state") == "searching"


def key_press_transition(raw_state: int, was_down: bool) -> tuple[bool, bool]:
    is_down = bool(raw_state & 0x8000)
    return is_down and not was_down, is_down


def parse_candidate_limit(value: object, maximum: int) -> int:
    text = str(value).strip()
    digits = text.replace(",", "")
    if not digits or not digits.isdecimal():
        raise ValueError("Maximum systems must be a whole number.")
    parsed = int(digits)
    if "," in text and text != f"{parsed:,}":
        raise ValueError("Maximum systems has invalid thousands separators.")
    if not 1 <= parsed <= maximum:
        raise ValueError(f"Maximum systems must be from 1 through {maximum:,}.")
    return parsed

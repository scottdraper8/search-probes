"""Borderless owned-window UI for the resident NMS system search.

The overlay is rendered by DearPyGUI in its own Win32 window, owned by and positioned over the NMS
window. UI callbacks only write immutable mailbox commands and read owned JSON results; they never
call NMS.
"""

from __future__ import annotations

import threading
import time
import uuid
from collections.abc import Mapping
from contextlib import suppress
from pathlib import Path

from search_probes import resident_search_protocol as protocol
from search_probes.overlay_client import OverlayClient
from search_probes.overlay_form import (
    FORM_WIDGET_FIELDS,
    MAX_PRESETS,
    POPULATION_LABELS,
    build_criteria,
    criteria_form_values,
)
from search_probes.overlay_platform import (
    compute_ui_layout,
    key_press_transition,
    parse_candidate_limit,
    progress_requires_passive_window,
)
from search_probes.overlay_results import format_result_details, format_result_label


class ResidentSearchOverlay:
    """Own a hotkey-toggled UI that communicates only through the resident mailbox."""

    def __init__(self, client: OverlayClient) -> None:
        self._client = client
        self._thread = threading.Thread(
            target=self._run,
            name="SQNResidentSearchOverlay",
            daemon=True,
        )
        self._stop = threading.Event()
        self._toggle = threading.Event()
        self._ready = threading.Event()
        self._active_id: str | None = None
        self._active_result: Path | None = None
        self._active_progress: Path | None = None
        self._navigation_result: Path | None = None
        self._generation = int(client._generation_getter())
        self._visible = False
        self._passive = False
        self._error: str | None = None

    def start(self, timeout: float = 10.0) -> None:
        self._thread.start()
        if not self._ready.wait(timeout):
            self.close()
            raise TimeoutError("resident search overlay did not become ready")
        if self._error is not None:
            raise RuntimeError(self._error)

    def close(self, timeout: float = 5.0) -> None:
        self._stop.set()
        if self._thread.is_alive() and self._thread is not threading.current_thread():
            self._thread.join(timeout=timeout)

    def request_toggle(self) -> None:
        self._toggle.set()

    def _next_generation(self) -> int:
        self._generation = self._client.next_generation()
        return self._generation

    def _write_command(self, command: dict[str, object]) -> None:
        self._client.write_command(command)

    def _read_result(self, path: Path) -> dict[str, object]:
        """Read a result without racing the Wine-side atomic replacement."""

        return self._client.read_result(path)

    def _wait_for_open(self, key_state) -> bool:
        was_down = False
        self._ready.set()
        while not self._stop.is_set():
            pressed, was_down = key_press_transition(int(key_state(0x76)), was_down)
            if pressed:
                self.request_toggle()
            if self._toggle.is_set():
                return was_down
            self._stop.wait(0.02)
        return was_down

    def _run(self) -> None:
        dpg = None
        try:
            import ctypes

            get_async_key_state = ctypes.windll.user32.GetAsyncKeyState
            get_async_key_state.argtypes = [ctypes.c_int]
            get_async_key_state.restype = ctypes.c_short
            self._client.emit("overlay_waiting_for_open")
            f7_was_down = self._wait_for_open(get_async_key_state)
            if self._stop.is_set():
                return
            self._client.emit("overlay_initializing")
            import dearpygui.dearpygui as dpg
            import win32con
            import win32gui

            nms_hwnd = win32gui.FindWindow(None, "No Man's Sky")
            if not nms_hwnd:
                raise RuntimeError("No Man's Sky window was not found")
            layout = compute_ui_layout(win32gui.GetWindowRect(nms_hwnd))
            scale = float(layout["content_scale"])

            def scaled(value: int) -> int:
                return max(1, round(value * scale))

            dpg.create_context()
            dpg.set_global_font_scale(float(layout["font_scale"]))
            dpg.create_viewport(
                title="NMS System Search",
                width=int(layout["viewport_width"]),
                height=int(layout["viewport_height"]),
                decorated=False,
                always_on_top=True,
                clear_color=(10, 17, 29, 255),
            )

            with dpg.theme() as global_theme, dpg.theme_component(dpg.mvAll):
                dpg.add_theme_color(dpg.mvThemeCol_WindowBg, (10, 17, 29, 248))
                dpg.add_theme_color(dpg.mvThemeCol_ChildBg, (16, 27, 44, 245))
                dpg.add_theme_color(dpg.mvThemeCol_FrameBg, (28, 47, 72, 255))
                dpg.add_theme_color(dpg.mvThemeCol_Button, (26, 91, 132, 255))
                dpg.add_theme_color(dpg.mvThemeCol_ButtonHovered, (35, 124, 177, 255))
                dpg.add_theme_color(dpg.mvThemeCol_Header, (26, 91, 132, 255))
                dpg.add_theme_style(dpg.mvStyleVar_FrameRounding, scaled(4))
                dpg.add_theme_style(
                    dpg.mvStyleVar_WindowPadding, scaled(18), scaled(14)
                )
                dpg.add_theme_style(dpg.mvStyleVar_ItemSpacing, scaled(10), scaled(8))

            def row(label: str, widget) -> None:
                with dpg.table_row():
                    dpg.add_text(label)
                    widget()

            def set_form_values(values: Mapping[str, str], *, query_name: str) -> None:
                for tag, value in values.items():
                    if dpg.does_item_exist(tag):
                        dpg.set_value(tag, value)
                dpg.set_value("query_name", query_name)

            tristate = ["Any", "Require", "Exclude"]
            resource_labels = {
                label: resource_id
                for resource_id, label in protocol.RESOURCE_LABELS.items()
            }
            resource_ids_to_labels = dict(protocol.RESOURCE_LABELS)
            population_labels = dict(POPULATION_LABELS)
            preset_store = self._client.preset_store
            no_preset = "Choose a preset..."
            delete_pending: dict[str, str | None] = {"name": None}

            def reset_delete_confirmation() -> None:
                delete_pending["name"] = None
                if dpg.does_item_exist("delete_preset_button"):
                    dpg.configure_item("delete_preset_button", label="Delete selected")

            def refresh_preset_combo(selected: str | None = None) -> None:
                dpg.configure_item("preset", items=[no_preset, *preset_store.names()])
                dpg.set_value(
                    "preset",
                    selected if selected in preset_store.names() else no_preset,
                )
                if dpg.does_item_exist("preset_capacity"):
                    dpg.set_value(
                        "preset_capacity",
                        f"{len(preset_store.names())} / {MAX_PRESETS} saved presets. "
                        "Built-ins may be updated or deleted. Guide changes require an NMS "
                        "restart.",
                    )

            def apply_preset(_sender, app_data) -> None:
                name = str(app_data)
                reset_delete_confirmation()
                preset = preset_store.get(name)
                if preset is None:
                    return
                criteria = preset["criteria"]
                if not isinstance(criteria, Mapping):
                    raise TypeError("stored preset criteria must be a mapping")
                set_form_values(
                    criteria_form_values(criteria, resource_ids_to_labels),
                    query_name=str(preset["name"]),
                )
                dpg.set_value("candidate_limit", str(preset["candidate_limit"]))
                dpg.set_value("result_limit", int(preset["result_limit"]))

            def clear_filters() -> None:
                set_form_values(
                    dict.fromkeys(FORM_WIDGET_FIELDS, "Any"),
                    query_name="My system search",
                )
                dpg.set_value("preset", no_preset)
                reset_delete_confirmation()

            def read_form_criteria() -> dict[str, object]:
                values = {
                    field: dpg.get_value(field)
                    for field in FORM_WIDGET_FIELDS
                    if dpg.does_item_exist(field)
                }
                values["required_resources"] = [
                    resource_labels.get(dpg.get_value(f"resource_label_{index}"), "Any")
                    for index in range(1, 4)
                ]
                population_label = dpg.get_value("population_label")
                values["population_state"] = population_labels.get(
                    population_label, "Any"
                )
                return build_criteria(values)

            def save_preset() -> None:
                try:
                    name = str(dpg.get_value("query_name"))
                    saved = preset_store.save(
                        name,
                        read_form_criteria(),
                        candidate_limit=parse_candidate_limit(
                            dpg.get_value("candidate_limit"),
                            protocol.MAX_PROVEN_UI_CANDIDATES,
                        ),
                        result_limit=int(dpg.get_value("result_limit")),
                    )
                    saved_name = str(saved["name"])
                    dpg.set_value("query_name", saved_name)
                    refresh_preset_combo(saved_name)
                    reset_delete_confirmation()
                    set_status(
                        f"Saved preset: {saved_name}. Guide library updates next launch."
                    )
                except Exception as exc:  # noqa: BLE001
                    set_status(f"Could not save preset: {exc}")

            def delete_selected_preset() -> None:
                name = str(dpg.get_value("preset"))
                if preset_store.get(name) is None:
                    reset_delete_confirmation()
                    set_status("Select a saved preset first.")
                    return
                if delete_pending["name"] != name:
                    delete_pending["name"] = name
                    dpg.configure_item(
                        "delete_preset_button", label=f"Confirm delete: {name}"
                    )
                    set_status("Press the delete button again to confirm.")
                    return
                if not preset_store.delete(name):
                    set_status("The selected preset no longer exists.")
                    return
                refresh_preset_combo()
                reset_delete_confirmation()
                set_status(
                    f"Deleted preset: {name}. Guide library updates next launch."
                )

            def restore_builtin_presets() -> None:
                try:
                    preset_store.restore_defaults()
                    refresh_preset_combo()
                    reset_delete_confirmation()
                    set_status(
                        "Built-in presets restored; custom presets were preserved. "
                        "Guide library updates next launch."
                    )
                except Exception as exc:  # noqa: BLE001
                    set_status(f"Could not restore built-ins: {exc}")

            with dpg.window(
                tag="search_window",
                no_title_bar=True,
                no_move=True,
                no_resize=True,
                no_collapse=True,
            ):
                with dpg.group(horizontal=True):
                    dpg.add_text("SEARCH PROBES")
                    dpg.add_spacer(width=scaled(390))
                    dpg.add_button(
                        label="Hide  [F7]", callback=lambda: self.request_toggle()
                    )
                dpg.add_separator()
                dpg.add_text(
                    "Find nearby systems and planets matching all selected filters."
                )
                with dpg.table(
                    header_row=False,
                    borders_innerH=False,
                    borders_outerH=False,
                    policy=dpg.mvTable_SizingStretchProp,
                ):
                    dpg.add_table_column(init_width_or_weight=0.38)
                    dpg.add_table_column(init_width_or_weight=0.62)
                    row(
                        "Query name",
                        lambda: dpg.add_input_text(
                            tag="query_name", default_value="My system search", width=-1
                        ),
                    )
                    row(
                        "Preset",
                        lambda: dpg.add_combo(
                            tag="preset",
                            items=[no_preset, *preset_store.names()],
                            default_value=no_preset,
                            callback=apply_preset,
                            width=-1,
                        ),
                    )
                    row(
                        "Maximum systems",
                        lambda: dpg.add_input_text(
                            tag="candidate_limit",
                            default_value="5000",
                            decimal=True,
                            width=-1,
                        ),
                    )
                    row(
                        "Results to retain",
                        lambda: dpg.add_slider_int(
                            tag="result_limit",
                            default_value=5,
                            min_value=1,
                            max_value=protocol.MAX_RESULTS,
                            width=-1,
                        ),
                    )

                with dpg.group(horizontal=True):
                    dpg.add_button(
                        label="Save / update preset",
                        width=scaled(250),
                        callback=save_preset,
                    )
                    dpg.add_button(
                        tag="delete_preset_button",
                        label="Delete selected",
                        width=scaled(220),
                        callback=delete_selected_preset,
                    )
                    dpg.add_button(
                        label="Restore built-ins",
                        width=scaled(200),
                        callback=restore_builtin_presets,
                    )
                dpg.add_text(
                    f"{len(preset_store.names())} / {MAX_PRESETS} saved presets. "
                    "Built-ins may be updated or deleted. Guide changes require an NMS restart.",
                    tag="preset_capacity",
                )
                result_addresses: dict[str, str] = {}
                result_details: dict[str, str] = {}
                result_sets: dict[str, dict[str, object]] = {}
                history_ids: dict[str, str] = {}
                with dpg.tab_bar():
                    with (
                        dpg.tab(label="Target planet"),
                        dpg.child_window(height=scaled(260)),
                    ):
                        dpg.add_text("Planet identity, terrain, and required resources")
                        with dpg.table(
                            header_row=False,
                            borders_innerH=False,
                            policy=dpg.mvTable_SizingStretchProp,
                        ):
                            dpg.add_table_column(init_width_or_weight=0.38)
                            dpg.add_table_column(init_width_or_weight=0.62)
                            for label, tag, items in (
                                (
                                    "Biome",
                                    "target_biome",
                                    ["Any", *protocol.PROVEN_BIOMES],
                                ),
                                (
                                    "Biome subtype",
                                    "biome_subtype",
                                    ["Any", *protocol.PROVEN_BIOME_SUBTYPES],
                                ),
                                (
                                    "Planet size",
                                    "planet_size",
                                    ["Any", *protocol.PLANET_SIZES],
                                ),
                                (
                                    "Terrain family (exact)",
                                    "terrain_setting",
                                    ["Any", *protocol.PROVEN_TERRAIN_SETTINGS],
                                ),
                                (
                                    "Floating-island terrain family",
                                    "floating_islands",
                                    tristate,
                                ),
                                (
                                    "Floating-island objects (resolved)",
                                    "floating_island_objects",
                                    tristate,
                                ),
                                (
                                    "Giant planet (non-gas)",
                                    "non_gas_giant_planet",
                                    tristate,
                                ),
                                ("Rings", "planet_rings", tristate),
                                ("Has moons", "planet_has_moons", tristate),
                                (
                                    "Required resource (all) 1",
                                    "resource_label_1",
                                    ["Any", *sorted(resource_labels)],
                                ),
                                (
                                    "Required resource (all) 2",
                                    "resource_label_2",
                                    ["Any", *sorted(resource_labels)],
                                ),
                                (
                                    "Required resource (all) 3",
                                    "resource_label_3",
                                    ["Any", *sorted(resource_labels)],
                                ),
                            ):
                                row(
                                    label,
                                    lambda tag=tag, items=items: dpg.add_combo(
                                        tag=tag,
                                        items=items,
                                        default_value="Any",
                                        width=-1,
                                    ),
                                )
                    with (
                        dpg.tab(label="Target planet · environment"),
                        dpg.child_window(height=scaled(260)),
                    ):
                        dpg.add_text("Target-planet flags")
                        with dpg.table(
                            header_row=False,
                            borders_innerH=False,
                            policy=dpg.mvTable_SizingStretchProp,
                        ):
                            dpg.add_table_column(init_width_or_weight=0.38)
                            dpg.add_table_column(init_width_or_weight=0.62)
                            for label, tag in (
                                ("Paradise planet", "paradise_planet"),
                                ("Water", "water_planet"),
                                ("Deep water", "deep_water_planet"),
                                ("Extreme storms", "extreme_weather_planet"),
                                ("Extreme hazards", "extreme_hazard_planet"),
                                ("Sentinels", "sentinels"),
                                ("Extreme sentinels", "extreme_sentinel_planet"),
                                ("Corrupt sentinels", "corrupt_sentinel_planet"),
                                ("Prime generation", "prime_planet"),
                                ("Infested", "infested_planet"),
                                ("Ordinary / normal group", "normal_planet"),
                                ("Relic planet", "relic_planet"),
                                ("RGB biome", "rgb_planet"),
                                ("Has scrap", "has_scrap"),
                                ("Creature discovery", "suitable_creature_discovery"),
                                (
                                    "Weird creature discovery",
                                    "suitable_weird_creature_discovery",
                                ),
                                ("Creature taming", "suitable_creature_taming"),
                                (
                                    "Robot creature discovery",
                                    "suitable_robot_creature_discovery",
                                ),
                            ):
                                row(
                                    label,
                                    lambda tag=tag: dpg.add_combo(
                                        tag=tag,
                                        items=tristate,
                                        default_value="Any",
                                        width=-1,
                                    ),
                                )
                            for label, tag, items in (
                                (
                                    "Life level (generated)",
                                    "life_level",
                                    ["Any", *protocol.PROVEN_PLANET_LIFE_LEVELS],
                                ),
                                (
                                    "Creature abundance (generated)",
                                    "creature_life_level",
                                    ["Any", *protocol.PROVEN_CREATURE_LIFE_LEVELS],
                                ),
                                (
                                    "Building density (generated)",
                                    "building_density",
                                    ["Any", *protocol.PROVEN_BUILDING_DENSITY_LEVELS],
                                ),
                                (
                                    "Resource abundance (generated)",
                                    "resource_level",
                                    ["Any", *protocol.PROVEN_RESOURCE_LEVELS],
                                ),
                            ):
                                row(
                                    label,
                                    lambda tag=tag, items=items: dpg.add_combo(
                                        tag=tag,
                                        items=items,
                                        default_value="Any",
                                        width=-1,
                                    ),
                                )
                            for label, tag, items in (
                                (
                                    "Storm frequency",
                                    "storm_frequency",
                                    ["Any", *protocol.PROVEN_STORM_FREQUENCIES],
                                ),
                                (
                                    "Weather intensity",
                                    "weather_intensity",
                                    ["Any", *protocol.WEATHER_INTENSITIES],
                                ),
                                (
                                    "Weather type",
                                    "weather_type",
                                    ["Any", *protocol.PROVEN_WEATHER_TYPES],
                                ),
                            ):
                                row(
                                    label,
                                    lambda tag=tag, items=items: dpg.add_combo(
                                        tag=tag,
                                        items=items,
                                        default_value="Any",
                                        width=-1,
                                    ),
                                )
                            for label, tag in (
                                ("Grass primary hue", "grass_primary_hue"),
                                ("Plant primary hue", "plant_primary_hue"),
                                ("Leaf primary hue", "leaf_primary_hue"),
                                ("Water primary hue", "water_primary_hue"),
                                ("Near-water primary hue", "near_water_primary_hue"),
                                ("Cloud primary hue", "cloud_primary_hue"),
                                ("Sky primary hue", "sky_primary_hue"),
                                ("Horizon primary hue", "horizon_primary_hue"),
                                ("Sky-fog primary hue", "sky_fog_primary_hue"),
                                (
                                    "Height-fog primary hue",
                                    "sky_height_fog_primary_hue",
                                ),
                                ("Sunset primary hue", "sky_sunset_primary_hue"),
                                ("Night-sky primary hue", "sky_night_primary_hue"),
                            ):
                                row(
                                    label,
                                    lambda tag=tag: dpg.add_combo(
                                        tag=tag,
                                        items=[
                                            "Any",
                                            *protocol.PROVEN_PRIMARY_HUES[tag],
                                        ],
                                        default_value="Any",
                                        width=-1,
                                    ),
                                )
                        dpg.add_text(
                            "Hue means the generated primary palette value; atmosphere, "
                            "weather, lighting, and display processing can change the final "
                            "on-screen colour.",
                            wrap=scaled(680),
                        )
                    with (
                        dpg.tab(label="System filters"),
                        dpg.child_window(height=scaled(260)),
                    ):
                        dpg.add_text(
                            "System-wide filters can match different planets from the target."
                        )
                        with dpg.table(
                            header_row=False,
                            borders_innerH=False,
                            policy=dpg.mvTable_SizingStretchProp,
                        ):
                            dpg.add_table_column(init_width_or_weight=0.38)
                            dpg.add_table_column(init_width_or_weight=0.62)
                            for label, tag, items in (
                                (
                                    "Star type",
                                    "star_type",
                                    ["Any", *protocol.STAR_TYPES],
                                ),
                                (
                                    "Minimum planets in system",
                                    "minimum_planets",
                                    [
                                        "Any",
                                        *[
                                            str(value)
                                            for value in protocol.PROVEN_MINIMUM_PLANETS
                                        ],
                                    ],
                                ),
                                (
                                    "Wealth",
                                    "wealth_class",
                                    ["Any", *protocol.WEALTH_CLASSES],
                                ),
                                (
                                    "Trading class",
                                    "trading_class",
                                    ["Any", *protocol.TRADING_CLASSES],
                                ),
                                (
                                    "Conflict",
                                    "conflict_level",
                                    ["Any", *protocol.CONFLICT_LEVELS],
                                ),
                                (
                                    "Population state",
                                    "population_label",
                                    ["Any", *population_labels],
                                ),
                                (
                                    "System's dominant race",
                                    "race",
                                    ["Any", *protocol.PROVEN_RACES],
                                ),
                                (
                                    "System anomaly",
                                    "anomaly",
                                    ["Any", *protocol.PROVEN_ANOMALIES],
                                ),
                                ("Pirate system", "pirate_system", tristate),
                                ("Contains any giant", "giant_planet_system", tristate),
                                ("Contains gas giant", "gas_giant_system", tristate),
                                (
                                    "Contains non-gas giant",
                                    "non_gas_giant_system",
                                    tristate,
                                ),
                                ("Contains waterworld", "waterworld_system", tristate),
                                ("Contains water planet", "system_water", tristate),
                                ("Contains deep water", "deep_water_system", tristate),
                                (
                                    "Contains weird planet",
                                    "system_weird_planet",
                                    tristate,
                                ),
                                (
                                    "Contains infested planet",
                                    "system_infested_planet",
                                    tristate,
                                ),
                                (
                                    "Contains ordinary planet",
                                    "system_normal_planet",
                                    tristate,
                                ),
                                (
                                    "Contains relic planet",
                                    "system_relic_planet",
                                    tristate,
                                ),
                                ("Contains RGB biome", "system_rgb_planet", tristate),
                                (
                                    "Contains corrupt sentinels",
                                    "system_corrupt_sentinel_planet",
                                    tristate,
                                ),
                                (
                                    "Contains extreme storms",
                                    "system_extreme_storm_planet",
                                    tristate,
                                ),
                            ):
                                row(
                                    label,
                                    lambda tag=tag, items=items: dpg.add_combo(
                                        tag=tag,
                                        items=items,
                                        default_value="Any",
                                        width=-1,
                                    ),
                                )

                def set_status(value: str) -> None:
                    dpg.set_value("status", value)

                def show_result_set(result: Mapping[str, object]) -> None:
                    matches = result.get("matches", [])
                    if not isinstance(matches, list):
                        matches = []
                    labels: list[str] = []
                    result_addresses.clear()
                    result_details.clear()
                    for index, match in enumerate(matches):
                        if not isinstance(match, Mapping):
                            continue
                        label = format_result_label(match, index)
                        labels.append(label)
                        result_addresses[label] = str(
                            match.get("navigation_address", match.get("address", ""))
                        )
                        result_details[label] = format_result_details(
                            match, protocol.RESOURCE_LABELS
                        )
                    if not labels:
                        labels = ["No exact match found."]
                    dpg.configure_item("results", items=labels)
                    dpg.set_value("results", labels[0])
                    dpg.set_value("result_details", result_details.get(labels[0], ""))

                def select_history(_sender, app_data) -> None:
                    command_id = history_ids.get(str(app_data))
                    result = result_sets.get(command_id or "")
                    if result is not None:
                        show_result_set(result)

                def select_result(_sender, app_data) -> None:
                    dpg.set_value(
                        "result_details", result_details.get(str(app_data), "")
                    )

                def submit_search() -> None:
                    if self._active_id is not None:
                        set_status(
                            "A search is already active; cancel or wait for it to finish."
                        )
                        return
                    try:
                        candidate_limit = parse_candidate_limit(
                            dpg.get_value("candidate_limit"),
                            protocol.MAX_PROVEN_UI_CANDIDATES,
                        )
                    except ValueError as exc:
                        set_status(str(exc))
                        return
                    command_id = f"overlay-{time.time_ns()}-{uuid.uuid4().hex}"
                    command = {
                        "protocol": protocol.PROTOCOL_VERSION,
                        "id": command_id,
                        "generation": self._next_generation(),
                        "action": "search",
                        "name": str(dpg.get_value("query_name")).strip()
                        or "System search",
                        "criteria": read_form_criteria(),
                        "candidate_limit": candidate_limit,
                        "result_limit": int(dpg.get_value("result_limit")),
                    }
                    self._write_command(command)
                    self._active_id = command_id
                    self._active_result = self._client.result_path(command_id)
                    self._active_progress = self._client.progress_path(command_id)
                    set_status("Searching...")
                    show_passive_progress()

                def cancel_search() -> None:
                    if self._active_id is None:
                        set_status("No active search.")
                        return
                    command_id = f"overlay-cancel-{time.time_ns()}-{uuid.uuid4().hex}"
                    self._write_command(
                        {
                            "protocol": protocol.PROTOCOL_VERSION,
                            "id": command_id,
                            "generation": self._generation,
                            "action": "cancel",
                            "target_id": self._active_id,
                        }
                    )
                    set_status("Cancellation requested...")
                    win32gui.SetForegroundWindow(nms_hwnd)

                def navigate_selected() -> None:
                    selected = dpg.get_value("results")
                    address = result_addresses.get(selected)
                    if address is None:
                        set_status("Select a result first.")
                        return
                    command_id = f"overlay-navigate-{time.time_ns()}-{uuid.uuid4().hex}"
                    self._write_command(
                        {
                            "protocol": protocol.PROTOCOL_VERSION,
                            "id": command_id,
                            "generation": self._next_generation(),
                            "action": "navigate",
                            "address": address,
                        }
                    )
                    self._navigation_result = self._client.result_path(command_id)
                    set_status("Publishing the selected target to NMS...")
                    if self._visible:
                        self.request_toggle()

                with dpg.group(horizontal=True):
                    dpg.add_button(
                        tag="search_button",
                        label="Search",
                        width=scaled(330),
                        callback=submit_search,
                    )
                    dpg.add_button(
                        label="Cancel", width=scaled(180), callback=cancel_search
                    )
                    dpg.add_button(
                        label="Hide",
                        width=scaled(180),
                        callback=lambda: self.request_toggle(),
                    )
                dpg.add_button(label="Clear filters", width=-1, callback=clear_filters)
                dpg.add_text("Ready", tag="status", wrap=scaled(700))
                dpg.add_separator()
                dpg.add_text("Results")
                dpg.add_combo(
                    tag="query_history",
                    items=[],
                    width=-1,
                    callback=select_history,
                )
                dpg.add_listbox(
                    tag="results",
                    items=[],
                    width=-1,
                    num_items=5,
                    callback=select_result,
                )
                dpg.add_text("", tag="result_details", wrap=scaled(700))
                dpg.add_button(
                    tag="navigate_button",
                    label="Navigate to selected result",
                    width=-1,
                    callback=navigate_selected,
                )
                dpg.add_text(
                    "Navigation publishes the selected result through the dedicated Search Probe "
                    "mission. Select Search Probe Target in your Log, then use Current Mission "
                    "in the Galaxy Map.",
                    wrap=scaled(700),
                )
                dpg.add_text(
                    "Search returns focus to NMS for full update cadence; "
                    "progress remains visible. "
                    "F7 toggles the form.",
                    wrap=scaled(700),
                )

            dpg.bind_theme(global_theme)
            dpg.setup_dearpygui()
            dpg.set_viewport_pos((int(layout["viewport_x"]), int(layout["viewport_y"])))
            dpg.show_viewport()
            dpg.set_primary_window("search_window", True)
            overlay_hwnd = win32gui.FindWindow(None, "NMS System Search")
            if not overlay_hwnd:
                raise RuntimeError("overlay window handle was not found")
            set_window_long = ctypes.windll.user32.SetWindowLongPtrW
            set_window_long.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_ssize_t]
            set_window_long.restype = ctypes.c_ssize_t
            set_window_long(
                overlay_hwnd, -8, int(nms_hwnd)
            )  # GWLP_HWNDPARENT for an owned top-level window

            get_window_long = ctypes.windll.user32.GetWindowLongPtrW
            get_window_long.argtypes = [ctypes.c_void_p, ctypes.c_int]
            get_window_long.restype = ctypes.c_ssize_t

            def set_overlay_passive(passive: bool) -> None:
                """Let a visible progress form render without taking NMS activation."""

                if self._passive == passive:
                    return
                extended_style = int(get_window_long(overlay_hwnd, -20))  # GWL_EXSTYLE
                no_activate = 0x08000000  # WS_EX_NOACTIVATE
                updated = (
                    extended_style | no_activate
                    if passive
                    else extended_style & ~no_activate
                )
                set_window_long(overlay_hwnd, -20, updated)
                self._passive = passive

            def show_passive_progress() -> bool:
                """Return X/Win32 focus to NMS before remapping passive progress."""

                win32gui.ShowWindow(overlay_hwnd, win32con.SW_HIDE)
                win32gui.SetForegroundWindow(nms_hwnd)
                set_overlay_passive(True)
                win32gui.ShowWindow(overlay_hwnd, win32con.SW_SHOWNOACTIVATE)
                if win32gui.GetForegroundWindow() == nms_hwnd:
                    return True
                win32gui.ShowWindow(overlay_hwnd, win32con.SW_HIDE)
                set_overlay_passive(False)
                self._visible = False
                self._client.emit(
                    "overlay_passive_focus_fallback",
                    expected_nms_hwnd=int(nms_hwnd),
                    foreground_hwnd=int(win32gui.GetForegroundWindow() or 0),
                )
                return False

            dpg.render_dearpygui_frame()
            self._client.emit("overlay_window_ready")
            guide_status_revision = -1

            while dpg.is_dearpygui_running() and not self._stop.is_set():
                f7_pressed, f7_was_down = key_press_transition(
                    int(get_async_key_state(0x76)), f7_was_down
                )  # VK_F7
                if f7_pressed:
                    self.request_toggle()
                if self._toggle.is_set():
                    self._toggle.clear()
                    self._visible = not self._visible
                    progress_active = progress_requires_passive_window(
                        self._active_id,
                        self._client.guide_status(),
                    )
                    if self._visible and progress_active:
                        show_passive_progress()
                    elif self._visible:
                        set_overlay_passive(False)
                        win32gui.ShowWindow(overlay_hwnd, win32con.SW_SHOW)
                        win32gui.SetForegroundWindow(overlay_hwnd)
                    else:
                        set_overlay_passive(False)
                        win32gui.ShowWindow(overlay_hwnd, win32con.SW_HIDE)
                        win32gui.SetForegroundWindow(nms_hwnd)
                guide_status = self._client.guide_status()
                revision = int(guide_status.get("revision", 0))
                if revision != guide_status_revision:
                    guide_status_revision = revision
                    set_status(
                        str(guide_status.get("message", "Guide status changed."))
                    )
                    if self._visible and self._active_id is None:
                        guide_searching = guide_status.get("state") == "searching"
                        if guide_searching:
                            show_passive_progress()
                        else:
                            set_overlay_passive(False)
                result_path = self._active_result
                if result_path is not None and result_path.exists():
                    result = self._read_result(result_path)
                    status = str(result.get("status", "unknown"))
                    matches = result.get("matches", [])
                    if status == "completed" and isinstance(matches, list):
                        result_sets[self._active_id or ""] = result
                        query_name = str(result.get("query_name", "System search"))
                        history_label = f"{query_name} - {len(matches)} result(s)"
                        if history_label in history_ids:
                            history_label = f"{history_label} · {len(history_ids) + 1}"
                        history_ids[history_label] = self._active_id or ""
                        dpg.configure_item("query_history", items=list(history_ids))
                        dpg.set_value("query_history", history_label)
                        show_result_set(result)
                        checked = result.get("candidates_checked", "?")
                        if matches:
                            set_status(
                                f"Completed - {len(matches)} result(s), {checked} checked. "
                                "Select a result and press Navigate to publish it to NMS."
                            )
                        else:
                            set_status(f"Completed - no matches, {checked} checked.")
                    elif status == "cancelled":
                        set_status("Cancelled.")
                    else:
                        set_status(f"Search ended with status: {status}")
                    self._active_id = None
                    self._active_result = None
                    self._active_progress = None
                    if self._visible:
                        guide_searching = (
                            self._client.guide_status().get("state") == "searching"
                        )
                        set_overlay_passive(guide_searching)
                progress_path = self._active_progress
                if progress_path is not None and progress_path.exists():
                    progress = self._read_result(progress_path)
                    checked = int(progress.get("candidates_checked", 0))
                    total = int(progress.get("candidate_count", 0))
                    found = int(progress.get("matches_found", 0))
                    set_status(
                        f"Searching... {checked}/{total} checked, {found} found."
                    )
                navigation_path = self._navigation_result
                if navigation_path is not None and navigation_path.exists():
                    result = self._read_result(navigation_path)
                    if result.get("status") == "completed":
                        set_status(
                            "Navigation ready. Select Search Probe Target in the Log, then use "
                            "Current Mission in the Galaxy Map. "
                            f"Target: {result.get('address', 'unknown')}"
                        )
                    else:
                        set_status(
                            f"Navigation handoff failed: {result.get('error', 'unknown')}"
                        )
                    self._navigation_result = None
                dpg.render_dearpygui_frame()
        except Exception as exc:  # noqa: BLE001
            self._error = f"{type(exc).__name__}: {exc}"
            with suppress(Exception):
                self._client.emit("overlay_error", error=self._error)
            self._ready.set()
        finally:
            if dpg is not None:
                with suppress(Exception):
                    dpg.destroy_context()
            self._ready.set()

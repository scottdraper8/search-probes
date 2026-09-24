from __future__ import annotations

import json
import queue
import time

from search_probes import resident_search_protocol as _PROTOCOL
from search_probes.native_layout import MAX_CANDIDATES_PER_SLICE, MAX_SEARCH_SLICE_NS
from search_probes.regions import CandidateStream
from search_probes.survey_summary import record_survey_snapshot


class SearchScheduler:
    def __init__(self, engine, commands, respond, progress, complete_guide, emit):
        self.engine = engine
        self.commands = commands
        self.respond = respond
        self.progress = progress
        self.complete_guide = complete_guide
        self.emit = emit
        self.searches = {}
        self.surveys = {}

    def advance_search(self, command: dict[str, object]) -> None:
        command_id = str(command["id"])
        state = self.searches.get(command_id)
        if state is None:
            if len(self.searches) >= _PROTOCOL.MAX_ACTIVE_SEARCHES:
                raise RuntimeError("active search limit reached")
            identity = json.dumps(
                {
                    "criteria": command.get("criteria", {}),
                    "candidate_limit": command["candidate_limit"],
                    "result_limit": command["result_limit"],
                },
                sort_keys=True,
                separators=(",", ":"),
            )
            if any(
                other.get("identity") == identity for other in self.searches.values()
            ):
                raise RuntimeError("an identical search is already active")
            started = time.perf_counter_ns()
            candidates = self.engine.enumerate_candidates(
                min(int(command["candidate_limit"]) + 1, _PROTOCOL.MAX_CANDIDATES)
            )
            enumeration_metadata = dict(self.engine.last_enumeration)
            stream = CandidateStream(
                candidates,
                int(enumeration_metadata["resolved_address"], 0),
                int(command["candidate_limit"]),
            )
            state = {
                "identity": identity,
                "stream": stream,
                "candidate_count": int(command["candidate_limit"]),
                "candidates_checked": 0,
                "matches": [],
                "evaluation_ns": 0,
                "max_evaluation_ns": 0,
                "slice_count": 0,
                "max_slice_ns": 0,
                "max_candidates_per_slice": 0,
                "candidates_per_slice_limit": (
                    1
                    if (
                        _PROTOCOL.uses_primary_colour(command.get("criteria", {}))
                        or _PROTOCOL.uses_planet_spawn_flags(
                            command.get("criteria", {})
                        )
                    )
                    else MAX_CANDIDATES_PER_SLICE
                ),
                "last_progress": 0,
                "last_progress_time": 0.0,
                "enumeration_ns": time.perf_counter_ns() - started,
                "enumeration_metadata": enumeration_metadata,
                "max_region_generation_ns": 0,
                "reality_index": int(enumeration_metadata["current_reality_index"]),
            }
            self.searches[command_id] = state
            self.emit(
                "search_started",
                command_id=command_id,
                generation=command["generation"],
                candidate_count=int(command["candidate_limit"]),
            )
            self.requeue_search(command)
            return

        current_reality = self.engine.current_reality_index()
        search_reality = int(state["reality_index"])
        if current_reality != search_reality:
            self.searches.pop(command_id, None)
            raise RuntimeError(
                f"the player changed from galaxy #{search_reality + 1} to galaxy "
                f"#{current_reality + 1} while this search was running; run it again"
            )

        stream = state["stream"]
        matches = state["matches"]
        if not isinstance(stream, CandidateStream) or not isinstance(matches, list):
            raise TypeError("corrupt resident search state")
        slice_started = time.perf_counter_ns()
        slice_candidates = 0
        while slice_candidates < int(state["candidates_per_slice_limit"]):
            if stream.done:
                self.record_search_slice(state, slice_started, slice_candidates)
                self.complete_search(command, state)
                return
            candidate = stream.take()
            if candidate is None:
                # Region generation is one non-preemptible native call. Give it
                # its own update, then evaluate its copied systems on later ticks.
                if slice_candidates == 0:
                    started = time.perf_counter_ns()
                    stream.expand(self.engine.generate_region)
                    elapsed = time.perf_counter_ns() - started
                    state["enumeration_ns"] += elapsed
                    state["max_region_generation_ns"] = max(
                        state["max_region_generation_ns"], elapsed
                    )
                break
            address = int(str(candidate["address"]), 0)
            started = time.perf_counter_ns()
            accepted, evaluation_pass, summary = self.engine.candidate_matches(
                command.get("criteria", {}), address
            )
            elapsed_ns = time.perf_counter_ns() - started
            state["candidates_checked"] = int(state["candidates_checked"]) + 1
            state["evaluation_ns"] = int(state["evaluation_ns"]) + elapsed_ns
            state["max_evaluation_ns"] = max(
                int(state["max_evaluation_ns"]), elapsed_ns
            )
            slice_candidates += 1
            if accepted:
                if not isinstance(summary, dict):
                    raise RuntimeError("accepted search result has no target summary")
                planet = summary.get("planet")
                if not isinstance(planet, dict) or "planet_index" not in planet:
                    raise RuntimeError(
                        "accepted search result has no exact target planet"
                    )
                navigation_address = _PROTOCOL.universe_address_with_planet_index(
                    address, int(planet["planet_index"])
                )
                matches.append(
                    {
                        **candidate,
                        "navigation_address": f"0x{navigation_address:016X}",
                        "evaluation_ns": elapsed_ns,
                        "evaluation_pass": evaluation_pass,
                        "summary": summary,
                    }
                )
            if len(matches) >= int(command["result_limit"]):
                self.record_search_slice(state, slice_started, slice_candidates)
                self.complete_search(command, state)
                return
            if time.perf_counter_ns() - slice_started >= MAX_SEARCH_SLICE_NS:
                break
        self.record_search_slice(state, slice_started, slice_candidates)
        checked = int(state["candidates_checked"])
        if (
            checked - int(state["last_progress"]) >= 64
            or time.monotonic() - state["last_progress_time"] >= 1.0
        ):
            state["last_progress"] = checked
            state["last_progress_time"] = time.monotonic()
            self.progress(
                command,
                candidates_checked=checked,
                candidate_count=state["candidate_count"],
                matches_found=len(matches),
                regions_generated=stream.regions_generated,
                region_systems_checked=stream.region_checked,
            )
        self.requeue_search(command)

    def advance_survey(self, command: dict[str, object]) -> None:
        """Collect bounded generated-data cardinalities without retaining every snapshot."""

        command_id = str(command["id"])
        state = self.surveys.get(command_id)
        if state is None:
            if len(self.surveys) >= _PROTOCOL.MAX_ACTIVE_SEARCHES:
                raise RuntimeError("active survey limit reached")
            started = time.perf_counter_ns()
            state = {
                "candidates": self.engine.enumerate_candidates(
                    int(command["candidate_limit"]),
                    maximum=_PROTOCOL.MAX_SURVEY_CANDIDATES,
                ),
                "next_index": 1,
                "systems_checked": 0,
                "planets_checked": 0,
                "counts": {},
                "samples": {},
                "name_samples": [],
                "slice_count": 0,
                "max_slice_ns": 0,
                "max_candidates_per_slice": 0,
                "evaluation_ns": 0,
                "max_evaluation_ns": 0,
                "enumeration_ns": time.perf_counter_ns() - started,
                "enumeration_metadata": dict(self.engine.last_enumeration),
            }
            self.surveys[command_id] = state
            self.requeue_search(command)
            return

        candidates = state["candidates"]
        if not isinstance(candidates, list):
            raise TypeError("corrupt resident survey state")
        slice_started = time.perf_counter_ns()
        slice_candidates = 0
        candidates_per_slice = (
            1
            if command["action"] in {"colour_survey", "spawn_survey"}
            else MAX_CANDIDATES_PER_SLICE
        )
        while slice_candidates < candidates_per_slice:
            next_index = int(state["next_index"])
            if next_index >= len(candidates):
                self.record_search_slice(state, slice_started, slice_candidates)
                self.surveys.pop(command_id, None)
                self.respond(
                    command,
                    "completed",
                    systems_checked=int(state["systems_checked"]),
                    planets_checked=int(state["planets_checked"]),
                    counts=state["counts"],
                    samples=state["samples"],
                    name_samples=state["name_samples"],
                    candidate_count=len(candidates),
                    current_system_excluded=True,
                    enumeration_ns=int(state["enumeration_ns"]),
                    enumeration_metadata=state["enumeration_metadata"],
                    evaluation_ns=int(state["evaluation_ns"]),
                    max_evaluation_ns=int(state["max_evaluation_ns"]),
                    slice_count=int(state["slice_count"]),
                    max_slice_ns=int(state["max_slice_ns"]),
                    max_candidates_per_slice=int(state["max_candidates_per_slice"]),
                )
                return
            candidate = candidates[next_index]
            if not isinstance(candidate, dict):
                raise TypeError("corrupt resident survey candidate")
            address = int(str(candidate["address"]), 0)
            started = time.perf_counter_ns()
            snapshot = self.engine.system_snapshot(
                address,
                include_colours=command["action"] == "colour_survey",
                include_weather=True,
                include_metadata=True,
                include_spawn_flags=True,
                include_spawns=command["action"] == "spawn_survey",
            )
            snapshot["system_name"] = self.engine.generated_system_name(address)
            snapshot["native_diagnostics"] = self.native_survey_diagnostics(
                address, snapshot
            )
            record_survey_snapshot(state, candidate, snapshot)
            elapsed_ns = time.perf_counter_ns() - started
            state["evaluation_ns"] = int(state["evaluation_ns"]) + elapsed_ns
            state["max_evaluation_ns"] = max(
                int(state["max_evaluation_ns"]), elapsed_ns
            )
            state["next_index"] = next_index + 1
            state["systems_checked"] = int(state["systems_checked"]) + 1
            slice_candidates += 1
            if time.perf_counter_ns() - slice_started >= MAX_SEARCH_SLICE_NS:
                break
        self.record_search_slice(state, slice_started, slice_candidates)
        self.requeue_search(command)

    def native_survey_diagnostics(
        self, address: int, snapshot: dict[str, object]
    ) -> dict[str, object]:
        """Compare traced native predicates with independently decoded generated fields."""

        comparisons = {
            "normal_planet": (
                {"any_biome_not_weird_or_dead": "Require"},
                bool(snapshot["has_normal_planet"]),
            ),
            "infested_planet": (
                {"any_infested_biome": "Require"},
                bool(snapshot["has_infested_planet"]),
            ),
            "rgb_planet": (
                {"any_rgb_biome": "Require"},
                bool(snapshot["has_rgb_planet"]),
            ),
            "relic_planet": (
                {"system_relic_planet": "Require"},
                bool(snapshot["has_relic_planet"]),
            ),
            "native_weird_group": (
                {"system_weird_planet": "Require"},
                bool(snapshot["has_weird_planet"] or snapshot["has_rgb_planet"]),
            ),
            "extreme_storm_planet": (
                {"system_extreme_storm_planet": "Require"},
                bool(snapshot["has_extreme_storm_planet"]),
            ),
        }
        result: dict[str, object] = {}
        for name, (criteria, expected) in comparisons.items():
            accepted, evaluation_pass = self.engine.native_candidate_matches(
                criteria, address
            )
            result[name] = {
                "native": accepted,
                "generated": expected,
                "matches": accepted == expected,
                "evaluation_pass": evaluation_pass,
            }
        undiscovered, undiscovered_pass = self.engine.native_candidate_matches(
            {"undiscovered": "Require"}, address
        )
        result["undiscovered"] = {
            "native": undiscovered,
            "evaluation_pass": undiscovered_pass,
        }
        return result

    @staticmethod
    def record_search_slice(
        state: dict[str, object], slice_started: int, slice_candidates: int
    ) -> None:
        if slice_candidates == 0:
            return
        slice_ns = time.perf_counter_ns() - slice_started
        state["slice_count"] = int(state.get("slice_count", 0)) + 1
        state["max_slice_ns"] = max(int(state.get("max_slice_ns", 0)), slice_ns)
        state["max_candidates_per_slice"] = max(
            int(state.get("max_candidates_per_slice", 0)), slice_candidates
        )

    def requeue_search(self, command: dict[str, object]) -> None:
        try:
            self.commands.put_nowait(command)
        except queue.Full as exc:
            self.searches.pop(str(command["id"]), None)
            self.surveys.pop(str(command["id"]), None)
            raise RuntimeError("command queue filled while continuing search") from exc

    def complete_search(
        self,
        command: dict[str, object],
        state: dict[str, object],
        *,
        status: str = "completed",
    ) -> None:
        command_id = str(command["id"])
        self.searches.pop(command_id, None)
        matches = state["matches"]
        if not isinstance(matches, list):
            raise TypeError("corrupt resident match state")
        slice_limit = int(state["candidates_per_slice_limit"])
        candidate_word = "candidate" if slice_limit == 1 else "candidates"
        guide_result: dict[str, object] = {}
        if "_guide_slot" in command and status == "completed":
            guide_result = self.complete_guide(command, matches)
        self.respond(
            command,
            status,
            matched=bool(matches),
            matches=matches,
            query_name=str(command.get("name", "System search")),
            candidates_checked=int(state["candidates_checked"]),
            candidate_count=state["candidate_count"],
            completion_reason=(
                "cancelled"
                if status == "cancelled"
                else "result_limit"
                if len(matches) >= int(command["result_limit"])
                else "candidate_limit"
                if state["stream"].checked >= state["stream"].limit
                else "regions_exhausted"
            ),
            regions_generated=state["stream"].regions_generated,
            max_region_generation_ns=state["max_region_generation_ns"],
            region_systems_checked=state["stream"].region_checked,
            search_order="nearby distance order, then expanding region shells",
            current_system_excluded=True,
            enumeration_ns=int(state["enumeration_ns"]),
            enumeration_metadata=state["enumeration_metadata"],
            evaluation_ns=int(state["evaluation_ns"]),
            max_evaluation_ns=int(state["max_evaluation_ns"]),
            slice_count=int(state.get("slice_count", 0)),
            max_slice_ns=int(state.get("max_slice_ns", 0)),
            max_candidates_per_slice=int(state.get("max_candidates_per_slice", 0)),
            scheduling=(
                f"at most {slice_limit} {candidate_word} per gameplay update; "
                f"do not start another after elapsed time reaches {MAX_SEARCH_SLICE_NS} ns"
            ),
            **guide_result,
        )

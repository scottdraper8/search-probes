# Validation and performance

## Supported scope

The product uses the exact executable identity in README.md. Runtime validation uses Steam Proton
on an i9-13900KF, with the permitted save's purple systems unlocked. Native Windows desktop has
not been tested. Game updates require native-interface validation before the executable guard can
be changed. Other native hook mods and alternate WinMM proxies are outside the tested scope.

## Current 7.06 qualification

Steam Proton acceptance uses the exact 7.06 executable and the backed-up Galaxies 1–50 save.
The host reported degraded graphics-worker health, so this review makes no timing, memory,
startup-duration, shutdown-duration, or throughput claims.

- The 1,000-system blue-sky/floating-island query returns the same four addresses at 1, 2, 4, 8
  and 16 workers, with 4,754 generated planets and 1,213 object scans. These are worker-parity
  comparisons, not the historical independent native oracle.
- Representative positive and negative controls cover Any, purple stars, Earthlike criteria,
  anomaly multiselect, resource All/Any, weather/storm/flora/fauna and minimum planet count.
  Any performs no optional metadata, colour or object acquisition.
- Cold, cached and disjoint continuation searches pass. Cached matches perform zero generation.
  Separate bounded searches traverse 21,000 systems while the fact cache stops at 16,384.
  Cancellation and resumption preserve completed work and continuation.
- Live solar/planet context bytes, the shared filename-map header and existing resource references
  remain unchanged; newly loaded table references return to zero.
- Remote navigation publishes the exact address, rejects duplicate selection and supports native
  Log abandonment. Save reload preserves the destination and player identity while clearing facts,
  object summaries and active progress. Previously completed visible cards intentionally remain.
- Guide completion creates the results window and reopens an existing hidden window with the
  preset criteria synchronized. Empty slots open the editor with an explanation. Ordinary Steam
  self-start and native Quit to Desktop work.
- The destination definition uses the nonprocedural Wiki mission table. Current NPC-table loading
  forcibly enables procedural titles. Native labels, exact route, fresh creation, abandonment and
  saved-route loading pass with the corrected definition. Authored mission leaves match the game
  export after removing runtime-only flags and omitting the unused static marker tag.
- Existing host Rust tests pass (70); the Windows GNU production build and package manifest pass.

Local-planet arrival/completion, live travel during a search, every enum's positive case, and native
Windows desktop were not repeated in this 7.06 review. Historical coverage below remains explicitly
versioned. Default frame budgets are scheduling targets, not hard limits on a native call.

## Historical 7.04 qualification

The following corpus and measurements were collected on 7.04. They document the architecture
and earlier coverage; they are not independent 7.06 reference or performance measurements.

- Independent solar, planet, query and RNG contexts; bounded persistent worker pool, 1–16 workers.
- One solar generation per searched system and at most one generation per required planet.
- 349 filter cases against native reference facts, including multiselect, resources and colours.
- Independent fresh native comparison across 1,000 systems and 4,769 planets.
- Purple systems and noncontiguous native region indices, including distant and boundary regions.
- 34 successive 1,000-system searches continue beyond the old graph limit. The fact cache remains
  bounded at 16,384 systems through more than a full additional cache of replacements.
- Cached matches require no new generation when their known facts satisfy the new criteria.
- Any omits its predicate and optional acquisition. Cancellation preserves completed work.
- Actual UI input covers dropdown budgets, multiselect, 100 results, replacement cache pages,
  presets, Guide opening and correct Guide criteria synchronization.
- Native destinations retain the exact planet address for local and remote systems. Completion,
  duplicate rejection, abandonment and save-reload persistence are verified without moving the player.
- Save reload clears search facts, object-table summaries, results and continuation.
- Ordinary Steam startup and native Quit to Desktop work without a development injector.

The host suite has 70 Rust tests. The parent repository retains native probes, screenshots, exact
run identities and raw results in its Search Probes Rust release plan. Positive anomaly controls
cover Atlas stations and black holes; other exposed native anomaly values have negative controls.
Configuration invalidation is supported by inspected native fields and epoch comparison plus live
save reload, not a live experiment for every possible language or difficulty change.

## Historical 7.04 performance

The same 1,000-system blue-sky/floating-island query produces nine identical matches across
1, 2, 4, 8 and 16 workers. It generates 4,769 planets and evaluates 989 object-table selections.
The table-summary cache reduces native table acquisitions from 20,234 to 169, with 20,065 hits.

| Workers | Elapsed time | Time inside search batches |
| --- | --- | --- |
| 1 | 3.55–4.00 s | 0.91–1.04 s |
| 2 | 2.24 s | 0.66 s |
| 4 | 1.41–1.54 s | 0.50–0.54 s |
| 8 | 1.29 s | 0.50 s |
| 16 | 1.30 s | 0.63 s |

These are bounded ordered samples, with game resources already warmed by earlier work. They are
not sustained throughput promises. At four workers, the previous repeated-acquisition path took
about 8.5 seconds elapsed and 7.2 seconds inside batches for the same results. Earthlike's cheaper
predicates reject most planets early: a separate 1,000-system run took 0.36 seconds elapsed, generated
449 planets, acquired metadata for 166 and colours for 49, and found three matches.

Workers still join before the gameplay callback returns because they borrow game-owned resources
and configuration. Search pauses with gameplay. Native work and shared resource acquisition can
exceed the requested frame budget; a cold batch exceeded 100 ms during validation. More workers
can increase frame stalls and do not provide linear scaling. Two workers and a 4 ms target are the
default compromise.

No 100,000-system or billion-system live search was run. The budget supports one billion without
allocating storage proportional to that number, but completing it in hours is not demonstrated.
The design retains acquired facts for a bounded recent population, not a complete galaxy database.
Full-cache requested heap measured about 41.2 MiB with eight planets and maximum-length names per
system; allocator bookkeeping is additional. Object summaries retain at most 1,024 bounded keys and
small owned values. Search continuations retain at most 128 filter/galaxy combinations.

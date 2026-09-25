# Validation and performance

## Supported scope

The product uses the exact executable identity in README.md. Runtime validation uses Steam Proton
on an i9-13900KF, with the permitted save's purple systems unlocked. Native Windows desktop has
not been tested. Game updates require native-interface validation before the executable guard can
be changed. Other native hook mods and alternate WinMM proxies are outside the tested scope.

## Accepted behavior

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

## Measured performance

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

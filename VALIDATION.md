# Validation and performance

## Supported scope

The product uses the exact executable identity in README.md. Runtime validation uses Steam Proton
on an i9-13900KF, with the permitted save's purple systems unlocked. Native Windows desktop has
not been tested. Game updates require native-interface validation before the executable guard can
be changed. Other native hook mods and alternate WinMM proxies are outside the tested scope.

## Overlay qualification

The 0.2.0 overlay is being qualified against 7.06 on Steam Proton. Actual in-game rendering passes
on the 3840×2160 SDR B8G8R8A8_UNORM swapchain through the game's Streamline dispatcher.
Operator checks pass opening with F7, switching to another application and back, a bounded search
returning results without hiding the overlay, subsequent F7 hide/reopen, overlay resizing and
scrolling, and keyboard/mouse capture. The captured search used a 1,000-system budget and stopped
at ten results after two systems. The operator also confirms cancellation and the production overlay
working after restart. Swapchain recreation remains pending. The HDR10 and scRGB graphics compositor passes offscreen Vulkan pixel tests for background
preservation, white/black levels, partial-alpha blending, gamut conversion and extended-range
highlights. This exercises the actual graphics pipeline and image transfers on the Intel Arc A770,
independently of NMS, with Khronos synchronization validation enabled and zero validation errors.
Visual NMS HDR, native Windows, frame generation and coexistence with other graphics overlays
remain unqualified.

The production Windows build and all 106 workspace tests pass. Package verification checks all file
hashes and archive contents. Desktop HDR was enabled during the live checks, but NMS's captured
swapchain was SDR and its saved HDR mode was Off; desktop HDR alone does not qualify game HDR output.

Vegetation emission checks use the production archive reader, scene/entity parser and material-mask
reader against current vanilla assets. All spawnable surface-table selections complete without
unresolved glow facts. Loaded-world captures identify glowing grass and flora on Dorent Mito and
Exleywoo Major, and glowing grass with non-glowing flora on Tapons, agreeing with operator
observations. The grass shader gates its emission by night and a moving pulse. Ordinary grass and
non-emissive trees provide negative controls. Model tests cover independent Require/Exclude
predicates and unknown facts; cache tests cover upgrading summaries acquired before glow was
requested. Visual filters assume standard game assets. Asset format and bounds checks remain
enforced. A fresh-process native search for Black grass, Require glowing grass and Exclude glowing
flora completes with Olarac Minor after 1,560 systems; the operator confirms no freeze. It uses 16
workers, performs cold object loads and reports no search error. Other glow combinations remain
unqualified. An isolated Proton asset replay produces identical facts for 100 models while reducing
cold model-resolution time from 98.0 seconds to 0.64 seconds. This measures the asset path, not
whole-search throughput. The focused vegetation contract records the scope and separate
atmospheric-bubble evidence.

Grass classification replay uses production capture, texture decoding, world LUTs and compiled
predicates over eight retained loaded worlds. Ohnniat XVII's confirmed grass shader tint reproduces
the reported Black false positive; the corrected rule rejects Black, Grey and White across all nine
possible cross-grass materials. The same outcome holds with its source tint scaled from 0.8 to 1.2.
Neutral-grey probing of the same LUT identifies the mild warm cast. Bradford adaptation for hue
naming classifies Ohnniat XVII as Green across all nine materials and the brightness variations,
rejecting Yellow and every neutral choice. Sybarryil remains Green, Antwithe Guro White, Ladonnea
Delta Magenta, Ixha
Delta and Apaeumasan Gaha Black, and Iomi 84/C9 Black/Grey. The source and graded main tint guard
against texture-darkened colour being mistaken for neutral. Separate graded-material qualification
keeps slight tints without accepting every muted colour. Neutral brightness overlaps remain intact. Olarac Minor’s Vibrant bright LUT clips the estimated
dark green tint to a neutral shadow. Three linear-exposure daylight probes preserve its consistent
Green identity, rejecting Black, Grey and White across nine source-brightness variations. The other
seven reference classifications are unchanged. Synthetic controls cover shadow clipping across six
hue families without undoing real desaturation or highlight clipping; the actual monochrome LUT
still produces a neutral result. All 85 vanilla LUTs resolve. Fresh native population validation of
this daylight-probe correction remains pending.

All twelve vanilla grass texture medians agree with an independent Pillow decoder, covering BC1
and BC7. Unit controls cover transparent padding, linear sampling, truncated/unsupported textures,
material disagreement, grassless facts and separate Grey/Black/White predicates. Colour controls
span the hue circle, several lightness/texture-brightness levels, hue removal and colour introduction
through grading. Mild warm/cool cast controls preserve named hues, including true yellow and
yellow-green; strong colour effects and hue rotations remain effective. Adaptation math matches
independent published D50-to-D65 reference vectors. Shared archive bounds remain enforced. These synthetic controls
and seven observed worlds do not establish population-wide precision or recall.

Grass acquisition rejects impossible main-tint hues before object-table scans. Its cached colour
bounds can reject a criterion but cannot establish grass presence or a positive match; surviving
candidates still acquire the selected tables and material brightness. Other cheap palette colours
and floating-island rejection precede material classification. Common tint/grading work is prepared
once per planet. Object-table cache hits use shared reads; misses retain exclusive loading.

Paired release microbenchmarks on the healthy Linux host preserve all seven reference outcomes and
220,320 classifications across all 85 vanilla LUT entries. At 16 workers, cached object lookups
improve from 218 to 69 ns per lookup (about 3.2×); one-worker reads increase from 28 to 30 ns.
Nine-material coloured-reference classification improves about 13×, and the neutral cross-grass
reference about 2.1×. Single-material neutral cases increase by roughly 30 ns. System-fact retention
adds about 0.2 µs per new record in the host harness; a full 16,384-record no-match startup scan takes
about 1.3 ms. These component measurements exclude native generation, cold asset loading and
in-game frame pacing. They do not establish a whole-search speedup or the effect of rarer matches.
A paired native before/after performance check remains pending.

The reported glow/grass-colour query passes a fresh native search after installation. Texture
medians are an appearance estimate, not rendered surface coverage; scene lighting, exposure, procedural texture
recolouring, distance-dependent blending and HDR grading are not reproduced.

Passive capture of a same-system flight between two planets reproduces false session resets as the
live generator's pending-task count changes. Application, lifecycle, language, difficulty, resource
identities and all other generator configuration bytes remain stable. Replaying all nineteen captured
snapshots through the corrected production identity function preserves one configuration identity;
the old comparison produces fifteen. The pending-task vector and RNG are excluded from session
identity, while borrowed resource pointers, vector headers and generation options remain checked.
Regression tests cover both sides of that boundary. A fresh in-game flight with an active search
remains pending after installation; captured replay does not establish that live acceptance.

Coordinate export tests cover the reported internal-address example, signed region bounds, leading
zeroes, selected planet identity and galaxy omission. Result cards expose separate twelve-digit
portal-code and colon-separated galactic-coordinate clipboard actions. The supplied example agrees
with the external portal converter in both directions; a fresh in-game button click remains pending.

## Search-engine 7.06 qualification before the overlay refactor

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
- Guide preset routing, empty-slot errors, ordinary Steam self-start and native Quit to Desktop
  passed for the earlier standalone UI. Overlay visibility now changes only through F7; the earlier
  automatic window-opening behavior is removed.
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

That release’s host suite had 70 Rust tests. The parent repository retains native probes, screenshots, exact
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

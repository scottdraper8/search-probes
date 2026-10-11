# Validation and performance

## Supported scope

The product uses the exact executable identity in README.md. Runtime validation uses Steam Proton
on an i9-13900KF, with the permitted save's purple systems unlocked. Native Windows desktop has
not been tested. Game updates require native-interface validation before the executable guard can
be changed. Other native hook mods and alternate WinMM proxies are outside the tested scope.

## Visible filter qualification

System composition predicates evaluate the system's owned planet observations. Include/exclude
biome and size masks clear conflicting selections; subtype choices retain their biome context and
group equivalent native variants. All mode requires each included value somewhere in the system;
Any mode combines active composition criteria as alternatives, including exclusions. An excluded
biome/subtype pair matches one planet, while different included criteria can match different planets.
System-only searches retain system destinations and names. Size uses solar input; biome, subtype,
water and dissonance reuse the native query. Dissonance uses the normal-difficulty corrupted-Sentinel
entry as a world property; the planet Sentinel control follows the selected combat difficulty.

Each subsection has independent All/Any criterion matching. Top-level sections remain conjunctive, and
empty subsections impose no requirement. Native dependent checks, including water presence for
water colour, remain attached to their criterion. Owned partial facts retain unknown values;
negative system predicates wait until all planets are known unless a positive counterexample is
found. A successful Any subsection skips its remaining acquisitions. Model controls cover mixed
systems, contextual subtypes, variant grouping, water truth tables, exclusions, cache reuse,
persistence and continuation-plan identity. Native decoder checks cover water and difficulty-independent
dissonance. The standalone window uses the production UI and reloads on source changes while NMS is stopped.
The launcher closes/suspends the preview while NMS runs and resumes after exit; watcher controls
exercise edits during suspension and a clean resume without taking gameplay focus.
The updated controls have passed deterministic checks and Windows compilation; fresh in-game
qualification of the composition and subsection modes remains pending.

Unique resources and Vile Brood read the generated `ExtraResourceHints` array before metadata,
colour or object acquisition. The native query builds these hints from selected external object
lists before the planet visitor runs. Reads retain only a resource bit set and a brood boolean;
empty arrays establish absence, while malformed arrays fail explicitly. Unique resource match
selects All selected (default) or Any selected independently of the ordinary resource match control.
Both modes reuse the same captured facts; compiled continuation keys include the selected mode.

Current loaded-planet captures cover Ancient Bones on Exleywoo Major (discovery name Apam Woods),
Vile Brood on ordinary-subtype Hersfiel XVI, and Salvageable Scrap on Sebash Prime. A dissonant Scrap
planet, Sisenil Minor, has no Swarm Debris hint and is a negative control for the compound shortcut.
Current generator configuration supplies `UI_B_SWARM_HINT` through an optional list with corrupted
Sentinels, scrap, biome and probability conditions. The installed explicit Swarm Debris search
returns ten matches after 166 systems within a 1,000-system budget, without a reported error.
Wafford Prime independently has Swarm Debris, Scrap and Brood hints; the operator confirms all
three labels in its tooltip. Production decoding and all seven nonempty selections in both match modes pass replay across 22
counted loaded planets, including cached fact merges, empty hints and the dissonant-Scrap negative.
Unit checks cover independent resource modes, persistence and distinct compiled continuation keys. Installed searches
using Bones selections and Brood Require/Exclude have not been separately exercised.

Player-attacking fauna uses spawnable generated `PlayerPredator` roles, with positive finite group
density. The verified query generates this role array before the visitor runs; the predicate reads
it before metadata, colour or object acquisition and retains only a known boolean. Ordinary
`Predator` and `FishPredator` roles are separate. Captured Arbyis XIX has two player-predator
species and two ordinary predator species; its five loaded neighbours provide ordinary-predator,
fish-predator and empty-role negative controls. Replaying those arrays through the production reader
and compiled Require/Exclude predicates passes, including cache merges. Tests reject malformed
arrays and roles and defer exclusions until the fact is acquired. Species classification is
independent of settings or behaviour mods that suppress attacks. The operator confirms the installed fresh-process filter works in game, including the requested
Require/Exclude check.

Fauna Criteria is a top-level peer of System Criteria and Planet Criteria. Its General,
Aquatic, Land and Airborne subsections are independent: every active subsection must pass.
Within each domain subsection, All mode requires one species to satisfy every active criterion;
Any mode accepts a species meeting one criterion. Body-family/category options are contextual.
General's All/Any mode combines only its quantity, aggression and encounter criteria. Its success
cannot bypass another active fauna subsection or skip its unknown species acquisition. Empty
subsections impose no requirement. Broad domain presence controls support Require and Exclude
without selecting a family. Unknown observations cannot prove absence.
All fauna subsections start expanded. Height inputs remain visible and disabled while their Any
checkbox is checked. The standalone preview shares these controls with the product UI.
Land habitat uses the actual cave tile. Height bounds use either generated discovery gender height
in metres; they do not imply every spawned individual has that height. Rarity uses the discovery
Common/Uncommon/Rare field. There is no aquatic shallow/deep species classification.

The native visitor copies compact species once, without another generation call or retained game
pointers. Cached facts share owned immutable species, preserve unknown/empty distinctions and have
a separate byte cap within the existing structural budget. Default searches acquire no species.
Model controls cover joint-species matching, domain conjunction, heights, unavailable options,
presets and cache ownership. Native controls distinguish actual underwater placement, ordinary air
families and non-air families using flying behaviour engines. The four suitability controls are
absent from model, UI, native query reads and authored mission assets.

Sandworms read the generated population flag. Titan worms identify positive burrow spawns in
selected non-superseded object tables. Brood hints and naturally player-attacking species remain
independent. Tests distinguish disabled burrows and giant travelling worm models. Current assets
and exact native generation disassembly support these separate surfaces.

Passive Eggyon IV capture contains land, flying, underwater and cave roles. The operator confirms
its 14-species discovery count; photographs confirm prawn/shark, flying-lizard and bipedal land
appearance with discovery heights matching the captured gender heights at displayed precision.
Production decoder replay checks present and absent families by domain, alongside the Wafford
Prime and earlier aggression references. Source tests, authored-asset checks and Windows builds
pass. Installed candidate searches and positive/negative worm-world acceptance remain pending;
these passive reads do not qualify the new controls in-game. Arena affinity/stat filters and complete
bioluminescent-fauna classification remain unimplemented pending native resolution evidence.
The focused fauna contract records source witnesses, raw evidence and remaining gates.

Terrain uses positive generated geometry dimensions and enabled shapes, together with positive,
unsuppressed island spawns. It exposes seven formation choices and includes underwater geometry.
Deterministic controls distinguish disabled or zero-sized shapes, subtractive canyons and craters,
additive arches, multiple plateau strata and caves. Mixed choices can settle from inline geometry
without object acquisition; partial cache observations defer island absence until the tables have
been checked. Current source and retained generator replay cover those acquisition and cache paths.
A fresh visual check of each terrain choice remains pending.

Biome subtype labels depend on the selected biome. Equivalent native archetypes share a label;
ambiguous meanings are omitted for mixed biomes. Changing biome clears its dependent subtype.
Current vanilla archetype and placement assets support the labels, with native-index and grouping
controls.

Weather exposes cadence (including None for no storms), extreme storm hazards and storm families. Current
executable scheduling returns before interval/fallback selection when frequency is zero. Query facts
settle occurrence and cadence immediately; absence or normal intensity settles extreme-storm
absence. Positive severity candidates acquire owned ambient/storm hazard profiles from the loaded
weather override/default tables. Both system and planet checks use that same predicate, including
Lush extreme rainstorms and mild gravitational/bubble weather. Storm families follow the emulated
17-entry HUD warning table; Bubble storm is a descriptive label supported by its atmospheric asset.
Deterministic tests cover native offsets/default fallbacks, table bounds, unknown exclusions,
subsection modes, cache parity and removed serialization keys. Fresh live weather-result and visual
checks remain pending. Investigation evidence is recorded in the repository's focused weather
contract.

Sky, cloud, fog, horizon, night sky and water use acquired canonical appearance sets. Clear daylight
anchors apply to daytime colours, clear night to night sky and intrinsic body colour to water.
The shared classifier applies tone mapping, world grading and mild-cast adaptation. Raw palette
indices cannot establish a colour match. Synthetic controls cover named hues, neutral lightness,
grading, and each capture binding. Current vanilla inputs and grading provide constructive witnesses
for exposed choices; they are not a population-wide rendered-colour oracle.

Flora colour combines discoverable surface mesh materials and correlates each sampled UV with its
vertex colour and local triangle area. Current scene, geometry, texture, material and shader assets
support diffuse recolouring, ground matching, primary and secondary texture blending, alpha gates
and vertex RGB/alpha. Deterministic controls cover those interactions and malformed inputs. Recipe
selections not acquired by the query are uncertainty bounds: only colours shared by possible variants
qualify a material. This can omit colours unique to one selected recipe. Sampling estimates authored
surface appearance, not camera-dependent coverage, lighting, animation, translucency or HDR output.
Independent visual validation of the combined flora filter remains pending.

Acquisition follows the selected criteria and rejects cheap nonmatches first. Flora observations
retain proven present and absent colours while leaving other choices unknown. Positive matches can
stop before checking remaining materials; impossible requested colours can stop a variant scan.
Within one model evaluation, meshes reuse prepared materials and lazily composed texture recipes.
An exact RGB memo shares colour classification across meshes under the same world grading.
Majority decisions track the requested colour categories; other categories remain unknown.
Controls cover distinct mesh colours, primary/secondary namespaces, transparency, weighted
majority boundaries, multiselect absence and subsequent acquisition of other cached choices.

Healthy-host release component measurements put generated terrain evaluation at about 54 ns per
planet and one canonical colour anchor at about 0.5 µs. These checks use existing generated data.
Flora colour requires authored mesh, texture and material processing only when selected. A retained
palette replay of positive surface-source models measures Black classification medians of 2.1–2.4 ms
per model, 95th percentiles of 13–27 ms and a maximum of 221 ms. Green-or-White medians are 1.8–2.7 ms,
with 95th percentiles of 15–31 ms and a maximum of 178 ms. Paired evaluations with unchanged inputs
and samples reduce total calculation time by 43% and 24% respectively, preserving every requested
colour decision. Full-colour controls preserve the complete observations. Repeated observations
take about 2 µs per model. Model retention counts shared allocations once and has a 256 MiB bound;
material, texture and result caches have separate 32, 128 and 16 MiB bounds. Expensive classification
runs outside the shared asset-cache lock. Each worker's model calculation retains at most 16 MiB
of prepared texture entries and 65,536 exact RGB classifications, released after that calculation.
Cold archive preparation, native generation, actual world
source selection and frame pacing are outside the per-model measurements. These results do not
establish a whole-search slowdown; a fresh native throughput and visual check remains pending.

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

The production Windows build and workspace tests pass. Package verification checks all file
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
and eight observed worlds do not establish population-wide precision or recall.

Grass acquisition rejects impossible main-tint hues before object-table scans. Its cached colour
bounds can reject a criterion but cannot establish grass presence or a positive match; surviving
candidates still acquire the selected tables and material brightness. Requested sky, fog and water appearance checks, generated terrain, and grass rejection
precede flora material classification. Geometry matches omit island-table acquisition when it is
not needed to settle the selected terrain alternatives. Common tint/grading work is prepared
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

The reported glow/grass-colour query passes a fresh native search after installation. Grass texture
medians estimate material appearance; scene lighting, exposure, distance-dependent blending and
HDR grading are not reproduced.

Passive capture of a same-system flight between two planets reproduces false session resets as the
live generator's pending-task count changes. Application, lifecycle, language, difficulty, resource
identities and all other generator configuration bytes remain stable. Replaying all nineteen captured
snapshots through the corrected production identity function preserves one configuration identity;
the old comparison produces fifteen. The pending-task vector and RNG are excluded from session
identity, while borrowed resource pointers, vector headers and generation options remain checked.
Regression tests cover both sides of that boundary. Search progress and result cards retain live
matches through cancellation and resource refreshes. Application travel/loading transitions suspend
work; save selection, title, shutdown and global save loading invalidate the search session. A search
keeps its original galaxy and pauses while the player visits another galaxy. Deterministic controls
cover live partial-result publication, travel suspension, session clearing and frontier retention.
Passive native capture of an operator-initiated galaxy round trip confirms that the active search
pauses outside its starting galaxy and resumes on return with the same origin and retained progress.
Destination-system loading also suspends work and resumes it after gameplay becomes available;
the checked count remains monotonic and neither runtime nor search diagnostics report an error.
This run has no matches, so live result-card interaction and cancellation with partial matches
remain pending in-game checks. Independent same-galaxy travel and save-session reset checks also
remain pending for this candidate.

Search-rate timing accumulates running time while gameplay resources are available in the search's
galaxy. Deterministic clock controls cover loading, a different galaxy, return, completion,
cancellation, continuation and a fresh search. Native work statistics remain independent of this
clock. Passive samples of the operator's glow/grass query show comparable work per batch second
before and after travel, with no additional object-table loads during a subsequent uninterrupted
sample. A fresh in-game travel check of the corrected counter remains pending.

Coordinate export tests cover the reported internal-address example, signed region bounds, leading
zeroes, selected planet identity and galaxy omission. Result cards expose separate twelve-digit
portal-code and colon-separated galactic-coordinate clipboard actions. The supplied example agrees
with the external portal converter in both directions; a fresh in-game button click remains pending.

System-only queries return one system address per match for every population type. Host tests
cover live and cached result parity, generated-name retention, cancellation, planet criteria, target
identity and coordinate exports. Native mission definitions pass the pinned compiler round trip with separate planet and system
scan conditions. System destinations use the game's local scan-event condition to complete on arrival.
Operator testing confirms system destination cards and native completion on arrival in an empty
system. Read-only capture and isolated execution of the native system-name routine reproduce
Vustvuet for the operator's visited result, matching Discoveries. The ten captured result addresses
all produce bounded system names. The operator confirms corrected names in the fresh process.

Navigation titles distinguish system and planet destinations. Descriptions, instructions and HUD
objectives use native scan-target name substitutions; mission ID and seed select the saved route.
The authored Log and sequence-formatting settings pass asset-contract and pinned compiler checks.
The operator confirms the mission wording in game; a separate saved-mission reload check remains pending.

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

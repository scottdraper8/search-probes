# Search Probes

Find No Man's Sky systems and planets by their generated properties, then set a destination or
copy their portal codes. Search Probes provides an in-game overlay, saved presets and parallel
search workers.

## Supported game

Search Probes targets **No Man's Sky 7.06**, Windows x64 Steam build **25732212**, with executable
SHA-256 `13d5060d4efb9d2a6a6b1b349bc4257231056cc2a055df4bb15d816262cc3499`.
Native initialization verifies this build. Proton is the qualified runtime environment.
See [validation and performance](VALIDATION.md) for compatibility, test coverage and measured limits.

## Search

Press **F7** in NMS to open or hide Search Probes. Move and resize the overlay as needed. While
open, it captures keyboard and mouse input; F7 returns control to the game. Switching applications
releases input capture, and returning to NMS restores the visible overlay.

Choose your criteria and click **Search**. Each subsection offers **Match all selected criteria**
(default) or **Match any selected criteria**. All requires every active criterion in that subsection;
Any requires at least one, including exclusion criteria. Different top-level sections must all match.
Nested fauna subsections each count as one criterion in their parent Fauna section.
**Any** in a dropdown skips that field. Multiple values within ordinary planet and system-profile
fields are alternatives; resource lists have independent All/Any selection controls.

- **Systems to search:** choose a budget from 1,000 to 1,000,000,000 systems. The default is 10,000.
- **Results:** choose 1–100 matches. A search ends at its result cap, system budget or galaxy exhaustion.
- **Workers / Game time per batch:** control the search's share of processing time. The time budget
  is a scheduling target; individual native calls run to completion.
- **Cancel:** keeps completed matches and the continuation position.

Search begins around your current region and traverses generated region lists outward. Results
follow traversal order. Matches appear as they are found and can be selected as destinations while
the search continues. Cancelling keeps those matches. The previous results remain available until
a new search finds matches. Loading during system travel pauses work; it resumes after arrival.
Visiting another galaxy pauses the original search until you return to its galaxy.
Result cards show generated names and identify planets or systems. Selecting only system criteria
produces one result per matching system.

Repeating the same criteria continues from the previous position within the same galaxy. Select
**Restart from current location** to begin around your new position. Search history lasts for the
current game session and clears when a save is loaded or language or combat difficulty changes.

## Criteria

System criteria cover population, dominant lifeform, economy, conflict and planetary composition.
Planet criteria cover biomes, environment, weather, resources, colours, size and other generated
properties. Some options benefit from a little context:

- **Dominant lifeform:** Gek, Vy'keen, Korvax or None. None matches uninhabited systems. Abandoned
  systems retain their species; choose **Population → Abandoned** to find them.
- **Planetary Composition:** includes or excludes systems by their planets' biomes, contextual
  biome subtypes and sizes. In All mode, every included value must occur somewhere in the system;
  in Any mode, an included value or another active composition criterion can satisfy the subsection.
  Biome and subtype selections describe the same planet. An exclusion criterion passes when no
  planet matches its selected values. Include/exclude biome and size selections clear conflicting
  values automatically; changing a biome clears its dependent subtypes.
- **Water:** requires any water or deep water, or excludes deep water or all water. Under System
  Profile it checks the whole system; under Conditions it checks the target planet. Shallow water
  satisfies Require any water type and Exclude deep water.
- **Dissonance:** requires or excludes systems containing a dissonant planet, independently of
  combat difficulty.
- **Biome → Mega-exotic:** groups the game's Red, Green and Blue biome families.
- **Biome subtype:** offers named features for the selected biome, such as giant trees or spores.
- **Storm frequency:** None selects planets without storms. Low and High select recurring storms;
  select both to accept either frequency. Individual storm intervals vary.
- **Extreme storms:** requires or excludes storms with intensified heat, cold, toxicity or radiation.
  **Contains extreme storms** checks for the same conditions anywhere in a system.
- **Storm type:** selects storm families such as Superheated rainstorm, Blizzard, Toxic rainstorm,
  Radioactive supercell, Firestorm, Wall of flame and Gravitational anomaly. Bubble storm describes
  atmospheric bubble weather during storms; Other storms groups generic storm warnings.
- **Terrain:** selects generated floating islands, mountains, plateaus, canyons, craters, arches
  or caves. Features can occupy part of a planet, including underwater areas; exploration may
  be needed to find them.
- **World colour filter:** selects the assigned world colour grading independently of biome.
  **Default** requires default grading. Options include the filters used by planet definitions
  and the corrupted-Sentinel grading override.
- **Grass colour:** estimates the main grass colour in daylight from grass textures, terrain tint
  and world grading. It requires grass cover and uses the main colour across grass materials.
  Black/Grey and Grey/White overlap slightly for borderline appearances. Recognizable daylight
  hues retain their colour names through dark shading. Night, storms and nearby lights can
  change the visible colour.
- **Glowing grass / Glowing flora:** independent **Any / Require / Exclude** controls under
  Biome & Environment. These identify light-emitting surface grass and other discoverable surface
  plants respectively. Glow is independent of colour and can be most apparent at night.
- **Fauna → Quantity:** selects the planet’s generated fauna abundance level.
- **Aquatic Fauna / Land Fauna / Airborne Fauna:** collapsed subsections with **Has <domain> fauna**
  (Any / Require / Exclude), body family, category, activity time, rarity and minimum/maximum
  discovery height in metres. In All mode, one species must satisfy all selected species criteria;
  in Any mode, one criterion is enough. Land fauna also offers Surface and Cave habitats.
  Categories and body families are specific to each subsection. Each subsection can match a
  different species. The parent Fauna mode combines its direct filters and active subsections:
  All requires each to pass; Any requires only one. Empty subsections impose no requirement.
- **Titan worms:** requires or excludes generated titan-worm burrows.
- **Sandworms:** requires or excludes planets with the giant travelling sandworm population.
- **Player-attacking fauna:** requires or excludes generated species that naturally hunt players,
  independently of predators that hunt other animals. Creature hostility settings and behaviour
  mods can affect whether those species actually attack. Vile Brood and worm encounters have
  independent controls.
- **Unique resources:** Ancient bones, Salvageable scrap or Swarm debris, as listed by the game.
  **Unique resource match** chooses **All selected** (default) or **Any selected**, independently
  of the ordinary Resources match control.
- **Vile Brood:** requires or excludes planets marked **Vile Brood Detected**.
- **Sentinels:** Low / absent, Standard, Aggressive or Corrupted, based on the current combat
  difficulty's native level.

Colour filters estimate appearance with the world's colour grading. Sky and fog use representative
clear-weather colours; water uses its body colour. Flora colour describes colours present on surface
plants in daylight, whose parts and species can differ.
Colour and glow filters assume standard game scenes, materials and textures. Appearance can vary
with lighting, vegetation and asset mods. Expensive criteria require more processing than basic
system filters. Search generation and classification run on parallel workers, but gameplay waits
for each batch to finish. Settings persistence and diagnostic writing run independently of those
batches.

Saved choices that need updating appear as **Unavailable selection**. Choose **Any** or a listed
option to replace them.

## Results and reuse

**Show cached matches first** reuses previously acquired system and planet facts. Continue searching
to obtain more matches. Continuing a full result page replaces it; continuing a partial page adds
matches. The cache holds up to 16,384 systems within a 64 MiB structural budget. Captured species have
an additional byte limit within that budget, so species-heavy records can trigger earlier eviction. A separate cache
holds up to 1,024 object-table summaries. Both clear on save reload, relevant native configuration
changes or explicit history clearing.

Choose **Set destination** on a result to create its navigation mission. Select **Current Mission**
in the Galaxy Map. Planet targets complete when you approach the planet. System targets complete
on entering the destination system.
Abandon the mission in the Log before choosing a different destination. Navigation stays in your
current galaxy.

**Copy portal code** copies twelve hexadecimal glyph digits for the target planet, or the first
planet in a system result. A portal provides arrival in that system. **Copy galactic coords**
copies the system coordinates as
`XXXX:YYYY:ZZZZ:SSSS` for coordinate converters. Use either code with the galaxy shown on the card.

## Presets and settings

Save up to nine named presets. Their order corresponds to **Preset 1** through **Preset 9** in
Catalogue & Guide → Search Probes. Saving updates that slot immediately; removing a preset shifts
following slots. Launch a slot and return to gameplay to run the search. Press **F7** to view
results and choose a destination.

Settings live in `%LOCALAPPDATA%\SearchProbes\settings.json` inside the game's Windows or Proton
user profile. `settings.backup.json` retains a valid previous version. Malformed settings are
preserved as `settings.rejected-*.json` when replacements are saved. Runtime diagnostics are
available in `startup-error.txt`, `current.json` and `overlay.json`.

## Install, update and remove

Close NMS before changing installed files. Extract a release archive into the game directory
containing `Binaries` and `GAMEDATA`. The product files are:

- `Binaries/winmm.dll`
- `Binaries/SearchProbes/installation.json`
- `GAMEDATA/MODS/SearchProbes/`

Check for an existing mod's `winmm.dll` before installing and preserve it. Remove or rename
`GAMEDATA/PCBANKS/DISABLEMODS.TXT` if present. On Proton, set the Steam launch option
`WINEDLLOVERRIDES="winmm=n,b" %command%`. Enable mods and Search Probes in NMS's mod settings,
then restart NMS.

For updates, replace the product files together. For Amethyst, deploy the DLL and installation
manifest through its game root-file deployment and the asset directory through its normal mod
deployment. Use the same release in both locations.

Before uninstalling, abandon any Search Probes navigation mission. Close NMS and remove the three
product locations above. Remove the WinMM launch override if applicable. User settings can be
retained or removed separately.

## Build

Use Rust 1.98.1 and the locked Cargo dependencies. The Windows product is a bootstrap DLL
containing the Rust runtime and egui UI. Controls render into the game's Vulkan swapchain, with
SDR, HDR10 and linear scRGB rendering paths. HDR rendering uses a 200-nit UI white point.
The native crate's `investigation` feature is reserved for development probes.

```sh
cargo test --locked --workspace
# Optional graphics qualification: requires Vulkan, a graphics device and Khronos validation layers.
cargo test --locked -p search-probes-native --features gpu-qualification --test hdr
cargo build --locked --release --target x86_64-pc-windows-gnu \
  -p search-probes-bootstrap
cargo run --locked -p search-probes-package -- package ./dist/SearchProbes-0.2.0
cargo run --locked -p search-probes-package -- verify ./dist/SearchProbes-0.2.0
```

`tooling/windows.Containerfile` supplies the GNU cross compiler for Linux builds. The packager
copies declarative assets, retains dependency licenses, records hashes, verifies the expanded
package and produces a ZIP archive. MBINCompiler supports development schema and roundtrip checks.

## UI development preview

Run `./tooling/preview-ui` from this repository to open an interactive desktop window using the
same UI as the in-game overlay. While NMS is running, the launcher suspends the preview and
closes its window. It resumes after the game exits. With NMS stopped, it rebuilds and reopens after changes to UI or model Rust sources
or their Cargo configuration. A failed build keeps the current window open and prints the compiler
error in the terminal. Window geometry, expanded sections, scrolling and draft settings persist
across reloads in the preview's own storage. Close the window or press Ctrl+C to stop the watcher.

The preview supports editing filters and local presets; searches and navigation run in NMS.
It uses separate settings from the installed mod. The launcher requires Python 3 and Rust;
`./tooling/preview-ui --no-watch` opens once. To run directly with Cargo, use
`cargo run --locked --manifest-path tooling/ui-preview/Cargo.toml`.
The development host has its own Cargo workspace and lockfile.

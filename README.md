# Search Probes

Find No Man's Sky systems and planets by their generated properties, then set a destination or
copy their portal codes. Search Probes provides an in-game overlay, saved presets and background
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

Choose your criteria and click **Search**. Select multiple values within a field to accept any
selected value; selected fields must all match. **Any** skips that field. Resource fields offer
explicit **All** and **Any** selection modes.

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
Result cards show destinations outside your current system.

Repeating the same criteria continues from the previous position within the same galaxy. Select
**Restart from current location** to begin around your new position. Search history lasts for the
current game session and clears when a save is loaded or language or combat difficulty changes.

## Criteria

System criteria cover population, dominant lifeform, economy, conflict and planetary composition.
Planet criteria cover biomes, environment, weather, resources, colours, size and other generated
properties. Some options benefit from a little context:

- **Dominant lifeform:** Gek, Vy'keen, Korvax or None. None matches uninhabited systems. Abandoned
  systems retain their species; choose **Population → Abandoned** to find them.
- **Biome → Mega-exotic:** groups the game's Red, Green and Blue biome families.
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
- **Sentinels:** Low / absent, Standard, Aggressive or Corrupted, based on the current combat
  difficulty's native level.

Grass colour estimates daylight appearance; other colour fields describe generated palettes.
Colour and glow filters assume standard game scenes, materials and textures. Appearance can vary
with lighting, vegetation and asset mods. Expensive criteria require more processing than basic
system filters.

Saved choices that need updating appear as **Unavailable selection**. Choose **Any** or a listed
option to replace them.

## Results and reuse

**Show cached matches first** reuses previously acquired system and planet facts. Continue searching
to obtain more matches. Continuing a full result page replaces it; continuing a partial page adds
matches. The cache holds up to 16,384 systems within a 64 MiB structural budget. A separate cache
holds up to 1,024 object-table summaries. Both clear on save reload, relevant native configuration
changes or explicit history clearing.

Choose **Set destination** on a result to create a **Search Probe Target** mission. Select
**Current Mission** in the Galaxy Map and approach the marked planet. Abandon the mission in the
Log before choosing a different destination. Navigation stays in your current galaxy.

**Copy portal code** copies the planet's twelve hexadecimal glyph digits. **Copy galactic coords**
copies its system coordinates as `XXXX:YYYY:ZZZZ:SSSS` for coordinate converters. Use either code
with the galaxy shown on the result card.

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

Before uninstalling, abandon any Search Probe Target mission. Close NMS and remove the three
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

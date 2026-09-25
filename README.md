# Search Probes

Search No Man's Sky systems and planets by their generated properties. The runtime, search
workers, controls, and desktop UI are Rust. No Python, NMS.py, pyMHF, or WebView is required.

The release targets the exact game build below. Bounded runtime validation covers standalone
startup, searches, purple systems, Guide presets, destination missions, save reload and shutdown.
See [validation and performance](VALIDATION.md) for measured limits.

## Supported game

The native interface targets **No Man's Sky 7.04**, Windows x64 Steam build **25442159**.
The executable must match SHA-256
`b7913f268dfc62386b6b68f524bfc8ade4a44a9f4fbad39085b7bf51be3680cb`.
An update stops native initialization until a matching version is validated. Proton is the
validated runtime environment. Native Windows desktop operation has not been tested.

## Search

Press **F7** in NMS to open Search Probes. Select criteria and start a search. Starting returns
focus to NMS so gameplay can continue. Under Proton the controls may hide during a search;
press F7 to reopen them. Pausing NMS or leaving gameplay pauses search progress.

- **Systems to search:** 1,000; 10,000; 100,000; 1,000,000; 10,000,000; 100,000,000;
  1,000,000,000. The default is 10,000.
- **Results:** 1–100 matches. A search ends at its result cap, budget, or galaxy exhaustion.
- Select multiple values in a field to accept any selected value. All selected fields must match.
  **Any** omits that predicate and its optional acquisition work.
- Resources have explicit **All** and **Any** selection modes.
- **Cancel** keeps completed matches and the continuation position.
- Worker count and the gameplay time budget control the search's share of processing time.
  The budget is a scheduling target; an individual native call cannot be interrupted.

Search begins around the player's current region and traverses actual generated region lists
outward. It is independent of the Galaxy Map's loaded search graph and does not move the player.
Results follow traversal order and are not guaranteed to be the closest possible matches.

A repeated filter continues from its previous position within the same galaxy, even after moving
elsewhere. Explicit restart begins around the current position. The latest 128 filter/galaxy
continuations are retained. Searches and continuation history last for the current game session.
Loading a save or changing relevant native configuration clears them.

## Reuse and results

A bounded cache keeps acquired facts for up to 16,384 systems, including nonmatches. It does not
collect every property of every planet. Unknown facts cannot satisfy a filter. Cached results
appear before live work, with a control to continue searching. Continuing a full result page
replaces that page with new matches; continuing a partial page adds matches. The cache has a conservative
64 MiB structural budget; it does not grow with the selected billion-system budget. A separate
cache holds up to 1,024 small object-table summaries, with no retained native pointers. Both caches
clear on save reload, relevant native configuration changes, or explicit history clearing.

Colours describe generated inputs rather than rendered pixels. Lighting, grading, vegetation
variation and reflections can change the appearance. Floating islands checks the generated
object flag. Expensive criteria take more work than basic system filters.

Choose **Set destination** on a result to create a **Search Probe Target** mission. Select Current Mission
in the Galaxy Map and approach the marked planet. Abandon that mission in the Log before choosing
a different destination. Navigation stays in the current galaxy.

## Presets and Guide

Save up to nine named presets in the Rust UI. Their numbered order corresponds to **Preset 1**
through **Preset 9** in Catalogue & Guide → Search Probes. Saving updates that slot immediately.
Removing a preset shifts the following slots. Launch a slot and return to gameplay to let the search
run. Results open in the Rust UI; choose the result you want to navigate to. Empty slots open the
controls with an explanation.

Settings are stored in `%LOCALAPPDATA%\SearchProbes\settings.json` inside the game's Windows or
Proton user profile. A valid previous version is kept as `settings.backup.json`. Unreadable
settings are preserved; saving replacements retains malformed files as `settings.rejected-*.json`.
`startup-error.txt` records initialization failures. `current.json` contains bounded runtime status.

## Install, update and remove

Close NMS before replacing files. Extract a release archive into the game directory containing
`Binaries` and `GAMEDATA`. The product files are:

- `Binaries/winmm.dll`
- `Binaries/SearchProbes/SearchProbes.exe`
- `GAMEDATA/MODS/SearchProbes/` (Guide, mission and localization patches)

Do not overwrite another mod's `winmm.dll`. A shared proxy chain is not supported. Remove or
rename `GAMEDATA/PCBANKS/DISABLEMODS.TXT` if present. On Proton the application-local WinMM proxy
requires the Steam launch option `WINEDLLOVERRIDES="winmm=n,b" %command%`.
Enable mods and Search Probes in NMS's mod settings, then restart NMS. Disabling asset mods leaves
the DLL search controls available, but prevents Guide and navigation definitions from loading.

For an update, replace the owned product files together. Older Python prerelease runtime folders
must be removed, not merged into this package. Keep other mods and personal game saves intact.
For Amethyst, the DLL and executable belong in its game root-file deployment; the asset directory
belongs in its normal mod deployment. Deploy one consistent version in both locations.

Before uninstalling, abandon any Search Probe Target mission. With NMS closed, remove the three
owned locations above. Remove the WinMM launch override if no remaining mod uses it. User settings
may be retained or removed separately; uninstalling does not require deleting a game save.

## Build

Use Rust 1.98.1 and the locked Cargo dependencies. The Windows product comprises the bootstrap
DLL and UI executable. The native crate's `investigation` feature is for development probes and
must not be enabled in a release.

```sh
cargo test --workspace
cargo build --locked --release --target x86_64-pc-windows-gnu \
  -p search-probes-bootstrap -p search-probes-ui
cargo run --locked -p search-probes-package -- package ./dist/SearchProbes-0.1.0
cargo run --locked -p search-probes-package -- verify ./dist/SearchProbes-0.1.0
```

`tooling/windows.Containerfile` supplies the GNU cross compiler for Linux builds. The packager
copies declarative assets, retains dependency licenses, records hashes, verifies the expanded
package, and produces a ZIP archive. MBINCompiler remains a development schema/roundtrip tool;
it is not shipped and is not part of search execution.

The parent squinchmods repository holds the exact native evidence and bounded runtime scenarios.
No billion-system or 100,000-system runtime benchmark has established an end-to-end completion
time. Ordered warm worker measurements must not be treated as a billion-system performance promise.

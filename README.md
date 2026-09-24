# Search Probes

A No Man's Sky mod for finding systems and planets with specific generated properties.

Launch the game normally and press **F7** to open the search form. Choose filters, run a search,
select a result, and navigate. The target appears as an ordinary **Search Probe Target** mission
in the Log. Abandon that mission before selecting a different target.

Choose **Maximum systems** from the dropdown: **1,000**, **10,000**, **100,000**, **1,000,000**,
**10,000,000**, **100,000,000**, or **1,000,000,000**. New searches default to **10,000**.
Existing preset budgets are rounded to the numerically nearest dropdown value on load and save;
ties round up. For example, 5,000 becomes 1,000 and 20,000 becomes 10,000. Migration retains the
original preset file as a backup. **Results to retain** accepts 1 through **100** matches per search.

Search Probes checks nearby indexed systems first, then generates additional
regions outward from the starting location. It can continue beyond the Galaxy Map's loaded graph
without moving the player. Each system is checked once; results follow search order and are not
guaranteed to be globally nearest.

A search stops when it finds the requested number of results or reaches its system budget. **Cancel**
keeps matches already found. Long searches have no time limit, but NMS must remain open with gameplay
updates running: opening the Galaxy Map or otherwise pausing gameplay pauses the search. Searches
resume with gameplay; they do not survive quitting NMS. Changing galaxies ends an active search.

Saved presets also appear in the Catalogue & Guide after the next launch. The mod does not
replace Quick Actions or multiplayer interactions. Presets are stored under
`Binaries/SearchProbes/app/.resident-search` in the game installation.
If a Guide search finds no exact match, abandon its probe in the Log before retrying; the form
shows the search status. The Guide snapshot updates on the next game launch.

The Floating islands filter is the canonical `floating_islands` criterion: it reads the resolved
object-list `IsFloatingIsland` result. Terrain families remain available as an exact diagnostic,
but the three `FloatingIslands*` values are not offered as a second island selector.
Required resource selections are combined with AND across three survey-resource slots. Target-
planet filters apply to the same planet; system filters can describe other planets in the system.

The form exposes base vegetation colours (Grass, Plant, and Leaf), water base colour, and the
daytime sky and atmosphere colours. These are generated inputs, not guarantees about rendered
pixels: lighting and colour grading are excluded, vegetation can contain multiple colours, and
water reflects the sky. Weather type remains available, with Storms grouped as None,
Non-extreme, or Extreme. Older protocol fields that are not form controls remain visible as
additional saved filters so presets retain their exact criteria.

## Source layout

- `src/search_probes`: form, criteria, generated-data decoding, native integration, mission
  navigation, Guide presets, and adapter definitions.
- `native-bootstrap`: application-local WINMM forwarder and embedded-runtime startup.
- `dependencies`: pinned runtime dependencies.

Native integration is version-qualified and fails closed when the executable changes. This
source targets No Man's Sky 7.04 on Windows and Steam Proton.

Build, packaging, installation, tests, client controls, and retained investigation evidence live
in the parent [squinchmods repository](https://github.com/scottdraper8/squinchmods), under
`games/no-mans-sky/tooling`. This repository contains mod source only.

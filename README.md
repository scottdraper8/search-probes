# Search Probes

A No Man's Sky mod for finding nearby systems and planets with specific generated properties.

Launch the game normally and press **F7** to open the search form. Choose filters, run a search,
select a result, and navigate. The target appears as an ordinary **Search Probe Target** mission
in the Log. Abandon that mission before selecting a different target.

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
source targets No Man's Sky 7.03.1 on Windows and Steam Proton.

Build, packaging, installation, tests, client controls, and retained investigation evidence live
in the parent [squinchmods repository](https://github.com/scottdraper8/squinchmods), under
`games/no-mans-sky/tooling`. This repository contains mod source only.

# Search Probes

A No Man's Sky mod for finding nearby systems and planets with specific generated properties.

Launch the game normally and press **F7** to open the search form. Choose filters, run a search,
select a result, and navigate. The target appears as an ordinary **Search Probe Target** mission
in the Log. Abandon that mission before selecting a different target.

Saved presets also appear in the Catalogue & Guide after the next launch. The mod does not
replace Quick Actions or multiplayer interactions. Presets are stored under
`Binaries/SearchProbes/app/.resident-search` in the game installation.

Terrain families and generated objects are separate filters. For example, floating-island
terrain does not imply that the generated object lists contain floating-island objects.
Required resource selections are combined with AND. Target-planet filters apply to the same
planet; system filters can describe other planets in the system.

## Source layout

- `src/search_probes`: form, criteria, generated-data decoding, native integration, mission
  navigation, Guide presets, and adapter definitions.
- `native-bootstrap`: application-local WINMM forwarder and embedded-runtime startup.
- `dependencies`: pinned runtime dependencies.

Native integration is version-qualified and fails closed when the executable changes. This
source targets No Man's Sky 7.01 on Windows and Steam Proton.

Build, packaging, installation, tests, client controls, and retained investigation evidence live
in the parent [squinchmods repository](https://github.com/scottdraper8/squinchmods), under
`games/no-mans-sky/tooling`. This repository contains mod source only.

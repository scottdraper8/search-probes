# Search Probes

A No Man's Sky mod that searches generated systems, planets and fauna without visiting them.
It provides a configurable overlay (F7 by default), coordinate copying, in-space interstellar
guidance and Galaxy Map destinations. The runtime and UI are Rust; parent-repository tooling is not
part of the distributed mod.

Saved presets appear in the in-game Guide under Search Probes; unused preset capacity is hidden.

## UI themes

The Theme control sits immediately left of UI Hotkey. Search Probes Dark preserves the original
overlay palette, Search Probes Light is its light counterpart, and both remain at the top of the
menu. The remaining themes are alphabetical. The screen-oriented catalog follows the maintained
[KDE KSyntaxHighlighting themes used by Kate](https://invent.kde.org/frameworks/syntax-highlighting/-/tree/master/data/themes),
with Material Dark and Light retained from the
[Material 3 color system](https://m3.material.io/styles/color/the-color-system/). The non-screen
Printing theme is deliberately omitted. [Tinted Theming/Base16](https://github.com/tinted-theming/home)
is the interoperability reference for published palette collections; named palettes retain their
upstream character while their UI roles are normalized where necessary for readable controls.
The experimental No Man's Sky theme combines the game's dark slate, white, red, cyan and gold UI
language with [NMS GeoSans 0.2](https://github.com/NMSCD/No-Mans-Sky-Universal-Font), an
OFL-1.1-licensed corrected typeface from NMSCD. The normal UI fonts remain available as glyph
fallbacks.

UI code consumes semantic color roles instead of raw colors. The complete color inventory is
centralized in `crates/ui/src/theme.rs`:

| Role group | Roles |
| --- | --- |
| Surfaces | `background`, `surface`, `surface_container`, `surface_container_high` |
| Surface content | `on_surface`, `on_surface_variant`, `link`, `outline` |
| Actions | `primary`, `primary_container`, `on_primary_container` |
| Selection | `selection`, `on_selection` |
| Status | `error`, `success`, `success_container`, `success_outline`, `warning`, `warning_container`, `warning_outline` |

Every palette is tested against [WCAG 2.2 minimum text contrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum)
and [non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast). High Contrast
Dark and Light additionally meet the WCAG enhanced 7:1 target for primary text pairs. The role
vocabulary follows the [W3C Design Tokens format](https://www.designtokens.org/tr/2025.10/format/)
and Material's paired `color`/`on-color` convention; it is not tied to either file format.

The native runtime currently targets public NMS 7.14 (Steam build `26540391`) and rejects other
executable identities. The exact executable fingerprint is maintained in `crates/model/src/supported.rs`.

Positive fauna filters produce one Animal result for each matching generated species and include
the planet's copyable coordinates. NMS does not attach the exact species/genus naming identity to
the remote role record, so the destination mission resolves and displays the generated animal name
after the target system is loaded. The mission remains active until that exact species is scanned.
A fauna absence filter has no animal to target, so its result and destination remain planet-level.
Each result card renders the destination's 12 portal glyphs from the same portal code used by its
copy action. The glyph artwork comes from the MIT-licensed
[NMSCoordinates](https://github.com/Kevin0M16/NMSCoordinates) project and is tinted with the active
theme's secondary text color.

## Agent instructions

- Do not commit or push without explicit permission. Preserve unrelated work.
- Keep development tooling in `games/no-mans-sky/tooling/`, outside this submodule.
- Do not recreate removed tests, one-off probes, investigation archives or progress logs.
- Game facts and oracles: [NMS wiki](../../../../.agent-docs/games/no-mans-sky/wiki/README.md).
  The [Arena rank coverage note](../../../../.agent-docs/games/no-mans-sky/search-probes-unresolved.md)
  describes why some remote underwater rank checks remain unresolved.

Run commands from squinchmods root unless stated otherwise. Create/select a task tranche with
`tooling/squinch tranche create <task>`; keep task inputs and generated output there.

### Open and close NMS

```sh
python3 games/no-mans-sky/tooling/client/launch-latest-save.py --interactive
```

Keep stdin open with a terminal. Inspect Steam after launch; choose **Local Save** for a Cloud
Conflict, as authorized by the user. Inspect each menu before sending input: parallax moves targets.
Select **Galaxies 1–50** by name and verify loaded identity; never assume a row number. The helper
provides a controller and startup checkpoint, not automatic save selection. Do not move the character
or travel. Loading and stationary observation are authorized. If movement is required, close NMS and
ask the user. Close NMS before ending an unattended session; terminate `NMS.exe` if necessary.
Never replace installed files while NMS is running.

### Search without the UI

```sh
tooling/squinch --tranche <task> nms-investigate search status --json
tooling/squinch --tranche <task> nms-investigate search start --criteria <tranche>/criteria.json --systems 1000 --results 100 --json
tooling/squinch --tranche <task> nms-investigate search cancel --json
```

NMS needs a loaded save. Omit `--criteria` to use current settings; `--settings` accepts full settings.
`configure` saves settings without searching. Commands return JSON results, progress and metrics;
`--no-wait` returns a queue acknowledgement.
These commands use the runtime command queue without focusing the window or navigating. Inspect
status after a timeout before retrying. Preserve explicit unknowns. Do not benchmark a degraded host
or run huge unattended soak searches.

### Live UI development

```sh
games/no-mans-sky/tooling/runtime/preview-ui
games/no-mans-sky/tooling/runtime/preview-ui --fixtures 25
```

The preview shares the product UI and reloads after source changes. `--fixtures COUNT` populates it
with that many representative results; the fixture patterns repeat with unique identities. **Stop
it while NMS is running**; it must not open windows or steal game focus. `--no-watch` opens one
preview.

### Build and package

Use Rust 1.98.1, locked dependencies and the parent tooling's `runtime/windows.Containerfile`.
From this mod directory:

```sh
cargo build --locked --release --target x86_64-pc-windows-gnu -p search-probes-bootstrap
cargo run --locked -p search-probes-package -- package <tranche>/SearchProbes
cargo run --locked -p search-probes-package -- verify <tranche>/SearchProbes
```

The packager honors `CARGO_TARGET_DIR`. With NMS closed, install one consistent package in NMS and
Amethyst, preserving deployment symlinks and checking hashes. The executable guard lives in `crates/model/src/supported.rs`; review native
bindings before changing it after an update.

## License

[PolyForm Strict 1.0.0](LICENSE.txt), matching squinchmods. Preserve dependency/toolchain notices in
`licenses/`. The root `license-policy.json` defines permitted SPDX licenses, required crate notices
and the exact bundled-asset and toolchain notice inventories. `licenses/sources.json` records
supplemental notice provenance. Packaging fails when these records are stale, incomplete or
inconsistent with the locked Windows dependency graph and bundled assets. No Man's Sky and its
assets belong to Hello Games.

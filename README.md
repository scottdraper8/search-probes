# Search Probes

A No Man's Sky mod that searches generated systems, planets and fauna without visiting them.
It provides an F7 overlay, coordinate copying and in-game destination guidance. The runtime and
UI are Rust; parent-repository tooling is not part of the distributed mod.

## Agent instructions

- Do not commit or push without explicit permission. Preserve unrelated work.
- Keep development tooling in `games/no-mans-sky/tooling/`, outside this submodule.
- Do not recreate removed tests, one-off probes, investigation archives or progress logs.
- Game facts and oracles: [NMS wiki](../../../../.agent-docs/games/no-mans-sky/wiki/README.md).
  The [single open-issue file](../../../../.agent-docs/games/no-mans-sky/search-probes-unresolved.md)
  records the paused Arena investigation.

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
tooling/squinch --tranche <task> nms-investigate search start --criteria <tranche>/criteria.json --systems 1000 --results 100 --restart --json
tooling/squinch --tranche <task> nms-investigate search cancel --json
tooling/squinch --tranche <task> nms-investigate search continue --results 100 --json
```

NMS needs a loaded save. Omit `--criteria` to use current settings; `--settings` accepts full settings.
`configure` saves settings without searching. Commands return JSON results, progress and metrics;
`--no-wait` returns a queue acknowledgement. `--use-cache` can return `cached`, awaiting continuation.
These commands use the F7 queue without focusing the window or navigating. Inspect status after a
timeout before retrying. Preserve explicit unknowns. Do not benchmark a degraded host or run huge
unattended soak searches.

### Live UI development

```sh
games/no-mans-sky/tooling/runtime/preview-ui
```

The preview shares the product UI and reloads after source changes. **Stop it while NMS is running**;
it must not open windows or steal game focus. `--no-watch` opens one preview.

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
`licenses/`. No Man's Sky and its assets belong to Hello Games.

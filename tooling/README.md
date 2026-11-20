# Search Probes development tooling

The preview is a workspace binary that hosts the production `search-probes-ui` application and the
same session-bound control endpoint used by the in-game runtime. It does not read or write the
installed mod's settings unless that directory is explicitly supplied.

Start a populated preview with an explicit control directory:

```bash
cargo run -p search-probes-preview -- \
  --settings-dir target/preview-control \
  --fixtures 40
```

Inspect and control that exact rendered application from another terminal:

```bash
cargo run -p search-probes-control -- \
  --settings-dir target/preview-control actions
cargo run -p search-probes-control -- \
  --settings-dir target/preview-control command action.json
```

`actions` publishes the current UI state, the accepted semantic actions, and their bounded values.
`command` accepts one strict current `UiAction` JSON object. The preview processes it through the
production `App`, publishes the resulting UI state, and simulates the game-side response for
runtime actions such as starting, pausing, cancelling, configuring, and navigating a search.

The preview and control CLI are Search Probes tooling. Generic No Man's Sky save, archive, and asset
investigation remains outside this repository.

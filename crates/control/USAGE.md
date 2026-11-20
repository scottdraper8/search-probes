# Search Probes control CLI

`SearchProbesControl.exe` is packaged beside `SearchProbes.dll`. It drives the live interface by
semantic actions, not pointer coordinates. The executable defaults to `%LOCALAPPDATA%\SearchProbes`;
use `--settings-dir DIR` to target a different live settings directory.

```text
SearchProbesControl.exe status
SearchProbesControl.exe actions
SearchProbesControl.exe command action.json
SearchProbesControl.exe --no-wait command action.json
SearchProbesControl.exe --timeout 30 command action.json
```

`status` returns the complete published runtime and UI state. `actions` returns every accepted
action and field, the current draft UI state, and the current option catalogs for themes, hotkeys,
notifications, filters, fauna, Arena inputs, exports, and navigation. Biome-subtype options reflect
the currently selected biome, so select a biome and run `actions` again before selecting subtypes.

`command` accepts one strict JSON action. Unknown actions, fields, option labels, and out-of-range
values are rejected. With the default wait behavior, the command returns only after the UI action
and any runtime command it generated have been applied. `--no-wait` returns after the live runtime
queues it.

Examples:

```json
{"type":"set_settings_sidebar","open":true}
```

```json
{"type":"set_page","page":"history"}
```

```json
{"type":"set_theme","theme":"no_mans_sky"}
```

```json
{"type":"set_choice_filter","field":"star","selected":["Yellow","Blue"]}
```

```json
{"type":"set_planet_requirement","flag":"rings","value":true}
```

```json
{"type":"show_toast","toast":{"type":"result_found","count":2}}
```

The option labels within one multi-select filter are alternatives. Separate filters remain
conjunctive, matching the rendered interface. An empty `selected` array restores that filter to
Any. Requirement values are `true` for Require, `false` for Exclude, and `null` for Any.

The control workspace exists only for the current Search Probes process. Startup removes the prior
endpoint, state publication, requests, and responses; commands are also bound to the current game
generation. There are no schema versions, migrations, compatibility readers, or old message forms.

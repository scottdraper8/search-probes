use crate::{
    binding::InputBinding,
    criteria::{Composition, Criteria, Field, PlanetFlag, Resources, SystemFlag, WaterFilter},
    settings::{
        InterfaceSettings, MAX_PRESETS, MAX_RESULTS, MAX_SETTINGS_BYTES, MAX_WORKERS,
        NotificationSettings, Preset, ResultCardSettings, SearchOptions, Settings,
        SettingsRecovery, TimeLimit, ToastPlacement, validate_custom_theme_name,
        validate_frame_budget, validate_name,
    },
    theme::{CustomTheme, ThemeSelection},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const PRESETS_DIRECTORY: &str = "presets";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSettings {
    pub settings: Settings,
    pub recovery: Option<SettingsRecovery>,
}

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

pub fn read_bytes(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid("file exceeds its size bound"));
    }
    Ok(bytes)
}

pub fn read_json<T: DeserializeOwned>(path: &Path, maximum: usize) -> io::Result<T> {
    serde_json::from_slice(&read_bytes(path, maximum)?).map_err(invalid)
}

#[cfg(windows)]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source: Vec<_> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<_> = destination.as_os_str().encode_wide().chain([0]).collect();
    for attempt in 0..25 {
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0
        {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if attempt == 24 || !matches!(error.raw_os_error(), Some(5 | 32 | 33)) {
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    unreachable!()
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

pub fn write_bytes(path: &Path, bytes: &[u8], maximum: usize) -> io::Result<()> {
    if bytes.len() > maximum {
        return Err(invalid("output exceeds its size bound"));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| invalid("output needs an explicit parent directory"))?;
    fs::create_dir_all(parent)?;
    let mut candidate = None;
    for _ in 0..64 {
        let temporary = parent.join(format!(
            ".search-probes-{}-{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                candidate = Some((temporary, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temporary, mut file) = candidate.ok_or_else(|| {
        io::Error::new(io::ErrorKind::AlreadyExists, "no available temporary file")
    })?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn write_json(path: &Path, value: &impl Serialize, maximum: usize) -> io::Result<()> {
    write_bytes(path, &serde_json::to_vec(value).map_err(invalid)?, maximum)
}

fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn unknown_keys(map: &Map<String, Value>, known: &[&str]) -> bool {
    map.keys().any(|key| !known.contains(&key.as_str()))
}

fn value<T: DeserializeOwned>(map: &Map<String, Value>, key: &str, bad: &mut bool) -> Option<T> {
    let raw = map.get(key)?;
    match serde_json::from_value(raw.clone()) {
        Ok(value) => Some(value),
        Err(_) => {
            *bad = true;
            None
        }
    }
}

fn recover_criteria(raw: &Value) -> (Criteria, bool) {
    if let Ok(criteria) = serde_json::from_value::<Criteria>(raw.clone())
        && criteria.compile().is_ok()
    {
        return (criteria, false);
    }
    let Some(map) = object(raw) else {
        return (Criteria::default(), true);
    };
    let mut bad = unknown_keys(
        map,
        &[
            "choices",
            "planets",
            "systems",
            "resources",
            "planet_counts",
            "star_counts",
            "composition",
            "system_water",
            "planet_water",
            "fauna",
            "arena",
        ],
    );
    let mut criteria = Criteria::default();
    if let Some(selected) =
        value::<std::collections::BTreeMap<Field, u64>>(map, "choices", &mut bad)
    {
        criteria.choices = selected;
    }
    if let Some(selected) =
        value::<std::collections::BTreeMap<PlanetFlag, bool>>(map, "planets", &mut bad)
    {
        criteria.planets = selected;
    }
    if let Some(selected) =
        value::<std::collections::BTreeMap<SystemFlag, bool>>(map, "systems", &mut bad)
    {
        criteria.systems = selected;
    }
    if let Some(selected) = value::<Resources>(map, "resources", &mut bad) {
        criteria.resources = selected;
    }
    if let Some(selected) = value::<u8>(map, "planet_counts", &mut bad) {
        criteria.planet_counts = selected;
    }
    if let Some(selected) = value::<u8>(map, "star_counts", &mut bad) {
        criteria.star_counts = selected;
    }
    if let Some(selected) = value::<Composition>(map, "composition", &mut bad) {
        criteria.composition = selected;
    }
    if let Some(selected) = value::<WaterFilter>(map, "system_water", &mut bad) {
        criteria.system_water = selected;
    }
    if let Some(selected) = value::<WaterFilter>(map, "planet_water", &mut bad) {
        criteria.planet_water = selected;
    }
    if let Some(selected) = value(map, "fauna", &mut bad) {
        criteria.fauna = selected;
    }
    if let Some(selected) = value(map, "arena", &mut bad) {
        criteria.arena = selected;
    }
    criteria.enforce_dependencies();
    if criteria.compile().is_err() {
        return (Criteria::default(), true);
    }
    (criteria, true | bad)
}

fn recover_search(raw: &Value) -> (SearchOptions, bool) {
    if let Ok(search) = serde_json::from_value::<SearchOptions>(raw.clone())
        && search.validate().is_ok()
    {
        return (search, false);
    }
    let Some(map) = object(raw) else {
        return (SearchOptions::default(), true);
    };
    let mut bad = unknown_keys(map, &["criteria", "systems", "results", "time_limit"]);
    let mut search = SearchOptions::default();
    if let Some(criteria) = map.get("criteria") {
        let (criteria, recovered) = recover_criteria(criteria);
        search.criteria = criteria;
        bad |= recovered;
    }
    if let Some(systems) = value::<u64>(map, "systems", &mut bad) {
        if crate::settings::BUDGETS.contains(&systems) {
            search.systems = systems;
        } else {
            bad = true;
        }
    }
    if let Some(results) = value::<usize>(map, "results", &mut bad) {
        if (1..=MAX_RESULTS).contains(&results) {
            search.results = results;
        } else {
            bad = true;
        }
    }
    if let Some(limit) = value::<Option<TimeLimit>>(map, "time_limit", &mut bad) {
        if limit.is_none_or(|limit| limit.validate().is_ok()) {
            search.time_limit = limit;
        } else {
            bad = true;
        }
    }
    if search.validate().is_err() {
        (SearchOptions::default(), true)
    } else {
        (search, true | bad)
    }
}

fn recover_notifications(raw: &Value) -> (NotificationSettings, bool) {
    if let Ok(settings) = serde_json::from_value::<NotificationSettings>(raw.clone())
        && settings.validate().is_ok()
    {
        return (settings, false);
    }
    let Some(map) = object(raw) else {
        return (NotificationSettings::default(), true);
    };
    let mut bad = unknown_keys(
        map,
        &[
            "result_found",
            "search_completed",
            "automatic_pause",
            "automatic_resume",
            "duration_seconds",
            "placement",
        ],
    );
    let mut settings = NotificationSettings::default();
    if let Some(selected) = value(map, "result_found", &mut bad) {
        settings.result_found = selected;
    }
    if let Some(selected) = value(map, "search_completed", &mut bad) {
        settings.search_completed = selected;
    }
    if let Some(selected) = value(map, "automatic_pause", &mut bad) {
        settings.automatic_pause = selected;
    }
    if let Some(selected) = value(map, "automatic_resume", &mut bad) {
        settings.automatic_resume = selected;
    }
    if let Some(selected) = value(map, "duration_seconds", &mut bad) {
        settings.duration_seconds = selected;
    }
    if let Some(selected) = value::<ToastPlacement>(map, "placement", &mut bad) {
        settings.placement = selected;
    }
    if settings.validate().is_err() {
        settings.duration_seconds = NotificationSettings::default().duration_seconds;
        bad = true;
    }
    (settings, true | bad)
}

fn recover_result_cards(raw: &Value) -> (ResultCardSettings, bool) {
    if let Ok(settings) = serde_json::from_value::<ResultCardSettings>(raw.clone())
        && settings.validate().is_ok()
    {
        return (settings, false);
    }
    let Some(map) = object(raw) else {
        return (ResultCardSettings::default(), true);
    };
    let mut bad = unknown_keys(
        map,
        &[
            "location_subtitle",
            "portal_glyphs",
            "set_destination",
            "copy_portal_code",
            "copy_galactic_coordinates",
        ],
    );
    let mut settings = ResultCardSettings::default();
    if let Some(selected) = value(map, "location_subtitle", &mut bad) {
        settings.location_subtitle = selected;
    }
    if let Some(selected) = value(map, "portal_glyphs", &mut bad) {
        settings.portal_glyphs = selected;
    }
    if let Some(selected) = value(map, "set_destination", &mut bad) {
        settings.set_destination = selected;
    }
    if let Some(selected) = value(map, "copy_portal_code", &mut bad) {
        settings.copy_portal_code = selected;
    }
    if let Some(selected) = value(map, "copy_galactic_coordinates", &mut bad) {
        settings.copy_galactic_coordinates = selected;
    }
    if settings.validate().is_err() {
        (ResultCardSettings::default(), true)
    } else {
        (settings, true | bad)
    }
}

fn recover_interface(raw: &Value) -> (InterfaceSettings, bool) {
    if let Ok(settings) = serde_json::from_value::<InterfaceSettings>(raw.clone()) {
        let candidate = Settings {
            interface: settings.clone(),
            ..Settings::default()
        };
        if candidate.validate().is_ok() {
            return (settings, false);
        }
    }
    let Some(map) = object(raw) else {
        return (InterfaceSettings::default(), true);
    };
    let mut bad = unknown_keys(
        map,
        &[
            "hotkey",
            "theme",
            "custom_themes",
            "notifications",
            "result_cards",
        ],
    );
    let mut settings = InterfaceSettings::default();
    if let Some(hotkey) = value::<InputBinding>(map, "hotkey", &mut bad) {
        if hotkey.validate().is_ok() {
            settings.hotkey = hotkey;
        } else {
            bad = true;
        }
    }
    if let Some(raw_themes) = map.get("custom_themes") {
        if let Some(themes) = raw_themes.as_array() {
            let mut names = BTreeSet::new();
            for raw_theme in themes.iter().take(crate::theme::MAX_CUSTOM_THEMES) {
                match serde_json::from_value::<CustomTheme>(raw_theme.clone()) {
                    Ok(theme)
                        if validate_custom_theme_name(&theme.name).is_ok()
                            && theme.validate_palette().is_ok()
                            && names.insert(theme.name.to_lowercase()) =>
                    {
                        settings.custom_themes.push(theme);
                    }
                    _ => bad = true,
                }
            }
            bad |= themes.len() > crate::theme::MAX_CUSTOM_THEMES;
        } else {
            bad = true;
        }
    }
    if let Some(theme) = value::<ThemeSelection>(map, "theme", &mut bad) {
        let available = match &theme {
            ThemeSelection::BuiltIn { .. } => true,
            ThemeSelection::Custom { name } => settings
                .custom_themes
                .iter()
                .any(|custom| custom.name == *name),
        };
        if available {
            settings.theme = theme;
        } else {
            bad = true;
        }
    }
    if let Some(raw) = map.get("notifications") {
        let (notifications, recovered) = recover_notifications(raw);
        settings.notifications = notifications;
        bad |= recovered;
    }
    if let Some(raw) = map.get("result_cards") {
        let (cards, recovered) = recover_result_cards(raw);
        settings.result_cards = cards;
        bad |= recovered;
    }
    (settings, true | bad)
}

fn recover_preset(raw: &Value) -> (Option<Preset>, bool) {
    if let Ok(preset) = serde_json::from_value::<Preset>(raw.clone())
        && validate_name(&preset.name).is_ok()
        && preset.search.validate().is_ok()
    {
        return (Some(preset), false);
    }
    let Some(map) = object(raw) else {
        return (None, true);
    };
    let mut bad = unknown_keys(map, &["name", "search"]);
    let Some(name) =
        value::<String>(map, "name", &mut bad).filter(|name| validate_name(name).is_ok())
    else {
        return (None, true);
    };
    let (search, recovered) = map
        .get("search")
        .map_or((SearchOptions::default(), true), recover_search);
    bad |= recovered;
    (Some(Preset { name, search }), true | bad)
}

fn recover_settings_document(raw: &Value) -> (Settings, Vec<Value>, bool) {
    let Some(map) = object(raw) else {
        return (Settings::default(), Vec::new(), true);
    };
    let legacy_presets = map
        .get("presets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut current = raw.clone();
    if let Some(current) = current.as_object_mut() {
        current.insert("presets".into(), Value::Array(Vec::new()));
    }
    if !map.contains_key("presets")
        && let Ok(settings) = serde_json::from_value::<Settings>(current.clone())
        && settings.validate().is_ok()
    {
        return (settings, legacy_presets, false);
    }
    let mut bad = unknown_keys(
        map,
        &[
            "search",
            "presets",
            "interface",
            "workers",
            "frame_budget_ms",
            "keep_game_running",
        ],
    ) || map.contains_key("presets");
    let mut settings = Settings::default();
    if let Some(raw) = map.get("search") {
        let (search, recovered) = recover_search(raw);
        settings.search = search;
        bad |= recovered;
    }
    if let Some(raw) = map.get("interface") {
        let (interface, recovered) = recover_interface(raw);
        settings.interface = interface;
        bad |= recovered;
    }
    if let Some(workers) = value::<Option<usize>>(map, "workers", &mut bad) {
        if workers.is_none_or(|workers| (1..=MAX_WORKERS).contains(&workers)) {
            settings.workers = workers;
        } else {
            bad = true;
        }
    }
    if let Some(frame_budget_ms) = value::<u8>(map, "frame_budget_ms", &mut bad) {
        if validate_frame_budget(frame_budget_ms).is_ok() {
            settings.frame_budget_ms = frame_budget_ms;
        } else {
            bad = true;
        }
    }
    if let Some(keep_game_running) = value(map, "keep_game_running", &mut bad) {
        settings.keep_game_running = keep_game_running;
    }
    (settings, legacy_presets, true | bad)
}

fn settings_document(settings: &Settings) -> io::Result<Value> {
    let mut document = serde_json::to_value(settings).map_err(invalid)?;
    document
        .as_object_mut()
        .ok_or_else(|| invalid("settings must serialize as an object"))?
        .remove("presets");
    Ok(document)
}

fn preset_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let directory = directory.join(PRESETS_DIRECTORY);
    let mut files = match fs::read_dir(&directory) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    files.sort();
    Ok(files)
}

fn save_preset_files(directory: &Path, presets: &[Preset]) -> io::Result<()> {
    let preset_directory = directory.join(PRESETS_DIRECTORY);
    fs::create_dir_all(&preset_directory)?;
    let expected: BTreeSet<_> = presets
        .iter()
        .enumerate()
        .map(|(index, preset)| {
            let path = preset_directory.join(format!("{:02}.json", index + 1));
            write_json(&path, preset, MAX_SETTINGS_BYTES)?;
            Ok(path)
        })
        .collect::<io::Result<_>>()?;
    for path in preset_files(directory)? {
        if !expected.contains(&path) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn load_settings_with_recovery(directory: &Path) -> io::Result<LoadedSettings> {
    let settings_path = directory.join("settings.json");
    let (mut settings, legacy_presets, mut settings_recovered) =
        match read_bytes(&settings_path, MAX_SETTINGS_BYTES) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(raw) => recover_settings_document(&raw),
                Err(_) => (Settings::default(), Vec::new(), true),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                (Settings::default(), Vec::new(), false)
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                (Settings::default(), Vec::new(), true)
            }
            Err(error) => return Err(error),
        };

    let files = preset_files(directory)?;
    let mut presets = Vec::new();
    let mut names = BTreeSet::new();
    let mut recovered_preset_files = 0;
    for path in &files {
        let recovered = read_bytes(path, MAX_SETTINGS_BYTES)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .map_or((None, true), |raw| recover_preset(&raw));
        let (preset, bad) = recovered;
        recovered_preset_files += usize::from(bad);
        if let Some(preset) = preset {
            let normalized = preset.name.to_lowercase();
            if presets.len() < MAX_PRESETS && names.insert(normalized) {
                presets.push(preset);
            } else {
                recovered_preset_files += usize::from(!bad);
            }
        }
    }
    for raw in legacy_presets {
        let (preset, _) = recover_preset(&raw);
        if let Some(preset) = preset {
            let normalized = preset.name.to_lowercase();
            if presets.len() < MAX_PRESETS && names.insert(normalized) {
                presets.push(preset);
            }
        }
        settings_recovered = true;
    }
    settings.presets = presets;
    settings.validate().map_err(invalid)?;

    let recovery = (settings_recovered || recovered_preset_files > 0).then_some(SettingsRecovery {
        settings: settings_recovered,
        preset_files: recovered_preset_files,
    });
    if recovery.is_some() {
        write_json(
            &settings_path,
            &settings_document(&settings)?,
            MAX_SETTINGS_BYTES,
        )?;
        save_preset_files(directory, &settings.presets)?;
    }
    Ok(LoadedSettings { settings, recovery })
}

pub fn load_settings(directory: &Path) -> io::Result<Settings> {
    load_settings_with_recovery(directory).map(|loaded| loaded.settings)
}

pub fn save_settings(directory: &Path, settings: &Settings) -> io::Result<()> {
    settings.validate().map_err(invalid)?;
    let path = directory.join("settings.json");
    if let Ok(previous) = read_bytes(&path, MAX_SETTINGS_BYTES) {
        write_bytes(
            &directory.join("settings.backup.json"),
            &previous,
            MAX_SETTINGS_BYTES,
        )?;
    }
    write_json(&path, &settings_document(settings)?, MAX_SETTINGS_BYTES)?;
    save_preset_files(directory, &settings.presets)
}

pub fn user_directory() -> io::Result<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    Ok(base
        .ok_or_else(|| invalid("user application-data directory is unavailable"))?
        .join("SearchProbes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "search-probes-storage-{name}-{}-{stamp}",
                std::process::id()
            )))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn settings_with_preset() -> Settings {
        let mut settings = Settings::default();
        settings.presets.push(Preset {
            name: "Lush ocean worlds".into(),
            search: SearchOptions::default(),
        });
        settings
    }

    #[test]
    fn presets_are_distinct_files_outside_settings_json() {
        let directory = TestDirectory::new("separate-presets");
        let settings = settings_with_preset();

        save_settings(&directory.0, &settings).unwrap();

        let settings_json: Value =
            read_json(&directory.0.join("settings.json"), MAX_SETTINGS_BYTES).unwrap();
        assert!(settings_json.get("presets").is_none());
        let preset: Preset = read_json(
            &directory.0.join(PRESETS_DIRECTORY).join("01.json"),
            MAX_SETTINGS_BYTES,
        )
        .unwrap();
        assert_eq!(preset, settings.presets[0]);

        let loaded = load_settings_with_recovery(&directory.0).unwrap();
        assert_eq!(loaded.settings, settings);
        assert_eq!(loaded.recovery, None);
    }

    #[test]
    fn embedded_presets_migrate_and_compatible_settings_are_salvaged() {
        let directory = TestDirectory::new("embedded-presets");
        let mut raw = serde_json::to_value(settings_with_preset()).unwrap();
        let map = raw.as_object_mut().unwrap();
        map.insert("obsolete_field".into(), Value::Bool(true));
        fs::create_dir_all(&directory.0).unwrap();
        write_json(&directory.0.join("settings.json"), &raw, MAX_SETTINGS_BYTES).unwrap();

        let loaded = load_settings_with_recovery(&directory.0).unwrap();

        assert_eq!(loaded.settings.presets.len(), 1);
        assert_eq!(loaded.settings.presets[0].name, "Lush ocean worlds");
        assert_eq!(
            loaded.recovery,
            Some(SettingsRecovery {
                settings: true,
                preset_files: 0,
            })
        );
        let rewritten: Value =
            read_json(&directory.0.join("settings.json"), MAX_SETTINGS_BYTES).unwrap();
        assert!(rewritten.get("presets").is_none());
        assert!(rewritten.get("obsolete_field").is_none());
        assert!(
            directory
                .0
                .join(PRESETS_DIRECTORY)
                .join("01.json")
                .is_file()
        );
    }

    #[test]
    fn invalid_preset_fields_are_dropped_and_the_file_is_rewritten() {
        let directory = TestDirectory::new("preset-recovery");
        let settings = settings_with_preset();
        save_settings(&directory.0, &settings).unwrap();
        let preset_path = directory.0.join(PRESETS_DIRECTORY).join("01.json");
        let mut raw: Value = read_json(&preset_path, MAX_SETTINGS_BYTES).unwrap();
        let map = raw.as_object_mut().unwrap();
        map.insert("obsolete_field".into(), Value::Bool(true));
        map.get_mut("search")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("results".into(), Value::from(0));
        write_json(&preset_path, &raw, MAX_SETTINGS_BYTES).unwrap();

        let loaded = load_settings_with_recovery(&directory.0).unwrap();

        assert_eq!(loaded.settings.presets.len(), 1);
        assert_eq!(loaded.settings.presets[0].name, "Lush ocean worlds");
        assert_eq!(
            loaded.settings.presets[0].search.results,
            SearchOptions::default().results
        );
        assert_eq!(
            loaded.recovery,
            Some(SettingsRecovery {
                settings: false,
                preset_files: 1,
            })
        );
        let rewritten: Value = read_json(&preset_path, MAX_SETTINGS_BYTES).unwrap();
        assert!(rewritten.get("obsolete_field").is_none());
        let strict: Preset = serde_json::from_value(rewritten).unwrap();
        assert_eq!(strict, loaded.settings.presets[0]);
    }

    #[test]
    fn removed_count_fields_are_discarded_without_migration() {
        let directory = TestDirectory::new("discard-removed-counts");
        let mut raw = settings_document(&Settings::default()).unwrap();
        let criteria = raw
            .get_mut("search")
            .unwrap()
            .get_mut("criteria")
            .unwrap()
            .as_object_mut()
            .unwrap();
        criteria.remove("planet_counts");
        criteria.remove("star_counts");
        criteria.insert("minimum_planets".into(), Value::from(4));
        criteria.insert("minimum_stars".into(), Value::from(3));
        fs::create_dir_all(&directory.0).unwrap();
        write_json(&directory.0.join("settings.json"), &raw, MAX_SETTINGS_BYTES).unwrap();

        let loaded = load_settings_with_recovery(&directory.0).unwrap();

        assert_eq!(loaded.settings.search.criteria.planet_counts, 0);
        assert_eq!(loaded.settings.search.criteria.star_counts, 0);
        assert_eq!(
            loaded.recovery,
            Some(SettingsRecovery {
                settings: true,
                preset_files: 0,
            })
        );
        let rewritten: Value =
            read_json(&directory.0.join("settings.json"), MAX_SETTINGS_BYTES).unwrap();
        let criteria = &rewritten["search"]["criteria"];
        assert!(criteria.get("minimum_planets").is_none());
        assert!(criteria.get("minimum_stars").is_none());
        assert_eq!(criteria["planet_counts"], Value::from(0));
        assert_eq!(criteria["star_counts"], Value::from(0));
    }
}

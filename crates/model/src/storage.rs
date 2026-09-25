use crate::settings::{MAX_SETTINGS_BYTES, Settings};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

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

pub fn load_settings(directory: &Path) -> io::Result<Settings> {
    let path = directory.join("settings.json");
    let settings = match read_json::<Settings>(&path, MAX_SETTINGS_BYTES) {
        Ok(settings) => settings,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
        Err(error) => return Err(error),
    };
    settings.validate().map_err(invalid)?;
    Ok(settings)
}

pub fn save_settings(directory: &Path, settings: &Settings) -> io::Result<()> {
    settings.validate().map_err(invalid)?;
    let path = directory.join("settings.json");
    match read_bytes(&path, MAX_SETTINGS_BYTES) {
        Ok(previous) => {
            let valid = serde_json::from_slice::<Settings>(&previous)
                .is_ok_and(|parsed| parsed.validate().is_ok());
            if valid {
                write_bytes(
                    &directory.join("settings.backup.json"),
                    &previous,
                    MAX_SETTINGS_BYTES,
                )?;
            } else {
                preserve_rejected_settings(&path)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            preserve_rejected_settings(&path)?;
        }
        Err(error) => return Err(error),
    }
    write_json(&path, settings, MAX_SETTINGS_BYTES)
}

fn preserve_rejected_settings(path: &Path) -> io::Result<()> {
    for _ in 0..64 {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let rejected = path.with_file_name(format!(
            "settings.rejected-{}-{stamp}-{}.json",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&rejected)
        {
            Ok(file) => {
                drop(file);
                if let Err(error) = replace(path, &rejected) {
                    let _ = fs::remove_file(rejected);
                    return Err(error);
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot preserve the rejected settings file",
    ))
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
    struct Temporary(PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn temporary() -> Temporary {
        Temporary(std::env::temp_dir().join(format!(
            "search-probes-test-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        )))
    }
    #[test]
    fn atomic_settings_replacement_retains_previous_valid_file() {
        let directory = temporary();
        let original = load_settings(&directory.0).unwrap();
        save_settings(&directory.0, &original).unwrap();
        let mut updated = original.clone();
        updated.search.results = 100;
        save_settings(&directory.0, &updated).unwrap();
        assert_eq!(load_settings(&directory.0).unwrap(), updated);
        assert_eq!(
            read_json::<Settings>(
                &directory.0.join("settings.backup.json"),
                MAX_SETTINGS_BYTES
            )
            .unwrap(),
            original
        );
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }
    #[test]
    fn rejected_settings_are_preserved_when_replaced() {
        let directory = temporary();
        let path = directory.0.join("settings.json");
        write_bytes(&path, b"invalid", 100).unwrap();
        assert!(load_settings(&directory.0).is_err());
        assert!(read_bytes(&path, 2).is_err());
        assert!(write_bytes(&path, b"too long", 2).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid");
        save_settings(&directory.0, &Settings::default()).unwrap();
        assert_eq!(load_settings(&directory.0).unwrap(), Settings::default());
        let rejected = fs::read_dir(&directory.0)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.rejected-")
            })
            .unwrap();
        assert_eq!(fs::read(rejected.path()).unwrap(), b"invalid");
        write_bytes(
            &path,
            &vec![0; MAX_SETTINGS_BYTES + 1],
            MAX_SETTINGS_BYTES + 1,
        )
        .unwrap();
        save_settings(&directory.0, &Settings::default()).unwrap();
        assert_eq!(load_settings(&directory.0).unwrap(), Settings::default());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 3);
    }
}

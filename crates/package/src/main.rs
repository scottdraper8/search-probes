mod assets;
mod licensing;
use search_probes_model::supported;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const THIRD_PARTY_NOTICES: &str = "Binaries/SearchProbes/SearchProbesThirdPartyNotices.txt";

fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(
                format!("symlink is not a package file: {}", entry.path().display()).into(),
            );
        }
        if kind.is_dir() {
            result.extend(files(&entry.path())?);
        } else if kind.is_file() {
            result.push(entry.path());
        } else {
            return Err("unsupported file type".into());
        }
    }
    result.sort();
    Ok(result)
}

fn copy(source: &Path, destination: &Path) -> Result<()> {
    if !source.symlink_metadata()?.is_file() {
        return Err(format!("not a regular file: {}", source.display()).into());
    }
    fs::create_dir_all(destination.parent().ok_or("missing destination parent")?)?;
    fs::copy(source, destination)?;
    if hash(source)? != hash(destination)? {
        return Err("copy verification failed".into());
    }
    Ok(())
}

fn manifest(root: &Path) -> Result<BTreeMap<String, Value>> {
    files(root)?
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("non-UTF8 package path")?
                .replace('\\', "/");
            Ok((
                relative,
                json!({"bytes": path.metadata()?.len(), "sha256": hash(&path)?}),
            ))
        })
        .collect()
}

fn package_manifest(files: BTreeMap<String, Value>) -> Value {
    json!({
        "format": 1,
        "product": "Search Probes",
        "version": env!("CARGO_PKG_VERSION"),
        "game_version": supported::GAME_VERSION,
        "steam_build": supported::STEAM_BUILD,
        "nms_executable_sha256": supported::EXECUTABLE_SHA256,
        "files": files,
    })
}

fn archive(root: &Path, path: &Path, manifest: &Value) -> Result<()> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut archive = zip::ZipWriter::new(file);
    archive.set_raw_comment(serde_json::to_vec(manifest)?.into())?;
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for path in files(root)? {
        let name = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-UTF8 archive path")?
            .replace('\\', "/");
        archive.start_file(name, options)?;
        let mut source = fs::File::open(path)?;
        std::io::copy(&mut source, &mut archive)?;
    }
    archive.finish()?.sync_all()?;
    Ok(())
}

fn archive_manifest(archive: &mut zip::ZipArchive<fs::File>) -> Result<BTreeMap<String, Value>> {
    let mut result = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name()?.replace('\\', "/");
        if !file.is_file() {
            return Err(format!("non-file ZIP entry: {name}").into());
        }
        if name.starts_with('/') || name.split('/').any(|part| part.is_empty() || part == "..") {
            return Err(format!("unsafe ZIP entry: {name}").into());
        }
        let bytes = file.size();
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        let sha256: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if result
            .insert(name.clone(), json!({"bytes": bytes, "sha256": sha256}))
            .is_some()
        {
            return Err(format!("duplicate ZIP entry: {name}").into());
        }
    }
    Ok(result)
}

fn verify(path: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path)?)?;
    let saved: Value = serde_json::from_slice(archive.comment())?;
    let actual = archive_manifest(&mut archive)?;
    if saved["format"] != 1
        || saved["product"] != "Search Probes"
        || saved["game_version"] != supported::GAME_VERSION
        || saved["steam_build"] != supported::STEAM_BUILD
        || saved["nms_executable_sha256"] != supported::EXECUTABLE_SHA256
        || saved["files"] != serde_json::to_value(actual)?
    {
        return Err("package contents differ from the manifest".into());
    }
    Ok(())
}

fn consolidate_notices(source: &Path, destination: &Path) -> Result<()> {
    let mut output = fs::File::create(destination)?;
    writeln!(
        output,
        "Search Probes third-party notices\nGenerated from the validated release dependency and asset inventory."
    )?;
    for path in files(source)? {
        let relative = path
            .strip_prefix(source)?
            .to_str()
            .ok_or("non-UTF8 notice path")?
            .replace('\\', "/");
        writeln!(output, "\n\n===== {relative} =====\n")?;
        output.write_all(&fs::read(path)?)?;
    }
    output.sync_all()?;
    Ok(())
}

fn package(workspace: &Path, output: &Path) -> Result<()> {
    assets::validate(&workspace.join("assets"), false)?;
    let zip = archive_path(output);
    if output.exists() || zip.exists() {
        return Err("output already exists; choose a new package path".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("specify an output with a parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".search-probes-package-{}", std::process::id()));
    fs::create_dir(&temporary)?;
    // Cargo configuration and CARGO_TARGET_DIR can move the build outside the workspace.
    // Ask Cargo rather than silently packaging an older DLL left in workspace/target.
    let metadata = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(workspace)
        .args(["metadata", "--locked", "--no-deps", "--format-version=1"])
        .output()?;
    if !metadata.status.success() {
        return Err(String::from_utf8_lossy(&metadata.stderr)
            .into_owned()
            .into());
    }
    let metadata: Value = serde_json::from_slice(&metadata.stdout)?;
    let binaries = Path::new(
        metadata["target_directory"]
            .as_str()
            .ok_or("missing Cargo target directory")?,
    )
    .join("x86_64-pc-windows-gnu/release");
    copy(
        &binaries.join("winmm.dll"),
        &temporary.join("Binaries/winmm.dll"),
    )?;
    copy(
        &binaries.join("search_probes.dll"),
        &temporary.join("Binaries/SearchProbes/SearchProbes.dll"),
    )?;
    let assets = workspace.join("assets");
    for file in files(&assets)? {
        let relative = file.strip_prefix(&assets)?;
        let destination = temporary.join("GAMEDATA/MODS/SearchProbes").join(relative);
        if relative == Path::new(assets::MISSION_TABLE) {
            fs::create_dir_all(destination.parent().ok_or("missing destination parent")?)?;
            fs::write(
                destination,
                assets::expand_fauna_missions(&fs::read(&file)?)?,
            )?;
        } else {
            copy(&file, &destination)?;
        }
    }
    assets::validate(&temporary.join("GAMEDATA/MODS/SearchProbes"), true)?;
    let notices = temporary.join(".SearchProbesThirdPartyNotices");
    licensing::collect(workspace, &notices)?;
    consolidate_notices(&notices, &temporary.join(THIRD_PARTY_NOTICES))?;
    fs::remove_dir_all(notices)?;
    let manifest = package_manifest(manifest(&temporary)?);
    fs::rename(&temporary, output)?;
    archive(output, &zip, &manifest)?;
    verify(&zip)?;
    println!(
        "{}\n{}\nZIP SHA-256: {}",
        output.display(),
        zip.display(),
        hash(&zip)?
    );
    Ok(())
}

fn archive_path(output: &Path) -> PathBuf {
    let mut name = output.as_os_str().to_owned();
    name.push(".zip");
    PathBuf::from(name)
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: search-probes-package package OUTPUT-DIRECTORY | verify PACKAGE.zip".into(),
        );
    }
    let path = Path::new(&args[1]);
    match args[0].to_str() {
        Some("verify") => verify(path),
        Some("package") => package(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .ok_or("missing workspace")?,
            path,
        ),
        _ => Err("unknown command".into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

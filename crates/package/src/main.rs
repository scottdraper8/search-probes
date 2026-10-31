mod assets;
mod licensing;
use search_probes_model::supported;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

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
        .filter(|path| *path != root.join("manifest.json"))
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

fn archive(root: &Path, path: &Path) -> Result<()> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut archive = zip::ZipWriter::new(file);
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

fn verify(root: &Path) -> Result<()> {
    let saved: Value = serde_json::from_slice(&fs::read(root.join("manifest.json"))?)?;
    if saved["format"] != 1
        || saved["product"] != "Search Probes"
        || saved["files"] != serde_json::to_value(manifest(root)?)?
    {
        return Err("package contents differ from the manifest".into());
    }
    Ok(())
}

fn package(workspace: &Path, output: &Path) -> Result<()> {
    assets::validate(&workspace.join("assets"))?;
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
    for (from, to) in [("winmm.dll", "Binaries/winmm.dll")] {
        copy(&binaries.join(from), &temporary.join(to))?;
    }
    let assets = workspace.join("assets");
    for file in files(&assets)? {
        copy(
            &file,
            &temporary
                .join("GAMEDATA/MODS/SearchProbes")
                .join(file.strip_prefix(&assets)?),
        )?;
    }
    let installation = json!({
        "schema": 1,
        "product": "Search Probes",
        "version": env!("CARGO_PKG_VERSION"),
        "files": manifest(&temporary)?,
    });
    fs::create_dir_all(temporary.join("Binaries/SearchProbes"))?;
    fs::write(
        temporary.join("Binaries/SearchProbes/installation.json"),
        serde_json::to_vec_pretty(&installation)?,
    )?;
    copy(
        &workspace.join("README.md"),
        &temporary.join("README-SEARCH-PROBES.md"),
    )?;
    copy(
        &workspace.join("LICENSE.txt"),
        &temporary.join("LICENSE.txt"),
    )?;
    licensing::collect(workspace, &temporary.join("THIRD-PARTY"))?;
    let manifest = json!({"format":1,"product":"Search Probes","version":env!("CARGO_PKG_VERSION"),"game_version":supported::GAME_VERSION,"steam_build":supported::STEAM_BUILD,"nms_executable_sha256":supported::EXECUTABLE_SHA256,"files":manifest(&temporary)?});
    fs::write(
        temporary.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    verify(&temporary)?;
    fs::rename(&temporary, output)?;
    archive(output, &zip)?;
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
        return Err("usage: search-probes-package package|verify DIRECTORY".into());
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

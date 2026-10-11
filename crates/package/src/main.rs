mod assets;
use search_probes_model::supported;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
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

fn licenses(workspace: &Path, output: &Path) -> Result<()> {
    let supplemental = workspace.join("licenses");
    let sources: Vec<Value> =
        serde_json::from_slice(&fs::read(supplemental.join("sources.json"))?)?;
    let metadata = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(workspace)
        .args([
            "metadata",
            "--locked",
            "--format-version=1",
            "--filter-platform=x86_64-pc-windows-gnu",
        ])
        .output()?;
    if !metadata.status.success() {
        return Err(String::from_utf8_lossy(&metadata.stderr)
            .into_owned()
            .into());
    }
    let data: Value = serde_json::from_slice(&metadata.stdout)?;
    let packages = data["packages"]
        .as_array()
        .ok_or("missing package metadata")?;
    let nodes = data["resolve"]["nodes"]
        .as_array()
        .ok_or("missing dependency graph")?;
    let mut pending: Vec<_> = packages
        .iter()
        .filter(|p| matches!(p["name"].as_str(), Some("search-probes-bootstrap")))
        .map(|p| {
            p["id"]
                .as_str()
                .ok_or("invalid package id")
                .map(str::to_owned)
        })
        .collect::<std::result::Result<_, _>>()?;
    let mut selected = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !selected.insert(id.clone()) {
            continue;
        }
        let node = nodes
            .iter()
            .find(|n| n["id"] == id)
            .ok_or("unresolved dependency")?;
        for dependency in node["dependencies"]
            .as_array()
            .ok_or("missing dependencies")?
        {
            pending.push(dependency.as_str().ok_or("invalid dependency")?.to_owned());
        }
    }
    let mut index = Vec::new();
    for package in packages.iter().filter(|p| {
        p["source"].is_string() && p["id"].as_str().is_some_and(|id| selected.contains(id))
    }) {
        let name = package["name"].as_str().ok_or("missing package name")?;
        let version = package["version"]
            .as_str()
            .ok_or("missing package version")?;
        let source = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("missing package source")?,
        )
        .parent()
        .ok_or("missing source directory")?;
        let identifier = format!("{name}-{version}");
        let mut retained = Vec::new();
        for path in files(source)? {
            let filename = path
                .file_name()
                .ok_or("missing filename")?
                .to_string_lossy()
                .to_lowercase();
            if !["license", "licence", "copying", "copyright", "notice"]
                .iter()
                .any(|part| filename.contains(part))
                && !(name == "epaint_default_fonts" && filename.ends_with(".txt"))
            {
                continue;
            }
            let relative = path.strip_prefix(source)?;
            copy(&path, &output.join(&identifier).join(relative))?;
            retained.push(relative.to_string_lossy().replace('\\', "/"));
        }
        for record in sources
            .iter()
            .filter(|record| record["package"] == name && record["version"] == version)
        {
            let relative = Path::new(
                record["path"]
                    .as_str()
                    .ok_or("missing supplemental license path")?,
            );
            if relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err("invalid supplemental license path".into());
            }
            let source = supplemental.join(relative);
            if hash(&source)? != record["sha256"].as_str().ok_or("missing license digest")? {
                return Err("supplemental license hash differs".into());
            }
            copy(&source, &output.join(relative))?;
            retained.push(
                relative
                    .strip_prefix(&identifier)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        if retained.is_empty() {
            return Err(format!("no retained license files for {identifier}").into());
        }
        index.push(json!({"name": name, "version": version, "license": package["license"], "repository": package["repository"], "files": retained}));
    }
    fs::create_dir_all(output)?;
    fs::write(
        output.join("DEPENDENCIES.json"),
        serde_json::to_vec_pretty(&index)?,
    )?;
    copy(
        &supplemental.join("sources.json"),
        &output.join("SOURCES.json"),
    )?;
    let sysroot = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--print", "sysroot"])
        .output()?;
    if !sysroot.status.success() {
        return Err("cannot locate Rust library license notices".into());
    }
    let rust = PathBuf::from(String::from_utf8(sysroot.stdout)?.trim()).join("share/doc/rust");
    copy(
        &rust.join("COPYRIGHT-library.html"),
        &output.join("rust/COPYRIGHT-library.html"),
    )?;
    for path in files(&rust.join("licenses"))? {
        copy(
            &path,
            &output
                .join("rust/licenses")
                .join(path.strip_prefix(rust.join("licenses"))?),
        )?;
    }
    let compiler = supplemental.join("toolchain");
    for path in files(&compiler)? {
        copy(
            &path,
            &output.join("toolchain").join(path.strip_prefix(&compiler)?),
        )?;
    }
    Ok(())
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
    let binaries = workspace.join("target/x86_64-pc-windows-gnu/release");
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
        &workspace.join("VALIDATION.md"),
        &temporary.join("VALIDATION.md"),
    )?;
    copy(
        &workspace.join("LICENSE.txt"),
        &temporary.join("LICENSE.txt"),
    )?;
    licenses(workspace, &temporary.join("THIRD-PARTY"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_names_preserve_the_complete_version() {
        assert_eq!(
            archive_path(Path::new("dist/SearchProbes-0.1.1")),
            Path::new("dist/SearchProbes-0.1.1.zip")
        );
    }
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn directory() -> Directory {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "search-probes-package-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Directory(path)
    }
    #[test]
    fn archive_and_manifest_detect_changes_and_extra_files() {
        let dir = directory();
        let root = dir.0.join("package");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("payload"), b"bounded package").unwrap();
        let expected = manifest(&root).unwrap();
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&json!({"format":1,"product":"Search Probes","files":expected}))
                .unwrap(),
        )
        .unwrap();
        verify(&root).unwrap();
        let path = dir.0.join("package.zip");
        archive(&root, &path).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(zip.len(), 2);
        let mut text = String::new();
        zip.by_name("payload")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "bounded package");
        assert!(archive(&root, &path).is_err());
        fs::write(root.join("payload"), b"changed package").unwrap();
        assert!(verify(&root).is_err());
        fs::write(root.join("payload"), b"bounded package").unwrap();
        fs::create_dir(root.join("extra")).unwrap();
        fs::write(root.join("extra/manifest.json"), b"extra").unwrap();
        assert!(verify(&root).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn package_does_not_follow_symlinks() {
        let directory = directory();
        std::os::unix::fs::symlink("/etc/passwd", directory.0.join("link")).unwrap();
        assert!(files(&directory.0).is_err());
    }
}

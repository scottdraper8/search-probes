use crate::{Result, copy, files, hash};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use spdx::{Expression, LicenseId, LicenseItem};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: u32,
    allowed_licenses: Vec<String>,
    extra_notice_files: BTreeMap<String, Vec<String>>,
    toolchain: Vec<ToolchainComponent>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolchainComponent {
    name: String,
    version: String,
    files: Vec<PolicyFile>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SupplementalFile {
    package: String,
    version: String,
    path: String,
    url: String,
    sha256: String,
}

#[derive(Serialize)]
struct ToolchainIndex<'a> {
    schema: u32,
    components: &'a [ToolchainComponent],
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(Into::into)
}

fn relative_path(value: &str, subject: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("invalid {subject} path: {value}").into());
    }
    Ok(path.to_path_buf())
}

fn validate_digest(value: &str, subject: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("invalid SHA-256 digest for {subject}").into());
    }
    Ok(())
}

fn package_key(name: &str, version: &str) -> String {
    format!("{name}@{version}")
}

fn listed_paths(root: &Path) -> Result<BTreeSet<String>> {
    files(root)?
        .into_iter()
        .map(|path| {
            Ok(path
                .strip_prefix(root)?
                .to_str()
                .ok_or("non-UTF8 license inventory path")?
                .replace('\\', "/"))
        })
        .collect()
}

fn validate_configuration(
    workspace: &Path,
    policy: &Policy,
    supplemental: &[SupplementalFile],
) -> Result<BTreeSet<LicenseId>> {
    if policy.schema != 1 {
        return Err(format!("unsupported license policy schema: {}", policy.schema).into());
    }

    let mut allowed_names = BTreeSet::new();
    let mut allowed = BTreeSet::new();
    for name in &policy.allowed_licenses {
        if !allowed_names.insert(name) {
            return Err(format!("duplicate allowed license: {name}").into());
        }
        let id = spdx::license_id(name)
            .ok_or_else(|| format!("unknown SPDX license in policy: {name}"))?;
        if id.name != name || id.is_deprecated() {
            return Err(format!("non-canonical SPDX license in policy: {name}").into());
        }
        allowed.insert(id);
    }
    if allowed.is_empty() {
        return Err("license policy does not allow any licenses".into());
    }

    for (package, paths) in &policy.extra_notice_files {
        let (name, version) = package
            .split_once('@')
            .filter(|(name, version)| !name.is_empty() && !version.is_empty())
            .ok_or_else(|| format!("invalid package key in license policy: {package}"))?;
        if package_key(name, version) != *package || paths.is_empty() {
            return Err(format!("invalid extra notice entry: {package}").into());
        }
        let mut unique = BTreeSet::new();
        for path in paths {
            relative_path(path, "extra notice")?;
            if !unique.insert(path) {
                return Err(format!("duplicate extra notice for {package}: {path}").into());
            }
        }
    }

    let mut expected = BTreeSet::from(["sources.json".to_owned()]);
    let mut source_records = BTreeSet::new();
    for record in supplemental {
        if record.package.is_empty() || record.version.is_empty() {
            return Err("supplemental license record has an empty package or version".into());
        }
        let path = relative_path(&record.path, "supplemental license")?;
        let identifier = format!("{}-{}", record.package, record.version);
        let relative = path.strip_prefix(&identifier).map_err(|_| {
            format!(
                "supplemental license path is outside {identifier}: {}",
                record.path
            )
        })?;
        if relative.as_os_str().is_empty() {
            return Err(format!(
                "supplemental license path is outside {identifier}: {}",
                record.path
            )
            .into());
        }
        if !record.url.starts_with("https://")
            || record.url.len() == "https://".len()
            || record.url.chars().any(char::is_whitespace)
        {
            return Err(format!("invalid supplemental license URL: {}", record.url).into());
        }
        validate_digest(&record.sha256, &record.path)?;
        if !source_records.insert((
            record.package.as_str(),
            record.version.as_str(),
            record.path.as_str(),
        )) {
            return Err(format!("duplicate supplemental license record: {}", record.path).into());
        }
        if !expected.insert(record.path.clone()) {
            return Err(format!("duplicate managed license path: {}", record.path).into());
        }
    }

    let mut components = BTreeSet::new();
    for component in &policy.toolchain {
        relative_path(&component.name, "toolchain component")?;
        if component.version.trim().is_empty() || component.files.is_empty() {
            return Err(format!("incomplete toolchain policy for {}", component.name).into());
        }
        if !components.insert(&component.name) {
            return Err(format!("duplicate toolchain component: {}", component.name).into());
        }
        let prefix = Path::new("toolchain").join(&component.name);
        for file in &component.files {
            let path = relative_path(&file.path, "toolchain license")?;
            if !path.starts_with(&prefix) || path == prefix {
                return Err(format!(
                    "toolchain license path is outside {}: {}",
                    prefix.display(),
                    file.path
                )
                .into());
            }
            validate_digest(&file.sha256, &file.path)?;
            if !expected.insert(file.path.clone()) {
                return Err(format!("duplicate managed license path: {}", file.path).into());
            }
        }
    }

    let actual = listed_paths(&workspace.join("licenses"))?;
    let missing: Vec<_> = expected.difference(&actual).cloned().collect();
    let untracked: Vec<_> = actual.difference(&expected).cloned().collect();
    if !missing.is_empty() || !untracked.is_empty() {
        return Err(format!(
            "license inventory mismatch; missing: [{}]; untracked: [{}]",
            missing.join(", "),
            untracked.join(", ")
        )
        .into());
    }

    Ok(allowed)
}

fn validate_license(identifier: &str, raw: &str, allowed: &BTreeSet<LicenseId>) -> Result<()> {
    let canonical = Expression::canonicalize(raw)?;
    let expression = Expression::parse(canonical.as_deref().unwrap_or(raw))?;
    let compatible = expression.evaluate(|requirement| {
        if requirement.addition.is_some() {
            return false;
        }
        match &requirement.license {
            LicenseItem::Spdx { id, or_later } => !or_later && allowed.contains(id),
            LicenseItem::Other(_) => false,
        }
    });
    if !compatible {
        return Err(format!("dependency license is not allowed for {identifier}: {raw}").into());
    }
    Ok(())
}

fn metadata(workspace: &Path) -> Result<Value> {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(workspace)
        .args([
            "metadata",
            "--locked",
            "--format-version=1",
            "--filter-platform=x86_64-pc-windows-gnu",
        ])
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

fn selected_registry_packages(data: &Value) -> Result<Vec<&Value>> {
    let packages = data["packages"]
        .as_array()
        .ok_or("missing package metadata")?;
    let nodes = data["resolve"]["nodes"]
        .as_array()
        .ok_or("missing dependency graph")?;
    let roots: Vec<_> = packages
        .iter()
        .filter(|package| package["name"] == "search-probes-bootstrap")
        .map(|package| {
            package["id"]
                .as_str()
                .ok_or("invalid bootstrap package id")
                .map(str::to_owned)
        })
        .collect::<std::result::Result<_, _>>()?;
    if roots.len() != 1 {
        return Err(format!("expected one bootstrap package, found {}", roots.len()).into());
    }

    let mut pending = roots;
    let mut selected = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !selected.insert(id.clone()) {
            continue;
        }
        let node = nodes
            .iter()
            .find(|node| node["id"] == id)
            .ok_or_else(|| format!("unresolved dependency: {id}"))?;
        for dependency in node["dependencies"]
            .as_array()
            .ok_or("missing dependencies")?
        {
            pending.push(
                dependency
                    .as_str()
                    .ok_or("invalid dependency id")?
                    .to_owned(),
            );
        }
    }

    let mut result: Vec<_> = packages
        .iter()
        .filter(|package| {
            package["source"].is_string()
                && package["id"]
                    .as_str()
                    .is_some_and(|id| selected.contains(id))
        })
        .collect();
    result.sort_by_key(|package| {
        (
            package["name"].as_str().unwrap_or_default(),
            package["version"].as_str().unwrap_or_default(),
        )
    });
    Ok(result)
}

fn retain_notice(
    source_root: &Path,
    source: &Path,
    destination_root: &Path,
    retained: &mut BTreeSet<String>,
) -> Result<()> {
    let relative = source.strip_prefix(source_root)?;
    let name = relative
        .to_str()
        .ok_or("non-UTF8 dependency notice path")?
        .replace('\\', "/");
    let destination = destination_root.join(relative);
    if retained.insert(name) {
        copy(source, &destination)?;
    } else if hash(source)? != hash(&destination)? {
        return Err(format!("conflicting dependency notice: {}", relative.display()).into());
    }
    Ok(())
}

fn collect_dependency_notices(
    package: &Value,
    output: &Path,
    supplemental_root: &Path,
    supplemental: &[SupplementalFile],
    extra: Option<&Vec<String>>,
) -> Result<Vec<String>> {
    let name = package["name"].as_str().ok_or("missing package name")?;
    let version = package["version"]
        .as_str()
        .ok_or("missing package version")?;
    let identifier = format!("{name}-{version}");
    let source = Path::new(
        package["manifest_path"]
            .as_str()
            .ok_or("missing package source")?,
    )
    .parent()
    .ok_or("missing source directory")?;
    let destination = output.join(&identifier);
    let mut retained = BTreeSet::new();

    for path in files(source)? {
        let filename = path
            .file_name()
            .ok_or("missing dependency notice filename")?
            .to_string_lossy()
            .to_lowercase();
        if ["license", "licence", "copying", "copyright", "notice"]
            .iter()
            .any(|part| filename.contains(part))
        {
            retain_notice(source, &path, &destination, &mut retained)?;
        }
    }

    if let Some(license_file) = package["license_file"].as_str() {
        let path = Path::new(license_file);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            source.join(path)
        };
        retain_notice(source, &path, &destination, &mut retained)?;
    }

    if let Some(extra) = extra {
        for relative in extra {
            retain_notice(
                source,
                &source.join(relative_path(relative, "extra notice")?),
                &destination,
                &mut retained,
            )?;
        }
    }

    for record in supplemental
        .iter()
        .filter(|record| record.package == name && record.version == version)
    {
        let managed_path = relative_path(&record.path, "supplemental license")?;
        let source_path = supplemental_root.join(&managed_path);
        if hash(&source_path)? != record.sha256 {
            return Err(format!("supplemental license hash differs: {}", record.path).into());
        }
        let relative = managed_path.strip_prefix(&identifier)?;
        retain_notice(
            &supplemental_root.join(&identifier),
            &source_path,
            &destination,
            &mut retained,
        )?;
        if relative.as_os_str().is_empty() {
            return Err(format!("invalid supplemental license path: {}", record.path).into());
        }
    }

    if retained.is_empty() {
        return Err(format!("no retained license files for {identifier}").into());
    }
    Ok(retained.into_iter().collect())
}

fn collect_rust_notices(output: &Path) -> Result<()> {
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
    let licenses = rust.join("licenses");
    for path in files(&licenses)? {
        copy(
            &path,
            &output
                .join("rust/licenses")
                .join(path.strip_prefix(&licenses)?),
        )?;
    }
    Ok(())
}

fn collect_toolchain_notices(
    root: &Path,
    output: &Path,
    components: &[ToolchainComponent],
) -> Result<()> {
    for component in components {
        for file in &component.files {
            let path = relative_path(&file.path, "toolchain license")?;
            let source = root.join(&path);
            if hash(&source)? != file.sha256 {
                return Err(format!("toolchain license hash differs: {}", file.path).into());
            }
            copy(&source, &output.join(&path))?;
        }
    }
    fs::write(
        output.join("TOOLCHAIN.json"),
        serde_json::to_vec_pretty(&ToolchainIndex {
            schema: 1,
            components,
        })?,
    )?;
    Ok(())
}

pub fn collect(workspace: &Path, output: &Path) -> Result<()> {
    let policy: Policy = read_json(&workspace.join("license-policy.json"))?;
    let supplemental_root = workspace.join("licenses");
    let supplemental: Vec<SupplementalFile> = read_json(&supplemental_root.join("sources.json"))?;
    let allowed = validate_configuration(workspace, &policy, &supplemental)?;
    let data = metadata(workspace)?;
    let packages = selected_registry_packages(&data)?;
    let active: BTreeSet<_> = packages
        .iter()
        .map(|package| {
            Ok(package_key(
                package["name"].as_str().ok_or("missing package name")?,
                package["version"]
                    .as_str()
                    .ok_or("missing package version")?,
            ))
        })
        .collect::<Result<_>>()?;
    let mut identifiers = BTreeSet::new();
    for package in &packages {
        let name = package["name"].as_str().ok_or("missing package name")?;
        let version = package["version"]
            .as_str()
            .ok_or("missing package version")?;
        if !identifiers.insert(format!("{name}-{version}")) {
            return Err(format!("ambiguous dependency package: {name}@{version}").into());
        }
    }
    for record in &supplemental {
        let key = package_key(&record.package, &record.version);
        if !active.contains(&key) {
            return Err(format!("stale supplemental license record: {key}").into());
        }
    }
    for key in policy.extra_notice_files.keys() {
        if !active.contains(key) {
            return Err(format!("stale extra notice policy: {key}").into());
        }
    }

    fs::create_dir_all(output)?;
    let mut index = Vec::new();
    for package in packages {
        let name = package["name"].as_str().ok_or("missing package name")?;
        let version = package["version"]
            .as_str()
            .ok_or("missing package version")?;
        let identifier = package_key(name, version);
        let license = package["license"]
            .as_str()
            .filter(|license| !license.trim().is_empty())
            .ok_or_else(|| format!("dependency has no license expression: {identifier}"))?;
        validate_license(&identifier, license, &allowed)?;
        let retained = collect_dependency_notices(
            package,
            output,
            &supplemental_root,
            &supplemental,
            policy.extra_notice_files.get(&identifier),
        )?;
        index.push(json!({
            "name": name,
            "version": version,
            "license": license,
            "repository": package["repository"],
            "files": retained,
        }));
    }
    fs::write(
        output.join("DEPENDENCIES.json"),
        serde_json::to_vec_pretty(&index)?,
    )?;
    fs::write(
        output.join("SOURCES.json"),
        serde_json::to_vec_pretty(&supplemental)?,
    )?;
    collect_rust_notices(output)?;
    collect_toolchain_notices(&supplemental_root, output, policy.toolchain.as_slice())?;
    Ok(())
}

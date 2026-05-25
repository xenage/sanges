use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::Context;
use reqwest::blocking::Client;

use super::alpine::fetch_to_file;
use super::types::{BASE_APK_PACKAGES, BASE_URL};

#[derive(Clone, Debug)]
pub(super) struct Package {
    name: String,
    version: String,
    repo: String,
    depends: Vec<String>,
    provides: Vec<String>,
}

pub(super) fn load_indexes(
    index_dir: &Path,
) -> anyhow::Result<(BTreeMap<String, Package>, BTreeMap<String, String>)> {
    let mut packages = BTreeMap::new();
    let mut providers = BTreeMap::new();
    for repo in ["main", "community"] {
        load_index(repo, index_dir, &mut packages, &mut providers)?;
    }
    Ok((packages, providers))
}

pub(super) fn wanted_apk_packages(extra: &[String], include_npm: bool) -> Vec<String> {
    let mut wanted = BASE_APK_PACKAGES
        .iter()
        .map(|package| (*package).to_owned())
        .collect::<BTreeSet<_>>();
    if include_npm {
        wanted.insert("npm".to_owned());
    }
    wanted.extend(
        extra
            .iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
    );
    wanted.into_iter().collect()
}

pub(super) fn resolve_packages(
    packages: &BTreeMap<String, Package>,
    providers: &BTreeMap<String, String>,
    wanted: &[String],
) -> anyhow::Result<Vec<Package>> {
    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = wanted.to_vec();
    while let Some(token) = stack.pop() {
        let Some(name) = normalize_dep(providers, &token) else {
            continue;
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let package = packages
            .get(&name)
            .with_context(|| format!("missing package in APKINDEX: {name}"))?
            .clone();
        stack.extend(package.depends.iter().cloned());
        resolved.push(package);
    }
    resolved.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(resolved)
}

pub(super) fn download_packages(
    client: &Client,
    apk_dir: &Path,
    arch: &str,
    resolved: &[Package],
    force_refresh: bool,
) -> anyhow::Result<()> {
    fs::create_dir_all(apk_dir)?;
    write_package_manifest(apk_dir, resolved)?;
    for package in resolved {
        let file_name = package.file_name();
        let target = apk_dir.join(&file_name);
        let url = format!("{BASE_URL}/{}/{arch}/{file_name}", package.repo);
        fetch_to_file(client, &url, &target, force_refresh)?;
    }
    Ok(())
}

fn load_index(
    repo: &str,
    index_dir: &Path,
    packages: &mut BTreeMap<String, Package>,
    providers: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let index_path = index_dir.join(format!("APKINDEX-{repo}"));
    let content = fs::read_to_string(&index_path)
        .with_context(|| format!("reading {}", index_path.display()))?;
    for block in content.trim().split("\n\n") {
        load_package_block(repo, block, packages, providers)?;
    }
    Ok(())
}

fn load_package_block(
    repo: &str,
    block: &str,
    packages: &mut BTreeMap<String, Package>,
    providers: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let fields = package_fields(block);
    let Some(name) = first_field(&fields, "P") else {
        return Ok(());
    };
    let Some(version) = first_field(&fields, "V") else {
        return Ok(());
    };
    let package = Package {
        name: name.to_owned(),
        version: version.to_owned(),
        repo: repo.to_owned(),
        depends: join_fields(&fields, "D"),
        provides: join_fields(&fields, "p"),
    };
    providers
        .entry(package.name.clone())
        .or_insert_with(|| package.name.clone());
    for entry in &package.provides {
        providers
            .entry(strip_constraint(entry))
            .or_insert_with(|| package.name.clone());
    }
    packages.insert(package.name.clone(), package);
    Ok(())
}

fn package_fields(block: &str) -> BTreeMap<String, Vec<String>> {
    let mut fields = BTreeMap::<String, Vec<String>>::new();
    for line in block.lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields
                .entry(key.to_owned())
                .or_default()
                .push(value.to_owned());
        }
    }
    fields
}

fn write_package_manifest(apk_dir: &Path, resolved: &[Package]) -> anyhow::Result<()> {
    let manifest = resolved
        .iter()
        .map(|package| package.name.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(apk_dir.join("manifest.txt"), format!("{manifest}\n"))?;
    Ok(())
}

pub(super) fn normalize_dep(providers: &BTreeMap<String, String>, token: &str) -> Option<String> {
    let dep = strip_constraint(token);
    if dep.is_empty() {
        return None;
    }
    if dep.starts_with("so:") || dep.starts_with("cmd:") {
        return providers.get(&dep).cloned();
    }
    Some(providers.get(&dep).cloned().unwrap_or(dep))
}

pub(super) fn strip_constraint(token: &str) -> String {
    let mut value = token.trim();
    for marker in ["!", "?", "<", ">", "=", "~"] {
        if let Some((left, _)) = value.split_once(marker) {
            value = left;
        }
    }
    value.trim().to_owned()
}

fn first_field<'a>(fields: &'a BTreeMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    fields
        .get(key)
        .and_then(|values| values.first())
        .map(String::as_str)
}

fn join_fields(fields: &BTreeMap<String, Vec<String>>, key: &str) -> Vec<String> {
    fields
        .get(key)
        .into_iter()
        .flat_map(|values| values.iter())
        .flat_map(|value| value.split_whitespace())
        .map(ToOwned::to_owned)
        .collect()
}

impl Package {
    pub(super) fn file_name(&self) -> String {
        format!("{}-{}.apk", self.name, self.version)
    }
}

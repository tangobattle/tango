//! Dependency-aware local installation and removal through the storage adapter.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tango_script::{Error, Package, PackageRef, Profile, Result};

use super::{catalog, load, Catalog, Source};
use crate::storage::Storage;

fn archive_path(root: &Path, reference: &PackageRef) -> PathBuf {
    root.join(&reference.name)
        .join(format!("{}.tangopkg", reference.version))
}

/// Validate the complete proposed graphs before writing any archives. Selected
/// files may supply one another's dependencies; other dependencies come from
/// the installed snapshot and the host's bundled packages. Hosts must serialize
/// mutations of a package library.
pub async fn install_many(
    storage: &dyn Storage,
    sources: &[PathBuf],
    root: &Path,
    bundled: &[Package],
) -> Result<Vec<PackageRef>> {
    if sources.is_empty() || sources.len() > catalog::MAX_PACKAGES {
        return Err(Error::Invalid("select between 1 and 256 packages".into()));
    }
    let listing = storage.list(&[root.to_owned()]).await;
    let mut paths: BTreeSet<_> = listing
        .entries()
        .iter()
        .filter_map(|entry| catalog::candidate(root, &entry.path).map(|(path, _, _)| path))
        .collect();
    let installed = Catalog::scan(storage, root, &listing, bundled).await;
    let mut available: BTreeMap<_, _> = installed
        .entries()
        .iter()
        .filter(|entry| !entry.conflicted)
        .map(|entry| {
            let reference = entry.reference();
            ((reference.name, reference.version), entry.package.clone())
        })
        .collect();
    let mut selected = BTreeMap::new();
    let mut selected_bytes = 0;
    for source in sources {
        // Snapshot once: the exact validated contents are what get installed.
        let package = load(storage, source).await?;
        let manifest = package.manifest();
        let key = (manifest.name.clone(), manifest.version.clone());
        let reference = PackageRef {
            name: key.0.clone(),
            version: key.1.clone(),
        };
        if installed.entry(&reference).is_some_and(|entry| entry.conflicted)
            || available
                .get(&key)
                .is_some_and(|existing| existing.digest() != package.digest())
        {
            return Err(Error::Invalid(format!(
                "{} {} already has different contents; increment the package version",
                key.0, key.1
            )));
        }
        selected_bytes += package.byte_len();
        if selected_bytes > catalog::MAX_BYTES {
            return Err(Error::Invalid(
                "selected packages exceed the package library size limit".into(),
            ));
        }
        paths.insert(archive_path(root, &reference));
        selected.insert(key.clone(), package.clone());
        available.insert(key, package);
    }
    if paths.len() > catalog::MAX_PACKAGES
        || available.len() > catalog::MAX_PACKAGES
        || available.values().map(Package::byte_len).sum::<usize>() > catalog::MAX_BYTES
    {
        return Err(Error::Invalid("installation exceeds the package library limits".into()));
    }
    let available: Vec<_> = available.into_values().collect();
    let mut outputs = Vec::new();
    let mut references = Vec::new();
    for ((name, version), package) in selected {
        let reference = PackageRef { name, version };
        Profile::resolve(&available, &reference)?;
        let output = archive_path(root, &reference);
        match load(storage, &output).await {
            Ok(existing) if existing.digest() == package.digest() => {}
            Ok(_) => {
                return Err(Error::Invalid(format!(
                    "{} already has different contents",
                    output.display()
                )))
            }
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                outputs.push((output, package.to_archive()?));
            }
            Err(error) => return Err(error),
        }
        references.push(reference);
    }
    let mut written: Vec<PathBuf> = Vec::new();
    for (path, bytes) in outputs {
        if let Err(error) = crate::storage::write_atomic(storage, &path, &bytes) {
            let mut failures = Vec::new();
            for path in written.iter().rev() {
                if let Err(error) = storage.remove_file(path) {
                    failures.push(format!("{}: {error}", path.display()));
                }
            }
            return if failures.is_empty() {
                Err(error.into())
            } else {
                Err(Error::Invalid(format!(
                    "{error}; could not roll back: {}",
                    failures.join(", ")
                )))
            };
        }
        written.push(path);
    }
    Ok(references)
}

/// Removing an archive may leave an identical bundled/development copy. Only
/// removing the last source of a required exact version strands dependents.
pub fn removal_blockers(catalog: &Catalog, reference: &PackageRef) -> Vec<PackageRef> {
    if catalog
        .entry(reference)
        .is_some_and(|entry| entry.sources.iter().any(|source| !matches!(source, Source::Archive(_))))
    {
        return Vec::new();
    }
    catalog.dependents(reference)
}

/// Remove one installed archive, checking current dependencies again instead of
/// trusting the UI's older inventory. Bundled files and development trees are
/// never deleted. A caller-supplied name is used as a path only after lookup in
/// the validated catalog.
pub async fn uninstall(storage: &dyn Storage, root: &Path, bundled: &[Package], reference: &PackageRef) -> Result<()> {
    let listing = storage.list(&[root.to_owned()]).await;
    let catalog = Catalog::scan(storage, root, &listing, bundled).await;
    let entry = catalog
        .entry(reference)
        .ok_or_else(|| Error::Invalid("package is not installed".into()))?;
    let path = archive_path(root, &entry.reference());
    if !entry.sources.contains(&Source::Archive(path.clone())) {
        return Err(Error::Invalid("only installed archives can be removed".into()));
    }
    let blockers = removal_blockers(&catalog, reference);
    if !blockers.is_empty() {
        return Err(Error::Invalid(format!(
            "package is required by {}",
            blockers
                .iter()
                .map(|reference| format!("{} {}", reference.name, reference.version))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    storage.remove_file(&path)?;
    Ok(())
}

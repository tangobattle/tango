//! Immutable installed-package snapshots, shared by native and browser hosts.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use tango_script::{ExportKind, Package, PackageRef, Profile};

use crate::storage::{Listing, Storage};

/// An export selection pins both the package version and the named export.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRef {
    pub kind: ExportKind,
    pub package: PackageRef,
    pub name: String,
}

#[derive(Clone)]
pub struct Export {
    pub reference: ExportRef,
    pub profile: Profile,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Bundled,
    Directory(PathBuf),
    Archive(PathBuf),
}

/// Package inventory includes libraries and unresolved dependency graphs.
#[derive(Clone)]
pub struct Entry {
    pub package: Package,
    pub sources: BTreeSet<Source>,
    pub error: Option<String>,
    pub(crate) conflicted: bool,
}

impl Entry {
    pub fn reference(&self) -> PackageRef {
        let manifest = self.package.manifest();
        PackageRef {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
        }
    }
}

/// Invalid packages are reported independently; a broken extension does not
/// make unrelated games disappear. A conflicting immutable identity is excluded
/// entirely, including from dependency resolution.
#[derive(Clone, Default)]
pub struct Catalog {
    entries: Vec<Entry>,
    exports: Vec<Export>,
    issues: Vec<String>,
}

pub(super) const MAX_PACKAGES: usize = 256;
pub(super) const MAX_BYTES: usize = 256 * 1024 * 1024;

impl Catalog {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, reference: &PackageRef) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.reference() == *reference)
    }

    /// Exact-version users, including packages whose graph currently fails.
    pub fn dependents(&self, reference: &PackageRef) -> Vec<PackageRef> {
        self.entries
            .iter()
            .filter(|entry| {
                entry.reference() != *reference
                    && entry.package.manifest().dependencies.get(&reference.name) == Some(&reference.version)
            })
            .map(Entry::reference)
            .collect()
    }

    pub fn exports(&self) -> &[Export] {
        &self.exports
    }

    pub fn issues(&self) -> &[String] {
        &self.issues
    }

    pub fn export(&self, reference: &ExportRef) -> Option<&Export> {
        self.exports.iter().find(|export| &export.reference == reference)
    }

    /// Resolve exactly the advertised package version/export, then reproduce
    /// its transformation using this host's ROM and backend ABI. Neither the
    /// latest installed version nor a native game is a fallback for a mismatch.
    pub fn resolve_gamemode(
        &self,
        configuration: &tango_match::gamemode::Configuration,
        engine: tango_match::gamemode::identity::Runtime,
        rom: Vec<u8>,
    ) -> tango_script::Result<tango_script::PreparedGameMode> {
        configuration
            .validate()
            .map_err(|error| tango_script::Error::Invalid(error.to_string()))?;
        let expected = &configuration.identity;
        if expected.engine != engine {
            return Err(tango_script::Error::Invalid(
                "gamemode requires a different engine ABI".into(),
            ));
        }
        let reference = ExportRef {
            kind: ExportKind::GameMode,
            package: PackageRef {
                name: expected.package.name.clone(),
                version: expected
                    .package
                    .version
                    .parse()
                    .map_err(|_| tango_script::Error::Invalid("invalid gamemode package version".into()))?,
            },
            name: expected.export.clone(),
        };
        let export = self.export(&reference).ok_or_else(|| {
            tango_script::Error::Invalid(format!(
                "missing gamemode {} {} / {}",
                reference.package.name, reference.package.version, reference.name
            ))
        })?;
        let prepared = tango_script::PreparedGameMode::prepare(
            export.profile.clone(),
            engine,
            rom,
            configuration.disable_bgm,
            configuration.options(),
        )?;
        prepared.verify(expected)?;
        Ok(prepared)
    }

    /// Automatic selection tries the newest release of each named export;
    /// prereleases are considered only when no release provides that export.
    /// Explicit references can still resolve every installed version.
    pub fn latest_exports(&self, kind: ExportKind) -> Vec<&Export> {
        let mut latest: BTreeMap<_, &Export> = BTreeMap::new();
        for export in self.exports.iter().filter(|export| export.reference.kind == kind) {
            let key = (&export.reference.package.name, &export.reference.name);
            let version = &export.reference.package.version;
            let rank = (version.pre.is_empty(), version);
            if latest.get(&key).is_none_or(|previous| {
                let previous = &previous.reference.package.version;
                rank > (previous.pre.is_empty(), previous)
            }) {
                latest.insert(key, export);
            }
        }
        latest.into_values().collect()
    }

    /// Directory development packages are `<root>/<name>/package.toml`;
    /// installed archives are `<root>/<name>/<version>.tangopkg`. Both use the
    /// ordinary sandboxed package loader. All dependencies are resolved from
    /// this snapshot plus the host's bundled packages, without hidden I/O.
    pub async fn scan(storage: &dyn Storage, root: &Path, listing: &Listing, bundled: &[Package]) -> Self {
        let mut catalog = Self::default();
        let mut packages = BTreeMap::new();
        let mut conflicts = BTreeSet::new();
        let mut sources: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        let mut bytes = 0;
        let mut insert = |package: Package, source: Source, issues: &mut Vec<String>| {
            let manifest = package.manifest();
            let key = (manifest.name.clone(), manifest.version.clone());
            sources.entry(key.clone()).or_default().insert(source);
            if conflicts.contains(&key) {
                return;
            }
            if let Some(existing) = packages.get(&key) {
                let existing: &Package = existing;
                if existing.digest() != package.digest() {
                    issues.push(format!(
                        "{} {} has conflicting contents; use a new version",
                        key.0, key.1
                    ));
                    conflicts.insert(key);
                }
                return;
            }
            let size = package.byte_len();
            if packages.len() >= MAX_PACKAGES || size > MAX_BYTES - bytes {
                issues.push(format!("{} {} exceeds the package library limits", key.0, key.1));
                return;
            }
            bytes += size;
            packages.insert(key, package);
        };
        for package in bundled {
            insert(package.clone(), Source::Bundled, &mut catalog.issues);
        }

        let candidates: BTreeSet<_> = listing
            .entries()
            .iter()
            .filter_map(|entry| candidate(root, &entry.path))
            .collect();
        if candidates.len() > MAX_PACKAGES {
            catalog
                .issues
                .push(format!("package library contains more than {MAX_PACKAGES} sources"));
        }
        for (path, name, version) in candidates.into_iter().take(MAX_PACKAGES) {
            let loaded = super::load(storage, &path).await.and_then(|package| {
                let manifest = package.manifest();
                if manifest.name != name || version.as_ref().is_some_and(|version| &manifest.version != version) {
                    return Err(tango_script::Error::Invalid(
                        "package name/version does not match its installed path".into(),
                    ));
                }
                Ok(package)
            });
            match loaded {
                Ok(package) => insert(
                    package,
                    if version.is_some() {
                        Source::Archive(path.clone())
                    } else {
                        Source::Directory(path.clone())
                    },
                    &mut catalog.issues,
                ),
                Err(error) => catalog.issues.push(format!("{}: {error}", path.display())),
            }
        }
        catalog.entries = packages
            .into_iter()
            .map(|(key, package)| Entry {
                package,
                sources: sources.remove(&key).unwrap_or_default(),
                error: conflicts
                    .contains(&key)
                    .then(|| "conflicting contents under the same name and version".into()),
                conflicted: conflicts.contains(&key),
            })
            .collect();
        let packages: Vec<_> = catalog
            .entries
            .iter()
            .filter(|entry| !entry.conflicted)
            .map(|entry| entry.package.clone())
            .collect();
        for entry in &mut catalog.entries {
            if entry.conflicted {
                continue;
            }
            let package = &entry.package;
            let manifest = package.manifest();
            let reference = entry.reference();
            match Profile::resolve(&packages, &reference) {
                Ok(profile) => {
                    for (kind, export) in ExportKind::ALL
                        .into_iter()
                        .flat_map(|kind| manifest.exports(kind).iter().map(move |export| (kind, export)))
                    {
                        // Every export is from the same validated manifest.
                        match profile.clone().with_export(kind, &export.name) {
                            Ok(profile) => catalog.exports.push(Export {
                                reference: ExportRef {
                                    kind,
                                    package: reference.clone(),
                                    name: export.name.clone(),
                                },
                                profile,
                            }),
                            Err(error) => catalog.issues.push(format!(
                                "{} {} / {}: {error}",
                                reference.name, reference.version, export.name
                            )),
                        }
                    }
                }
                Err(error) => {
                    entry.error = Some(error.to_string());
                    catalog
                        .issues
                        .push(format!("{} {}: {error}", reference.name, reference.version));
                }
            }
        }
        catalog
    }
}

pub(super) fn candidate(root: &Path, path: &Path) -> Option<(PathBuf, String, Option<semver::Version>)> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let (Some(Component::Normal(name)), Some(Component::Normal(file)), None) =
        (components.next(), components.next(), components.next())
    else {
        return None;
    };
    let name = name.to_str()?.to_owned();
    if file == "package.toml" {
        return Some((root.join(&name), name, None));
    }
    let file = Path::new(file);
    if file.extension()? != "tangopkg" {
        return None;
    }
    Some((path.to_owned(), name, Some(file.file_stem()?.to_str()?.parse().ok()?)))
}

//! The repo index (feature `index`) — one small JSON file describing
//! every package a repo serves, so clients can browse and check netplay
//! compatibility without downloading anything.
//!
//! This is the whole reason patches can be fetched on demand. The index
//! carries what the UI lists and what [`crate::Tag`] resolution needs; a
//! package's bytes are only fetched once something actually requires
//! them. Clients poll this file and nothing else.
//!
//! Every field is per *version*, because that is where the truth lives —
//! a patch's title and authors can change between releases, and hoisting
//! them into a patch-level summary would just create a second place for
//! them to disagree. To describe a patch as a whole, use its newest
//! version ([`Index::latest`]).

use crate::layout::RomTarget;
use crate::manifest::Compatibility;
use crate::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The only index format version this crate reads or writes.
pub const FORMAT: u32 = 1;

pub const FILE_NAME: &str = "index.json";

/// Suffix for the README sidecars [`Index::build`] can extract, so the UI
/// can show a patch's README before downloading its package.
pub const README_SUFFIX: &str = ".README.md";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub format: u32,
    /// Patch name → version → entry.
    pub patches: BTreeMap<String, BTreeMap<semver::Version, Entry>>,
}

/// An empty index of the current format — what a client has before its
/// first successful fetch.
impl Default for Index {
    fn default() -> Self {
        Index {
            format: FORMAT,
            patches: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub netplay: Compatibility,
    /// The ROM builds this version patches.
    pub games: Vec<RomTarget>,
    /// Package location, relative to the index. Always
    /// `<name>/<name>-<version>.tangopatch`.
    pub path: String,
    pub size: u64,
    /// Hex SHA-256 of the package. Clients verify downloads against this,
    /// which is also what makes a mirror or a cached CDN copy safe to
    /// use.
    pub sha256: String,
    /// Extracted README, relative to the index, when the repo published
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
}

impl Entry {
    /// This version's netplay identity for a given ROM family.
    pub fn tag(&self, family: &str, name: &str, version: &semver::Version) -> crate::Tag {
        crate::Tag::patched(family, name, version, &self.netplay)
    }

    /// Is `raw` the package this entry describes? A client checks every
    /// download against this, which is also what makes serving packages
    /// from a mirror or a CDN cache safe.
    pub fn verify(&self, raw: &[u8]) -> Result<(), Error> {
        if raw.len() as u64 != self.size {
            return Err(Error::Invalid(format!(
                "expected {} bytes, got {}",
                self.size,
                raw.len()
            )));
        }
        let sha256 = crate::sha256_hex(raw);
        if sha256 != self.sha256 {
            return Err(Error::Invalid(format!(
                "hash mismatch: expected {}, got {sha256}",
                self.sha256
            )));
        }
        Ok(())
    }
}

impl Index {
    pub fn parse(raw: &str) -> Result<Self, Error> {
        let index: Index = serde_json::from_str(raw)?;
        if index.format != FORMAT {
            return Err(Error::UnsupportedFormat(index.format));
        }
        Ok(index)
    }

    pub fn to_json(&self) -> Result<String, Error> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn get(&self, name: &str, version: &semver::Version) -> Option<&Entry> {
        self.patches.get(name)?.get(version)
    }

    /// The newest version of `name`, which is what a patch list shows.
    pub fn latest(&self, name: &str) -> Option<(&semver::Version, &Entry)> {
        self.patches.get(name)?.iter().next_back()
    }

    /// Total number of patch *versions*.
    pub fn len(&self) -> usize {
        self.patches.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.patches.values().all(|v| v.is_empty())
    }
}

/// Building an index means opening every package and hashing it, so it
/// needs the reader and the hasher; a client that only consumes an index
/// needs neither.
#[cfg(all(feature = "package", feature = "bundle"))]
mod build {
    use super::*;
    use std::path::Path;

    impl Index {
        /// Build an index by scanning `root` for `.tangopatch` files.
        ///
        /// With `emit_readmes`, each package's README is also extracted
        /// beside it as `<stem>.README.md` and linked from the entry.
        pub fn build(root: &Path, emit_readmes: bool) -> Result<Index, Error> {
            let mut paths = Vec::new();
            collect_packages(root, &mut paths)?;
            paths.sort();

            let mut patches: BTreeMap<String, BTreeMap<semver::Version, Entry>> = BTreeMap::new();
            for path in paths {
                let raw = std::fs::read(&path).map_err(|e| Error::from(e).at(&path))?;
                let mut package = crate::Package::read(std::io::Cursor::new(&raw)).map_err(|e| e.at(&path))?;
                let manifest = package.manifest().clone();

                // A package's URL has to be derivable from its identity,
                // so the file has to be named after what's inside it.
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                if file_name != manifest.file_name() {
                    return Err(Error::Invalid(format!(
                        "{}: package for {} should be named {}",
                        path.display(),
                        manifest.stem(),
                        manifest.file_name()
                    )));
                }

                let readme = match package.readme()? {
                    Some(readme) if emit_readmes => {
                        let sidecar = path.with_file_name(format!("{}{README_SUFFIX}", manifest.stem()));
                        std::fs::write(&sidecar, readme).map_err(|e| Error::from(e).at(&sidecar))?;
                        Some(relative_url(root, &sidecar)?)
                    }
                    _ => None,
                };

                let entry = Entry {
                    title: manifest.title.clone(),
                    authors: manifest.authors.clone(),
                    license: manifest.license.clone(),
                    source: manifest.source.clone(),
                    netplay: manifest.netplay.clone(),
                    games: package.targets().collect(),
                    path: relative_url(root, &path)?,
                    size: raw.len() as u64,
                    sha256: crate::sha256_hex(&raw),
                    readme,
                };

                if patches
                    .entry(manifest.name.clone())
                    .or_default()
                    .insert(manifest.version.clone(), entry)
                    .is_some()
                {
                    return Err(Error::Invalid(format!(
                        "{} appears twice under {}",
                        manifest.stem(),
                        root.display()
                    )));
                }
            }

            Ok(Index {
                format: FORMAT,
                patches,
            })
        }
    }

    fn collect_packages(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), Error> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(Error::from(e).at(dir)),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                collect_packages(&path, out)?;
            } else if path.extension().is_some_and(|e| e == crate::EXTENSION) {
                out.push(path);
            }
        }
        Ok(())
    }

    /// `path` relative to `root`, with `/` separators for use in a URL.
    fn relative_url(root: &Path, path: &Path) -> Result<String, Error> {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::Invalid(format!("{} is not under {}", path.display(), root.display())))?;
        Ok(relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_index_format_is_rejected() {
        assert!(matches!(
            Index::parse(r#"{"format": 99, "patches": {}}"#),
            Err(Error::UnsupportedFormat(99))
        ));
    }

    #[test]
    fn an_empty_index_round_trips() {
        let index = Index {
            format: FORMAT,
            patches: BTreeMap::new(),
        };
        assert_eq!(Index::parse(&index.to_json().unwrap()).unwrap(), index);
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }
}

//! Installed packages, offered versions, and library scanning.

use super::{index_path, Error};
use crate::rom::GameRef;
use crate::scanner;
use crate::storage::{self, Listing, Storage};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tango_patch::{Compatibility, Package, Tag};

/// One installed patch version — everything read out of its package at
/// scan time. The BPS payloads stay in the file and are read on demand
/// (see [`super::apply_patch`]); a scan only touches metadata, the
/// README, and any save templates.
pub struct Version {
    /// The `.tangopatch` this came from.
    pub path: PathBuf,
    pub netplay: Compatibility,
    pub rom_overrides: BTreeMap<tango_patch::RomTarget, tango_patch::Overrides>,
    pub supported_games: HashSet<GameRef>,
    /// Per-game save templates the patch ships. Keyed by template name
    /// (empty string = the default template); values are owned Save
    /// trait objects ready to be serialized via `to_sram_dump`.
    pub save_templates: HashMap<GameRef, BTreeMap<String, tango_gamesupport::BoxedSave>>,
    pub readme: Option<String>,
}

impl Version {
    /// The override object for this exact ROM, or an empty object when the
    /// package does not override it.
    pub fn rom_overrides_for(&self, game: GameRef) -> tango_patch::Overrides {
        let target = tango_patch::RomTarget::new(*game.rom_code, game.revision);
        self.rom_overrides.get(&target).cloned().unwrap_or_default()
    }
}

/// An installed patch: its versions plus the metadata of the newest one.
/// Metadata is per-version in the package format (a patch can be
/// retitled or change hands between releases), and the newest version is
/// what a list row should describe.
pub struct Patch {
    pub title: String,
    /// Raw `Name <addr@example.com>` strings, exactly as the manifest
    /// has them — the same form the index carries, so both sources
    /// render identically once passed through [`display_authors`].
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub versions: BTreeMap<semver::Version, Arc<Version>>,
}

pub type PatchMap = BTreeMap<String, Arc<Patch>>;
pub type Scanner = scanner::Scanner<Catalog>;

/// What patches exist, from both directions: what's installed here and
/// what the repo offers. The two overlap freely — a patch can be
/// installed and indexed, installed only (sideloaded), or indexed only
/// (not downloaded yet).
#[derive(Default)]
pub struct Catalog {
    pub installed: PatchMap,
    /// Last index we fetched. Empty before the first successful fetch,
    /// and kept across restarts so the app can browse offline.
    pub index: tango_patch::Index,
}

/// One version as the UI sees it, whether or not it's on disk.
pub struct VersionInfo<'a> {
    pub installed: Option<&'a Arc<Version>>,
    pub indexed: Option<&'a tango_patch::index::Entry>,
}

impl VersionInfo<'_> {
    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    pub fn netplay(&self) -> Option<&Compatibility> {
        self.installed.map(|v| &v.netplay).or(self.indexed.map(|e| &e.netplay))
    }

    /// Package size in bytes, when the repo told us.
    pub fn size(&self) -> Option<u64> {
        self.indexed.map(|e| e.size)
    }
}

impl Catalog {
    /// Every patch name, installed or merely offered.
    pub fn names(&self) -> BTreeSet<&str> {
        self.installed
            .keys()
            .map(|s| s.as_str())
            .chain(self.index.patches.keys().map(|s| s.as_str()))
            .collect()
    }

    pub fn is_installed(&self, name: &str, version: &semver::Version) -> bool {
        self.version(name, version).is_some()
    }

    pub fn version(&self, name: &str, version: &semver::Version) -> Option<&Arc<Version>> {
        self.installed.get(name)?.versions.get(version)
    }

    /// The index entry for a version, if the repo offers it.
    pub fn entry(&self, name: &str, version: &semver::Version) -> Option<&tango_patch::index::Entry> {
        self.index.get(name, version)
    }

    /// Display metadata for a patch: the installed newest version's, or
    /// the index's if none is installed.
    pub fn title(&self, name: &str) -> Option<&str> {
        self.installed
            .get(name)
            .map(|p| p.title.as_str())
            .or_else(|| self.index.latest(name).map(|(_, e)| e.title.as_str()))
    }

    /// Every known version of `name`, oldest first.
    pub fn versions(&self, name: &str) -> BTreeMap<semver::Version, VersionInfo<'_>> {
        let mut out: BTreeMap<semver::Version, VersionInfo<'_>> = BTreeMap::new();
        if let Some(patch) = self.installed.get(name) {
            for (v, version) in &patch.versions {
                out.insert(
                    v.clone(),
                    VersionInfo {
                        installed: Some(version),
                        indexed: None,
                    },
                );
            }
        }
        if let Some(entries) = self.index.patches.get(name) {
            for (v, entry) in entries {
                out.entry(v.clone())
                    .or_insert(VersionInfo {
                        installed: None,
                        indexed: None,
                    })
                    .indexed = Some(entry);
            }
        }
        out
    }

    /// A version's netplay declaration, preferring the installed package
    /// (it's the thing that would actually run) over the index.
    pub fn compatibility(&self, name: &str, version: &semver::Version) -> Option<&Compatibility> {
        self.version(name, version)
            .map(|v| &v.netplay)
            .or_else(|| self.entry(name, version).map(|e| &e.netplay))
    }

    /// Resolve the netplay identity of a `(game, patch)` pair. `None`
    /// when the patch is one we've never heard of, installed or indexed
    /// — the peer may be running something sideloaded, which we can't
    /// vouch for either way.
    ///
    /// Resolving from the index matters: it's what lets a peer's patch
    /// be judged compatible *before* it's downloaded.
    pub fn tag(&self, game: GameRef, patch: Option<(&str, &semver::Version)>) -> Option<Tag> {
        let family = game.family_and_variant().0;
        match patch {
            None => Some(Tag::vanilla(family)),
            Some((name, version)) => Some(Tag::patched(family, name, version, self.compatibility(name, version)?)),
        }
    }

    /// Games a version supports. Falls back to the index's list so the
    /// Play tab can offer a patch that isn't downloaded yet.
    pub fn supported_games(&self, name: &str, version: &semver::Version) -> HashSet<GameRef> {
        if let Some(v) = self.version(name, version) {
            return v.supported_games.clone();
        }
        self.entry(name, version)
            .map(|e| e.games.iter().filter_map(|t| game_for(*t)).collect())
            .unwrap_or_default()
    }

    /// Newest version of `name` supporting `game` (any version when
    /// `game` is `None`), across installed *and* indexed versions.
    pub fn newest_version(&self, name: &str, game: Option<GameRef>) -> Option<semver::Version> {
        self.versions(name).into_keys().rfind(|v| match game {
            Some(g) => self.supported_games(name, v).contains(&g),
            None => true,
        })
    }
}

fn game_for(target: tango_patch::RomTarget) -> Option<GameRef> {
    crate::game::find_by_rom_info(&target.code, target.revision)
}

/// Read the installed packages and the cached index.
pub fn scan(storage: &dyn Storage, patches_path: &Path, listing: &Listing) -> Result<Catalog, Error> {
    let index = match storage::read_opt(storage, &index_path(patches_path))? {
        Some(raw) => match tango_patch::Index::parse(&String::from_utf8_lossy(&raw)) {
            Ok(index) => index,
            Err(e) => {
                log::warn!("cached patch index is unusable, ignoring it: {e}");
                tango_patch::Index::default()
            }
        },
        None => tango_patch::Index::default(),
    };

    // Newest version wins for the patch-level display metadata, so
    // collect per version first and fold afterwards.
    let mut versions: BTreeMap<String, BTreeMap<semver::Version, (Arc<Version>, tango_patch::Manifest)>> =
        BTreeMap::new();
    for entry in listing.entries() {
        let path = &entry.path;
        if path.extension().is_none_or(|e| e != tango_patch::EXTENSION) {
            continue;
        }
        match read_package(storage, path) {
            Ok((manifest, version)) => {
                versions
                    .entry(manifest.name.clone())
                    .or_default()
                    .insert(manifest.version.clone(), (Arc::new(version), manifest));
            }
            Err(e) => log::warn!("{}: {e}", path.display()),
        }
    }

    let installed = versions
        .into_iter()
        .filter_map(|(name, versions)| {
            // Newest version supplies the patch's display metadata.
            let (_, newest) = versions.values().next_back()?;
            let patch = Patch {
                title: newest.title.clone(),
                authors: newest.authors.clone(),
                license: newest.license.clone(),
                source: newest.source.clone(),
                versions: versions.into_iter().map(|(v, (version, _))| (v, version)).collect(),
            };
            Some((name, Arc::new(patch)))
        })
        .collect();

    Ok(Catalog { installed, index })
}

/// Read one package into a [`Version`] and its manifest.
fn read_package(storage: &dyn Storage, path: &Path) -> Result<(tango_patch::Manifest, Version), Error> {
    let mut package = Package::read(storage.open(path)?)?;
    let manifest = package.manifest().clone();

    // A package can be named for a game this build doesn't support (no
    // gamesupport feature, or a rom we don't know); that just means
    // fewer supported games, not a bad package.
    let mut supported_games = HashSet::new();
    let mut save_templates: HashMap<GameRef, BTreeMap<String, tango_gamesupport::BoxedSave>> = HashMap::new();
    for target in package.targets().collect::<Vec<_>>() {
        let Some(game) = game_for(target) else {
            continue;
        };
        supported_games.insert(game);
        for template in package.save_templates(target).map(|s| s.to_owned()).collect::<Vec<_>>() {
            let raw = package.save_template(target, &template)?;
            match game.parse_save(&raw) {
                Ok(save) => {
                    save_templates.entry(game).or_default().insert(template, save);
                }
                Err(e) => log::warn!(
                    "{}: {target} template {template:?} is not a valid save: {e}",
                    path.display()
                ),
            }
        }
    }

    let version = Version {
        path: path.to_path_buf(),
        netplay: manifest.netplay.clone(),
        rom_overrides: manifest.rom_overrides.clone(),
        supported_games,
        save_templates,
        readme: package.readme()?,
    };
    Ok((manifest, version))
}

/// `Name <addr@example.com>` → `Name`, falling back to the address when
/// there's no display name and to the raw string when it doesn't parse.
///
/// Applied at render time rather than at scan time: an installed patch's
/// authors come from its package and a non-installed one's from the
/// index, and reducing only the former made a patch appear to change
/// authors the moment it finished downloading.
pub fn display_authors(authors: &[String]) -> Vec<String> {
    authors
        .iter()
        .map(|s| match mailparse::addrparse(s) {
            Ok(addrs) => addrs
                .iter()
                .filter_map(|addr| match addr {
                    mailparse::MailAddr::Single(info) => {
                        Some(info.display_name.clone().unwrap_or_else(|| info.addr.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", "),
            Err(_) => s.clone(),
        })
        .collect()
}

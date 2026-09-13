//! Package capabilities discovered for each exact cartridge image.
use super::Choice;
use crate::library::{rom, Scanners};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tango_library::package::{Export, ExportRef};
use tango_match::gamemode::Configuration;
use tango_script::{ExportKind, PreparedGameMode};

type Imported = Result<Option<(rom::Id, Arc<PreparedGameMode>)>, String>;
type Resolved = Result<(rom::Id, Arc<PreparedGameMode>), String>;

#[derive(Default)]
pub struct Catalog {
    pub revision: (u64, u64),
    pub(super) choices: HashMap<rom::Id, Vec<Choice>>,
    editors: HashMap<rom::Id, Vec<Choice>>,
    packages: tango_library::package::Catalog,
    resolved: Mutex<Vec<(Configuration, Resolved)>>,
    imported: Mutex<Vec<(super::replay::LegacyKey, Imported)>>,
}

fn playable(export: &Export) -> bool {
    export.profile.gamemode_platform().ok().flatten().as_deref() == Some("gba")
}

impl Catalog {
    pub fn scan(scanners: &Scanners) -> Self {
        let roms = scanners.roms.read();
        let packages = scanners.packages.read();
        let exports: Vec<_> = packages
            .latest_exports(ExportKind::GameMode)
            .into_iter()
            .filter(|e| playable(e))
            .collect();
        let mut choices = HashMap::new();
        let mut editors = HashMap::new();
        for (id, image) in roms.images() {
            let rom = image.bytes.as_ref();
            let matches = super::editor::selection::choices(&packages, rom, None);
            if !matches.is_empty() {
                editors.insert(id, matches);
            }
            let mut modes: Vec<_> = exports
                .iter()
                .filter(|export| {
                    export
                        .profile
                        .supports_export(ExportKind::GameMode, rom)
                        .unwrap_or(false)
                })
                .map(|export| Choice {
                    reference: export.reference.clone(),
                    profile: export.profile.clone(),
                })
                .collect();
            modes.sort_by_key(|choice| {
                (
                    choice.reference.package.name.clone(),
                    choice
                        .profile
                        .packages()
                        .last()
                        .unwrap()
                        .exports(ExportKind::GameMode)
                        .iter()
                        .position(|export| export.name == choice.reference.name),
                )
            });
            if !modes.is_empty() {
                choices.insert(id, modes);
            }
        }
        Self {
            revision: (roms.revision(), packages.revision()),
            choices,
            editors,
            packages: packages.clone(),
            resolved: Mutex::default(),
            imported: Mutex::default(),
        }
    }

    /// One picker entry per cartridge, including editor-only packages.
    pub fn roms(&self) -> std::collections::BTreeSet<rom::Id> {
        self.choices.keys().chain(self.editors.keys()).copied().collect()
    }

    pub fn label(&self, id: rom::Id, locale: &str) -> Option<String> {
        let profile = self
            .editors
            .get(&id)
            .and_then(|choices| choices.first().map(|choice| &choice.profile))
            .or_else(|| self.choices.get(&id)?.first().map(|choice| &choice.profile))?;
        Some(
            profile
                .clone()
                .with_locale(locale)
                .unwrap_or_else(|_| profile.clone())
                .display_name()
                .to_owned(),
        )
    }

    pub(super) fn import_replay(&self, key: &super::replay::LegacyKey, roms: &rom::Catalog) -> Imported {
        let mut cache = self.imported.lock().unwrap();
        if let Some((_, result)) = cache.iter().find(|(previous, _)| previous == key) {
            return result.clone();
        }
        let result = (|| {
            let engine = tango_backend_mgba::gamemode::runtime();
            let legacy = tango_script::LegacyReplay {
                game: &key.game,
                match_type: key.match_type,
                match_subtype: key.match_subtype,
                engine: &engine,
            };
            let mut found = None;
            for (id, image) in roms.images() {
                // Detection was already performed by this catalog's scan.
                // Import callbacks only inspect its matching gamemodes.
                for export in self.choices.get(&id).into_iter().flatten() {
                    let rom = image.bytes.as_ref();
                    let imported = match export.profile.import_replay(&legacy, rom) {
                        Ok(Some(imported)) => imported,
                        Ok(None) => continue,
                        Err(error) => {
                            log::warn!(
                                "legacy replay import {}/{}: {error}",
                                export.reference.package.name,
                                export.reference.name
                            );
                            continue;
                        }
                    };
                    let prepared = PreparedGameMode::prepare(
                        export.profile.clone(),
                        engine.clone(),
                        rom.clone(),
                        imported.disable_bgm,
                        imported.options,
                    )
                    .map_err(|error| error.to_string())?;
                    if found.is_some() {
                        return Err("multiple package gamemodes or ROMs accept this legacy replay".into());
                    }
                    found = Some((id, Arc::new(prepared)));
                }
            }
            Ok(found)
        })();
        if cache.len() == 2 {
            drop(cache.remove(0));
        }
        cache.push((key.clone(), result.clone()));
        result
    }

    pub fn resolve(&self, configuration: &Configuration, roms: &rom::Catalog) -> Resolved {
        let mut cache = self.resolved.lock().unwrap();
        if let Some((_, result)) = cache.iter().find(|(key, _)| key == configuration) {
            return result.clone();
        }
        let result = (|| {
            configuration.validate().map_err(|e| e.to_string())?;
            let reference = ExportRef {
                kind: ExportKind::GameMode,
                package: tango_script::PackageRef {
                    name: configuration.identity.package.name.clone(),
                    version: configuration
                        .identity
                        .package
                        .version
                        .parse()
                        .map_err(|e: semver::Error| e.to_string())?,
                },
                name: configuration.identity.export.clone(),
            };
            let export = self
                .packages
                .export(&reference)
                .ok_or("selected gamemode package is not installed")?;
            if !playable(export) {
                return Err("gamemode platform is not supported".into());
            }
            for (id, image) in roms.images() {
                let rom = image.bytes.as_ref();
                if !export
                    .profile
                    .supports_export(ExportKind::GameMode, rom)
                    .unwrap_or(false)
                {
                    continue;
                }
                if let Ok(prepared) =
                    self.packages
                        .resolve_gamemode(configuration, tango_backend_mgba::gamemode::runtime(), rom.clone())
                {
                    return Ok((id, Arc::new(prepared)));
                }
            }
            Err("cannot reproduce the selected gamemode from installed packages and ROMs".into())
        })();
        // These entries retain effective ROMs. Keep only the two active seats.
        if cache.len() == 2 {
            drop(cache.remove(0));
        }
        cache.push((configuration.clone(), result.clone()));
        result
    }
}

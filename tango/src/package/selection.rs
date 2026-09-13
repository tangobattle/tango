//! Disk save selection for packages; all format knowledge stays in the editor.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::library::{rom, Scanners};
use crate::selection::LoadedSave;
use tango_script::Profile;

#[derive(Default)]
pub struct Selection {
    inputs: Option<(rom::Id, Option<[u8; 32]>, Option<String>)>,
    profile: Option<Profile>,
    source_error: Option<String>,
    save_revision: Option<u64>,
    saved_revision: Option<u64>,
    saves: BTreeMap<PathBuf, Result<(), String>>,
    loaded: Option<(PathBuf, rom::Id)>,
    raw_sram: Option<Arc<Vec<u8>>>,
    pub error: Option<String>,
}

impl Selection {
    /// Runs only on selection/scan changes, never from the view. A package that
    /// exports no editor can still commit its exact selected file as SRAM.
    pub fn refresh(
        &mut self,
        scanners: &Scanners,
        rom: &[u8],
        editor: &super::editor::selection::Selection,
        save_path: Option<&Path>,
        loaded: &mut Option<LoadedSave>,
    ) {
        let inputs = (
            rom::Id::of(rom),
            editor.profile.as_ref().map(Profile::digest),
            editor.error.clone(),
        );
        if self.inputs.as_ref() != Some(&inputs) {
            let result = match &editor.error {
                Some(error) => Err(error.clone()),
                None => editor
                    .profile
                    .clone()
                    .map(|profile| {
                        tango_script::Inputs::new([("rom".into(), rom.to_vec())].into())
                            .map(|inputs| profile.with_inputs(inputs))
                            .map_err(|error| error.to_string())
                    })
                    .transpose(),
            };
            let profile = result.as_ref().ok().cloned().flatten();
            let source_error = result.err().map(|error| error.to_string());
            let same_source = self.inputs.as_ref().is_some_and(|previous| previous.0 == inputs.0)
                && self.profile.as_ref().map(Profile::digest) == profile.as_ref().map(Profile::digest)
                && self.source_error == source_error;
            self.inputs = Some(inputs);
            if !same_source {
                self.save_revision = None;
                self.saved_revision = None;
                self.loaded = None;
                *loaded = None;
            }
            self.profile = profile;
            self.source_error = source_error;
        }
        let saves = scanners.saves.read();
        // An acknowledged write is newer than the scanner until its rescan
        // publishes. Intermediate selection refreshes must not reload old bytes.
        if self.saved_revision == Some(saves.revision())
            && loaded.is_some()
            && self
                .loaded
                .as_ref()
                .is_some_and(|(path, _)| Some(path.as_path()) == save_path)
        {
            return;
        }
        self.saved_revision = None;
        if self.save_revision != Some(saves.revision()) {
            self.save_revision = Some(saves.revision());
            self.saves = saves
                .files()
                .iter()
                .map(|(path, bytes)| {
                    let result = match (&self.source_error, &self.profile) {
                        (Some(error), _) => Err(error.clone()),
                        (_, Some(profile)) => profile.check_save(bytes).map_err(|error| error.to_string()),
                        (_, None) => Ok(()),
                    };
                    (path.clone(), result)
                })
                .collect();
        }
        self.error = self.source_error.clone();
        self.raw_sram = None;
        let Some(path) = save_path else {
            self.loaded = None;
            *loaded = None;
            return;
        };
        let Some(bytes) = saves.file(path) else {
            self.error = Some("selected save is no longer available".into());
            self.loaded = None;
            *loaded = None;
            return;
        };
        if let Some(Err(error)) = self.saves.get(path) {
            self.error = Some(error.clone());
            self.loaded = None;
            *loaded = None;
            return;
        }
        let key = (path.to_owned(), rom::Id::of(bytes));
        match &self.profile {
            Some(profile) => {
                if self.loaded.as_ref() != Some(&key) || loaded.is_none() {
                    *loaded = match super::editor::embedded::open_profile(None, profile.clone(), rom, bytes) {
                        Ok(mut data) => {
                            data.save_path = path.to_owned();
                            Some(data)
                        }
                        Err(error) => {
                            self.error = Some(error.to_string());
                            None
                        }
                    };
                }
            }
            None => {
                *loaded = None;
                self.raw_sram = Some(bytes.clone());
            }
        }
        self.loaded = Some(key);
    }

    pub fn saves(&self) -> impl Iterator<Item = &PathBuf> {
        self.saves
            .iter()
            .filter_map(|(path, result)| result.is_ok().then_some(path))
    }

    pub fn raw_sram(&self) -> Option<&[u8]> {
        self.raw_sram.as_deref().map(Vec::as_slice)
    }

    pub fn raw_sram_for(&self, path: &Path) -> Option<&[u8]> {
        self.loaded.as_ref().filter(|(loaded, _)| loaded == path)?;
        self.raw_sram()
    }

    /// Keep the current editor and scroll position when its own write is scanned.
    pub fn saved(&mut self, bytes: &[u8]) {
        self.saved_revision = self.save_revision;
        if let Some((_, hash)) = &mut self.loaded {
            *hash = rom::Id::of(bytes);
        }
    }

    pub fn invalidate(&mut self, error: String, loaded: &mut Option<LoadedSave>) {
        *self = Self {
            error: Some(error),
            ..Default::default()
        };
        *loaded = None;
    }
}

#[cfg(test)]
mod tests;

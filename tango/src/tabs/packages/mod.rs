//! Installed package inventory and local package management.
use std::path::PathBuf;

use tango_script::PackageRef;

#[cfg(test)]
mod tests;
mod view;

#[derive(Clone, Debug)]
pub enum Message {
    Selected(PackageRef),
    SearchChanged(String),
    Install,
    FilesPicked(Option<Vec<PathBuf>>),
    Uninstall(PackageRef),
    Finished(Result<Vec<PackageRef>, String>),
    Refresh,
    OpenFolder,
    OpenFolderFailed(String),
    Reveal(PathBuf),
    LegacyPatches,
}

pub enum Effect {
    ChooseFiles,
    Install(Vec<PathBuf>),
    Uninstall(PackageRef),
    Rescan,
    OpenFolder,
    Reveal(PathBuf),
    LegacyPatches,
}

#[derive(Default)]
pub struct State {
    selected: Option<PackageRef>,
    search: String,
    busy: bool,
    error: Option<String>,
    rescan_pending: bool,
}

impl State {
    pub fn request_rescan(&mut self) {
        self.rescan_pending = true;
    }

    pub fn take_rescan(&mut self) -> bool {
        std::mem::take(&mut self.rescan_pending)
    }

    pub fn update(&mut self, message: Message) -> Option<Effect> {
        match message {
            Message::Selected(reference) => self.selected = Some(reference),
            Message::SearchChanged(search) => self.search = search,
            Message::Install if !self.busy => {
                self.busy = true;
                self.error = None;
                return Some(Effect::ChooseFiles);
            }
            Message::FilesPicked(Some(paths)) if self.busy && !paths.is_empty() => {
                return Some(Effect::Install(paths));
            }
            Message::FilesPicked(_) => self.busy = false,
            Message::Uninstall(reference) if !self.busy => {
                self.busy = true;
                self.error = None;
                return Some(Effect::Uninstall(reference));
            }
            Message::Finished(result) => {
                self.busy = false;
                match result {
                    Ok(references) => {
                        if let Some(reference) = references.first() {
                            self.selected = Some(reference.clone());
                        }
                        self.error = None;
                    }
                    Err(error) => {
                        log::warn!("package management: {error}");
                        self.error = Some(error);
                    }
                }
                // Even a failed rollback may have changed files on disk.
                return Some(Effect::Rescan);
            }
            Message::Refresh if !self.busy => return Some(Effect::Rescan),
            Message::OpenFolder => return Some(Effect::OpenFolder),
            Message::OpenFolderFailed(error) => self.error = Some(error),
            Message::Reveal(path) => return Some(Effect::Reveal(path)),
            Message::LegacyPatches => return Some(Effect::LegacyPatches),
            Message::Install | Message::Uninstall(_) | Message::Refresh => {}
        }
        None
    }
}

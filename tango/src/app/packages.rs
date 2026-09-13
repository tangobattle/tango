//! Filesystem/dialog effects for package management run away from the UI loop.
use super::*;
use crate::library::package;

impl App {
    pub(super) fn update_packages(&mut self, message: tabs::packages::Message) -> iced::Task<Message> {
        use tabs::packages::{Effect, Message as M};
        if matches!(message, M::Install | M::Uninstall(_) | M::Refresh)
            && (!self.library_scanned || self.is_rescanning())
        {
            return iced::Task::none();
        }
        let Some(effect) = self.packages.update(message) else {
            return iced::Task::none();
        };
        let root = self.config.packages_path();
        match effect {
            Effect::ChooseFiles => {
                let label = t!(&self.config.language, "tab-packages");
                iced::Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .add_filter(label, &["tangopkg"])
                            .pick_files()
                            .await
                            .map(|files| files.into_iter().map(|file| file.path().to_owned()).collect())
                    },
                    M::FilesPicked,
                )
                .map(Message::Packages)
            }
            Effect::Rescan => {
                self.packages.request_rescan();
                self.rescan_packages_when_idle()
            }
            Effect::OpenFolder => iced::Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        crate::library::storage()
                            .create_dir_all(&root)
                            .map_err(|error| error.to_string())?;
                        open::that(root).map_err(|error| error.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())?
                },
                |result| match result {
                    Ok(()) => Message::NoOp,
                    Err(error) => Message::Packages(M::OpenFolderFailed(error)),
                },
            ),
            Effect::Reveal(path) => reveal_path(path),
            Effect::LegacyPatches => iced::Task::done(Message::TabSelected(Tab::Patches)),
            Effect::Install(_) | Effect::Uninstall(_) => iced::Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        futures::executor::block_on(async move {
                            let bundled = crate::package::editor::bundled::packages()?;
                            let storage = crate::library::storage();
                            match effect {
                                Effect::Install(sources) => package::install_many(storage, &sources, &root, bundled)
                                    .await
                                    .map_err(|error| error.to_string()),
                                Effect::Uninstall(reference) => package::uninstall(storage, &root, bundled, &reference)
                                    .await
                                    .map(|()| Vec::new())
                                    .map_err(|error| error.to_string()),
                                _ => unreachable!(),
                            }
                        })
                    })
                    .await
                    .map_err(|error| error.to_string())?
                },
                M::Finished,
            )
            .map(Message::Packages),
        }
    }
    /// A tab-triggered scan may already hold a pre-install snapshot. Wait for
    /// it to finish, then enumerate again so it cannot hide the mutation.
    pub(super) fn rescan_packages_when_idle(&mut self) -> iced::Task<Message> {
        if !self.is_rescanning() && self.packages.take_rescan() {
            self.rescan_off_thread(RescanFollowup::Refresh)
        } else {
            iced::Task::none()
        }
    }
}

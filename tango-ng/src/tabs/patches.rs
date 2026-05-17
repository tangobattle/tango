use crate::i18n::t;
use crate::icons;
use crate::{game, save_view, Scanners, STANDARD_PADDING, STANDARD_TEXT_SIZE, TEXT_BODY, TEXT_CAPTION};
use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, pick_list, row, scrollable, text, vertical_rule,
    Space,
};
use iced::{Alignment, Element, Fill, Length};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone)]
pub enum Message {
    Selected(String),
    VersionSelected(semver::Version),
    OpenFolder(std::path::PathBuf),
    ReadmeLinkClicked(iced::widget::markdown::Url),
    Rescan,
    Update,
    UpdateFinished(Result<(), String>),
}

#[derive(Default)]
pub struct PatchesState {
    pub selected: Option<String>,
    pub version: Option<semver::Version>,
    pub readme_items: Vec<iced::widget::markdown::Item>,
    pub readme_cache_key: Option<(String, semver::Version)>,
    pub updating: bool,
    pub last_update_error: Option<String>,
}

/// Side-effects bubble-up. See [`crate::tabs::replays::Effect`]
/// for the rationale: pure state mutations stay in the module;
/// anything that needs App-level collaborators (file system,
/// browser open, async patch update, scanner refresh) is
/// returned to be dispatched by the caller.
#[derive(Debug)]
pub enum Effect {
    /// `open::that(_)` — folder or http URL.
    OpenPath(String),
    /// User clicked Rescan; App re-scans + refreshes loaded.
    Rescan,
    /// User clicked Update; App spawns `patch::update(url, root)`
    /// and dispatches `Message::UpdateFinished` on completion.
    /// Carries the repo URL + on-disk root.
    StartUpdate {
        url: String,
        root: std::path::PathBuf,
    },
    /// Patch update finished cleanly — App should re-scan +
    /// refresh loaded.
    UpdateRescan,
}

impl PatchesState {
    /// Apply a tab message. See [`crate::tabs::replays::Effect`]
    /// for the side-effect surface convention.
    pub fn update(
        &mut self,
        msg: Message,
        scanners: &Scanners,
        config: &crate::config::Config,
    ) -> Option<Effect> {
        match msg {
            Message::Selected(p) => {
                let v = scanners
                    .patches
                    .read()
                    .get(&p)
                    .and_then(|patch| patch.versions.keys().max().cloned());
                self.selected = Some(p);
                self.version = v;
                self.refresh_readme(scanners);
                None
            }
            Message::VersionSelected(v) => {
                self.version = Some(v);
                self.refresh_readme(scanners);
                None
            }
            Message::OpenFolder(p) => Some(Effect::OpenPath(p.display().to_string())),
            Message::ReadmeLinkClicked(url) => Some(Effect::OpenPath(url.to_string())),
            Message::Rescan => Some(Effect::Rescan),
            Message::Update => {
                if self.updating {
                    return None;
                }
                self.updating = true;
                self.last_update_error = None;
                Some(Effect::StartUpdate {
                    url: config.patch_repo.clone(),
                    root: config.data_path.join("patches"),
                })
            }
            Message::UpdateFinished(res) => {
                self.updating = false;
                match res {
                    Ok(()) => {
                        self.last_update_error = None;
                        Some(Effect::UpdateRescan)
                    }
                    Err(e) => {
                        log::warn!("patch update failed: {e}");
                        self.last_update_error = Some(e);
                        None
                    }
                }
            }
        }
    }

    /// Rebuild the parsed-markdown cache for the currently selected
    /// patch+version. No-op if the cache already matches.
    pub fn refresh_readme(&mut self, scanners: &Scanners) {
        let key = match (&self.selected, &self.version) {
            (Some(n), Some(v)) => Some((n.clone(), v.clone())),
            _ => None,
        };
        if self.readme_cache_key == key {
            return;
        }
        self.readme_cache_key = key.clone();
        self.readme_items = key
            .as_ref()
            .and_then(|(n, _)| {
                scanners
                    .patches
                    .read()
                    .get(n)
                    .and_then(|p| p.readme.clone())
            })
            .map(|md| iced::widget::markdown::parse(&md).collect())
            .unwrap_or_default();
    }

    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
    ) -> Element<'a, Message> {
        let patches = scanners.patches.read();

        let update_msg = if self.updating { None } else { Some(Message::Update) };

        let mut top_row = row![
            text(format!(
                "{}: {}",
                t(lang, "patches-installed"),
                patches.len()
            ))
            .size(TEXT_CAPTION)
            .style(save_view::muted_text_style),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        if self.updating {
            top_row = top_row.push(
                text(t(lang, "patches-updating"))
                    .size(TEXT_CAPTION)
                    .style(save_view::muted_text_style),
            );
        } else if let Some(err) = &self.last_update_error {
            top_row = top_row.push(
                text(format!("{}: {}", t(lang, "patches-update-failed"), err))
                    .size(TEXT_CAPTION)
                    .style(text::danger),
            );
        }

        top_row = top_row
            .push(horizontal_space())
            .push(icons::icon_button_maybe(
                icons::UPDATE,
                t(lang, "patches-update"),
                update_msg,
                STANDARD_TEXT_SIZE,
                STANDARD_PADDING,
            ))
            .push(icons::icon_button(
                icons::RESCAN,
                t(lang, "rescan"),
                Message::Rescan,
                STANDARD_TEXT_SIZE,
                STANDARD_PADDING,
            ));

        let top = container(top_row.padding(8)).width(Fill);

        let mut list = column![].spacing(1).padding(8);
        for (name, patch) in patches.iter() {
            let selected = self.selected.as_deref() == Some(name.as_str());
            let style = if selected { button::primary } else { button::text };
            list = list.push(
                button(
                    column![
                        text(patch.title.clone()).size(TEXT_BODY),
                        text(name.clone()).size(TEXT_CAPTION).style(save_view::muted_text_style),
                    ]
                    .spacing(2),
                )
                .padding([6, 10])
                .width(Fill)
                .style(style)
                .on_press(Message::Selected(name.clone())),
            );
        }
        let left = container(scrollable(list).height(Fill))
            .width(Length::Fixed(280.0))
            .height(Fill);

        let right: Element<_> = if let Some(patch) = self.selected.as_ref().and_then(|n| patches.get(n)) {
            let mut versions: Vec<semver::Version> = patch.versions.keys().cloned().collect();
            versions.sort_by(|a, b| b.cmp(a));
            let selected_version = self
                .version
                .clone()
                .filter(|v| patch.versions.contains_key(v))
                .or_else(|| versions.first().cloned());

            let version_info = selected_version
                .as_ref()
                .and_then(|v| patch.versions.get(v))
                .cloned();

            let supported_games_str = version_info
                .as_ref()
                .map(|v| {
                    let mut names: Vec<String> = v
                        .supported_games
                        .iter()
                        .map(|g| game::display_name(lang, *g))
                        .collect();
                    names.sort();
                    if names.is_empty() {
                        "—".to_string()
                    } else {
                        names.join(", ")
                    }
                })
                .unwrap_or_else(|| "—".to_string());

            let netplay_compat = version_info
                .as_ref()
                .map(|v| v.netplay_compatibility.clone())
                .unwrap_or_default();

            let header = row![
                text(patch.title.clone()).size(20),
                horizontal_space(),
                pick_list(versions, selected_version, Message::VersionSelected)
                    .text_size(STANDARD_TEXT_SIZE)
                    .padding(STANDARD_PADDING),
                icons::icon_button(
                    icons::FOLDER,
                    t(lang, "patches-open-folder"),
                    Message::OpenFolder(patch.path.clone()),
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            let mut details = column![].spacing(4);
            if !patch.authors.is_empty() {
                details = details.push(
                    text(format!(
                        "{}: {}",
                        t(lang, "patches-details-authors"),
                        patch.authors.join(", ")
                    ))
                    .size(TEXT_CAPTION),
                );
            }
            if let Some(license) = &patch.license {
                details = details.push(
                    text(format!("{}: {}", t(lang, "patches-details-license"), license)).size(TEXT_CAPTION),
                );
            }
            if let Some(source) = &patch.source {
                details = details.push(
                    text(format!("{}: {}", t(lang, "patches-details-source"), source)).size(TEXT_CAPTION),
                );
            }
            details = details.push(
                text(format!(
                    "{}: {}",
                    t(lang, "patches-details-games"),
                    supported_games_str
                ))
                .size(TEXT_CAPTION),
            );
            if !netplay_compat.is_empty() {
                details = details.push(
                    text(format!(
                        "{}: {}",
                        t(lang, "patches-netplay-compatibility"),
                        netplay_compat
                    ))
                    .size(TEXT_CAPTION),
                );
            }

            // Markdown README, parsed and cached in self.readme_items.
            let readme_body: Element<'_, Message> = if self.readme_items.is_empty() {
                text(t(lang, "patches-readme-placeholder")).size(TEXT_CAPTION).into()
            } else {
                let theme = iced::Theme::Dark;
                iced::widget::markdown::view(
                    &self.readme_items,
                    iced::widget::markdown::Settings::default(),
                    iced::widget::markdown::Style::from_palette(theme.palette()),
                )
                .map(Message::ReadmeLinkClicked)
            };

            container(
                column![
                    header,
                    Space::with_height(8),
                    horizontal_rule(1),
                    Space::with_height(8),
                    details,
                    Space::with_height(12),
                    text(t(lang, "patches-readme")).size(TEXT_BODY).style(text::primary),
                    horizontal_rule(1),
                    scrollable(container(readme_body).padding(4)).height(Fill),
                ]
                .spacing(6)
                .padding(16),
            )
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            container(text(t(lang, "patches-select-prompt")).size(TEXT_BODY))
                .center(Fill)
                .into()
        };

        column![
            top,
            horizontal_rule(1),
            row![left, vertical_rule(1), right].height(Fill),
        ]
        .height(Fill)
        .into()
    }
}

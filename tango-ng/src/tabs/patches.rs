use crate::i18n::t;
use crate::{game, save_view, Scanners, STANDARD_PADDING, STANDARD_TEXT_SIZE};
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

impl PatchesState {
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

        let mut update_btn =
            button(text(t(lang, "patches-update")).size(STANDARD_TEXT_SIZE)).padding(STANDARD_PADDING);
        if !self.updating {
            update_btn = update_btn.on_press(Message::Update);
        }

        let mut top_row = row![
            text(format!(
                "{}: {}",
                t(lang, "patches-installed"),
                patches.len()
            ))
            .size(11)
            .style(save_view::muted_text_style),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        if self.updating {
            top_row = top_row.push(
                text(t(lang, "patches-updating"))
                    .size(11)
                    .style(save_view::muted_text_style),
            );
        } else if let Some(err) = &self.last_update_error {
            top_row = top_row.push(
                text(format!("{}: {}", t(lang, "patches-update-failed"), err))
                    .size(11)
                    .style(text::danger),
            );
        }

        top_row = top_row.push(horizontal_space()).push(update_btn).push(
            button(text(t(lang, "rescan")).size(STANDARD_TEXT_SIZE))
                .padding(STANDARD_PADDING)
                .on_press(Message::Rescan),
        );

        let top = container(top_row.padding(8)).width(Fill);

        let mut list = column![].spacing(2).padding(8);
        for (name, patch) in patches.iter() {
            let selected = self.selected.as_deref() == Some(name.as_str());
            let style = if selected { button::primary } else { button::text };
            list = list.push(
                button(
                    column![
                        text(patch.title.clone()).size(14),
                        text(name.clone()).size(10).style(save_view::muted_text_style),
                    ]
                    .spacing(2),
                )
                .padding(8)
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
                pick_list(versions, selected_version, Message::VersionSelected),
                {
                    let path = patch.path.clone();
                    button(text(t(lang, "patches-open-folder")).size(STANDARD_TEXT_SIZE))
                        .padding(STANDARD_PADDING)
                        .on_press(Message::OpenFolder(path))
                },
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
                    .size(12),
                );
            }
            if let Some(license) = &patch.license {
                details = details.push(
                    text(format!("{}: {}", t(lang, "patches-details-license"), license)).size(12),
                );
            }
            if let Some(source) = &patch.source {
                details = details.push(
                    text(format!("{}: {}", t(lang, "patches-details-source"), source)).size(12),
                );
            }
            details = details.push(
                text(format!(
                    "{}: {}",
                    t(lang, "patches-details-games"),
                    supported_games_str
                ))
                .size(12),
            );
            if !netplay_compat.is_empty() {
                details = details.push(
                    text(format!(
                        "{}: {}",
                        t(lang, "patches-netplay-compatibility"),
                        netplay_compat
                    ))
                    .size(12),
                );
            }

            // Markdown README, parsed and cached in self.readme_items.
            let readme_body: Element<'_, Message> = if self.readme_items.is_empty() {
                text(t(lang, "patches-readme-placeholder")).size(12).into()
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
                    text(t(lang, "patches-readme")).size(13).style(text::primary),
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
            container(text(t(lang, "patches-select-prompt")).size(13))
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

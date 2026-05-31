use crate::app::{Scanners, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::i18n::t;
use crate::widgets;
use crate::game;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{container, scrollable, text};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{button, column, pick_list, row, text_input};
use unic_langid::LanguageIdentifier;

/// Gold tone used for filled favorite stars. Hardcoded (not from
/// the theme palette) so it reads as "this is a favorite" on both
/// light and dark themes, regardless of the configured accent.
const FAVORITE_GOLD: iced::Color = iced::Color::from_rgb(1.0, 0.78, 0.0);

#[derive(Debug, Clone)]
pub enum Message {
    Selected(String),
    VersionSelected(semver::Version),
    OpenFolder(std::path::PathBuf),
    ReadmeLinkClicked(iced::widget::markdown::Uri),
    Rescan,
    Update,
    UpdateFinished(Result<(), String>),
    /// Star / unstar a patch. The patches list sorts favorites to
    /// the top, and the Play tab's patch dropdown shows a ★ glyph
    /// next to favorite names.
    ToggleFavorite(String),
    /// Patches list filter input changed.
    SearchChanged(String),
}

#[derive(Default)]
pub struct PatchesState {
    pub selected: Option<String>,
    pub version: Option<semver::Version>,
    pub readme_items: Vec<iced::widget::markdown::Item>,
    pub readme_cache_key: Option<(String, semver::Version)>,
    pub updating: bool,
    pub last_update_error: Option<String>,
    /// Case-insensitive substring filter applied to the sidebar
    /// list (matches against patch name and title).
    pub search: String,
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
    StartUpdate { url: String, root: std::path::PathBuf },
    /// Patch update finished cleanly — App should re-scan +
    /// refresh loaded.
    UpdateRescan,
    /// Toggle the named patch's favorite status in `Config`.
    ToggleFavorite(String),
}

impl PatchesState {
    /// Apply a tab message. See [`crate::tabs::replays::Effect`]
    /// for the side-effect surface convention.
    pub fn update(&mut self, msg: Message, scanners: &Scanners, config: &crate::config::Config) -> Option<Effect> {
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
            Message::ToggleFavorite(name) => Some(Effect::ToggleFavorite(name)),
            Message::SearchChanged(s) => {
                self.search = s;
                None
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
            .and_then(|(n, _)| scanners.patches.read().get(n).and_then(|p| p.readme.clone()))
            .map(|md| iced::widget::markdown::parse(&md).collect())
            .unwrap_or_default();
    }

    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        config: &'a crate::config::Config,
        rescanning: bool,
    ) -> Element<'a, Message> {
        let patches = scanners.patches.read();

        let update_msg = if self.updating { None } else { Some(Message::Update) };

        // Search input replaces the "Installed: N" label. The
        // count was informational only; a filter is more useful
        // once the patch list grows past a handful of entries.
        let search_input = text_input(&t!(lang, "patches-search-placeholder"), &self.search)
            .on_input(Message::SearchChanged)
            .padding(STANDARD_PADDING)
            .width(Length::Fixed(260.0))
            .style(widgets::chunky_text_input);
        let mut top_row = row![search_input].spacing(8).align_y(Alignment::Center);

        if self.updating {
            top_row = top_row.push(
                text(t!(lang, "patches-updating"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            );
        } else if let Some(err) = &self.last_update_error {
            top_row = top_row.push(
                text(t!(lang, "patches-update-failed", error = err.clone()))
                    .size(TEXT_CAPTION)
                    .style(text::danger),
            );
        }

        top_row = top_row
            .push(horizontal_space())
            .push(widgets::icon_button_maybe(
                Icon::CloudSync,
                t!(lang, "patches-update"),
                update_msg,
                STANDARD_PADDING,
            ))
            .push(widgets::icon_button_maybe(
                Icon::RefreshCw,
                t!(lang, "rescan"),
                (!rescanning).then_some(Message::Rescan),
                STANDARD_PADDING,
            ));

        let top = container(top_row)
            .padding(widgets::PANE_PADDING)
            .width(Fill)
            .style(widgets::pane);

        // Apply the search filter case-insensitively against both
        // the patch name (e.g. `bn6-foo`) and human title.
        let query = self.search.trim().to_lowercase();
        let mut ordered_patches: Vec<(&String, &std::sync::Arc<crate::patch::Patch>)> = patches
            .iter()
            .filter(|(n, p)| {
                query.is_empty() || n.to_lowercase().contains(&query) || p.title.to_lowercase().contains(&query)
            })
            .collect();
        // Favorites first, alphabetical within each group.
        ordered_patches.sort_by(|(a, _), (b, _)| {
            let fa = config.favorite_patches.contains(*a);
            let fb = config.favorite_patches.contains(*b);
            fb.cmp(&fa).then_with(|| a.cmp(b))
        });

        let mut list = column![].spacing(2).padding([8, 0]);
        for (idx, (name, patch)) in ordered_patches.iter().enumerate() {
            let selected = self.selected.as_deref() == Some(name.as_str());
            let is_fav = config.favorite_patches.contains(*name);
            // Title row: title text with a leading filled gold
            // star for favorites. Indicator only — the toggle
            // lives in the detail header.
            let title_row: Element<'_, Message> = if is_fav {
                row![
                    text("\u{2605}")
                        .size(TEXT_BODY)
                        .style(|_: &iced::Theme| iced::widget::text::Style {
                            color: Some(FAVORITE_GOLD),
                        }),
                    text(patch.title.clone()).size(TEXT_BODY),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            } else {
                text(patch.title.clone()).size(TEXT_BODY).into()
            };
            list = list.push(
                button(
                    column![
                        title_row,
                        text((*name).clone())
                            .size(TEXT_CAPTION)
                            .style(move |theme: &iced::Theme| if selected {
                                iced::widget::text::Style { color: None }
                            } else {
                                widgets::muted_text_style(theme)
                            }),
                    ]
                    .spacing(2),
                )
                .padding([6, 10])
                .width(Fill)
                .style(widgets::list_item(selected, idx))
                .on_press(Message::Selected((*name).clone())),
            );
        }
        let left = container(scrollable(list).height(Fill))
            .width(Length::Fixed(280.0))
            .height(Fill)
            .style(widgets::pane);

        let right: Element<_> = if let Some(patch) = self.selected.as_ref().and_then(|n| patches.get(n)) {
            let mut versions: Vec<semver::Version> = patch.versions.keys().cloned().collect();
            versions.sort_by(|a, b| b.cmp(a));
            let selected_version = self
                .version
                .clone()
                .filter(|v| patch.versions.contains_key(v))
                .or_else(|| versions.first().cloned());

            let version_info = selected_version.as_ref().and_then(|v| patch.versions.get(v)).cloned();

            let supported_games_str = version_info
                .as_ref()
                .map(|v| {
                    let mut names: Vec<String> =
                        v.supported_games.iter().map(|g| game::display_name(lang, *g)).collect();
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

            // Favorite toggle lives in the detail header, styled
            // as a flat icon-only affordance so it reads as a
            // toggle indicator rather than a CTA. State is carried
            // entirely by the glyph + color: filled gold "★" when
            // favorite, hollow muted "☆" when not.
            let is_fav = config.favorite_patches.contains(self.selected.as_ref().unwrap());
            let favorite_toggle = {
                let label = if is_fav {
                    t!(lang, "patches-unfavorite")
                } else {
                    t!(lang, "patches-favorite")
                };
                let glyph = if is_fav { "\u{2605}" } else { "\u{2606}" };
                let star = text(glyph)
                    .size(TEXT_TITLE)
                    .style(move |theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(if is_fav {
                            FAVORITE_GOLD
                        } else {
                            theme.extended_palette().background.weak.text
                        }),
                    });
                let btn = button(star)
                    .padding([4, 6])
                    .style(widgets::flat)
                    .on_press(Message::ToggleFavorite(self.selected.clone().unwrap()));
                iced::widget::tooltip(
                    btn,
                    container(text(label).size(TEXT_CAPTION))
                        .padding(6)
                        .style(widgets::tooltip_chrome),
                    iced::widget::tooltip::Position::Bottom,
                )
                .gap(4)
            };

            // Title in a Fill container so a long patch name takes
            // the leftover space and wraps naturally instead of
            // squashing the version picker / folder button on
            // the right. Default `Wrapping::Word` keeps it
            // readable across multiple lines if it has to.
            let header = row![
                favorite_toggle,
                container(text(patch.title.clone()).size(TEXT_TITLE))
                    .padding([4, 0])
                    .width(Fill),
                pick_list(versions, selected_version, Message::VersionSelected)
                    .padding(STANDARD_PADDING)
                    .style(widgets::chunky_pick_list),
                widgets::icon_button(
                    Icon::FolderOpen,
                    t!(lang, "patches-open-folder"),
                    Message::OpenFolder(patch.path.clone()),
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            // Top-align so the action buttons stay anchored when
            // a long title wraps to multiple lines (Center would
            // re-center them as the title grows).
            .align_y(Alignment::Start);

            // Single key:value row helper — muted label, plain value,
            // so the readable density matches the rest of the UI's
            // caption rows (e.g. save list metadata).
            let detail_row = |label: String, value: String| -> Element<'_, Message> {
                row![
                    text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                    text(value).size(TEXT_CAPTION),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            };

            let mut details = column![].spacing(3);
            if !patch.authors.is_empty() {
                details = details.push(detail_row(
                    t!(lang, "patches-details-authors"),
                    patch.authors.join(", "),
                ));
            }
            if let Some(license) = &patch.license {
                details = details.push(detail_row(t!(lang, "patches-details-license"), license.clone()));
            }
            if let Some(source) = &patch.source {
                details = details.push(detail_row(t!(lang, "patches-details-source"), source.clone()));
            }
            details = details.push(detail_row(t!(lang, "patches-details-games"), supported_games_str));
            if !netplay_compat.is_empty() {
                details = details.push(detail_row(t!(lang, "patches-netplay-compatibility"), netplay_compat));
            }

            // Markdown README, parsed and cached in self.readme_items.
            // Pull the live Theme from `crate::theme::theme_for(config)`
            // so link color tracks the active palette, and pin
            // the body text to the app's TEXT_BODY size (default
            // Settings::from would otherwise use 16 px and make
            // the README visually heavier than the rest of the
            // pane).
            let readme_body: Element<'_, Message> = if self.readme_items.is_empty() {
                text(t!(lang, "patches-readme-placeholder")).size(TEXT_CAPTION).into()
            } else {
                let theme = crate::theme::theme_for(config);
                let style = crate::theme::markdown_style(&theme);
                iced::widget::markdown::view(
                    &self.readme_items,
                    iced::widget::markdown::Settings::with_text_size(TEXT_BODY, style),
                )
                .map(Message::ReadmeLinkClicked)
            };

            let meta_pane = container(column![header, details,].spacing(6))
                .width(Fill)
                .padding(widgets::PANE_PADDING)
                .style(widgets::pane);
            // README is flush with the pane edges (no outer
            // padding) so the scrollbar hugs the pane; the markdown
            // body has its own PANE_PADDING inset so the prose
            // doesn't slam the pane wall. Pane height shrinks to
            // content but capped by the parent column's remaining
            // space — long readmes scroll inside, short ones don't
            // pad to the full page height.
            let readme_pane = container(scrollable(
                container(readme_body).padding(widgets::PANE_PADDING).width(Fill),
            ))
            .width(Fill)
            .style(widgets::pane);
            column![meta_pane, readme_pane]
                .spacing(widgets::PANE_GAP)
                .width(Fill)
                .height(Fill)
                .into()
        } else {
            container(text(t!(lang, "patches-select-prompt")).size(TEXT_BODY))
                .center(Fill)
                .style(widgets::pane)
                .into()
        };

        column![top, row![left, right].spacing(widgets::PANE_GAP).height(Fill),]
            .spacing(widgets::PANE_GAP)
            .padding(widgets::PANE_GAP)
            .height(Fill)
            .into()
    }
}

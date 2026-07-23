use crate::app::Scanners;
use crate::i18n::t;
use crate::library::game;
use crate::library::patch::{Catalog, Download, Downloads, VersionKey};
use crate::ui::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::ui::widgets;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, scrollable, text};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, row, text_input};
use unic_langid::LanguageIdentifier;

/// Gold tone used for filled favorite stars. Hardcoded (not from
/// the theme palette) so it reads as "this is a favorite" on both
/// light and dark themes, regardless of the configured accent.
const FAVORITE_GOLD: iced::Color = iced::Color::from_rgb(1.0, 0.78, 0.0);

#[derive(Debug, Clone)]
pub enum Message {
    Selected(String),
    VersionSelected(semver::Version),
    RevealPackage(std::path::PathBuf),
    ReadmeLinkClicked(iced::widget::markdown::Uri),
    /// Re-fetch the repo index (metadata only).
    Refresh,
    RefreshFinished(Result<(), String>),
    /// Download the selected version.
    Install(VersionKey),
    InstallProgress(VersionKey, u64, u64),
    InstallFinished(VersionKey, Result<(), String>),
    /// Delete an installed package. The index still lists it, so it can
    /// be reinstalled.
    Uninstall(VersionKey),
    /// A README fetched for a version that isn't installed.
    ReadmeFetched(VersionKey, Option<String>),
    /// Star / unstar a patch. The patches list sorts favorites to
    /// the top, and the Play tab's patch dropdown shows a ★ glyph
    /// next to favorite names.
    ToggleFavorite(String),
    /// Patches list filter input changed.
    SearchChanged(String),
    /// Which slice of the catalog the list shows.
    FilterChanged(Filter),
}

/// Which patches the list shows. The catalog holds both what's on disk
/// and what the repo offers, and the two questions people ask of it —
/// "what do I have?" and "what could I get?" — are different enough to
/// be worth separating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    /// On disk.
    Installed,
    /// Offered by the repo and not on disk.
    Available,
}

impl Filter {
    const ALL: [Filter; 3] = [Filter::All, Filter::Installed, Filter::Available];

    fn label(&self, lang: &LanguageIdentifier) -> String {
        match self {
            Filter::All => t!(lang, "patches-filter-all"),
            Filter::Installed => t!(lang, "patches-filter-installed"),
            Filter::Available => t!(lang, "patches-filter-available"),
        }
    }

    fn accepts(&self, installed: bool) -> bool {
        match self {
            Filter::All => true,
            Filter::Installed => installed,
            Filter::Available => !installed,
        }
    }
}

#[derive(Default)]
pub struct PatchesState {
    pub selected: Option<String>,
    pub version: Option<semver::Version>,
    pub readme_items: Vec<iced::widget::markdown::Item>,
    pub readme_cache_key: Option<VersionKey>,
    pub refreshing: bool,
    pub last_error: Option<String>,
    /// READMEs fetched for versions we haven't downloaded. `None` means
    /// "asked, and the repo doesn't publish one" — cached either way so
    /// selection doesn't re-request on every frame.
    remote_readmes: std::collections::HashMap<VersionKey, Option<String>>,
    /// Case-insensitive substring filter applied to the sidebar
    /// list (matches against patch name and title).
    pub search: String,
    pub filter: Filter,
    /// Entrance restarted when a different patch is selected —
    /// the detail panel slides in from the right.
    pub detail_enter: crate::ui::anim::Enter,
}

/// Side-effects bubble-up. See [`crate::tabs::replays::Effect`]
/// for the rationale: pure state mutations stay in the module;
/// anything that needs App-level collaborators (file system,
/// browser open, async fetch, scanner refresh) is returned to be
/// dispatched by the caller.
#[derive(Debug)]
pub enum Effect {
    /// `open::that(_)` — folder or http URL.
    OpenPath(String),
    /// Show a package in the file manager.
    RevealPath(std::path::PathBuf),
    /// Re-fetch the repo index. Cheap: metadata only, and conditional on
    /// the stored ETag.
    RefreshIndex,
    /// Download and install a patch version.
    Install(VersionKey),
    /// Delete an installed package.
    Uninstall(VersionKey),
    /// Fetch the README the repo published for a version we don't have.
    FetchReadme(VersionKey),
    /// Something changed on disk — App should re-scan + refresh loaded.
    Rescan,
    /// A download failed — anything queued behind it should give up.
    InstallFailed,
    /// Toggle the named patch's favorite status in `Config`.
    ToggleFavorite(String),
}

impl PatchesState {
    /// Apply a tab message. See [`crate::tabs::replays::Effect`]
    /// for the side-effect surface convention.
    pub fn update(&mut self, msg: Message, patches: &Catalog) -> Option<Effect> {
        match msg {
            Message::Selected(name) => {
                let newest = patches.versions(&name).keys().next_back().cloned();
                if self.selected.as_deref() != Some(name.as_str()) {
                    self.detail_enter.start(iced::time::Instant::now());
                }
                self.selected = Some(name);
                self.version = newest;
                self.refresh_readme(patches)
            }
            Message::VersionSelected(v) => {
                self.version = Some(v);
                self.refresh_readme(patches)
            }
            Message::RevealPackage(p) => Some(Effect::RevealPath(p)),
            Message::ReadmeLinkClicked(url) => Some(Effect::OpenPath(url.to_string())),
            Message::Refresh => {
                if self.refreshing {
                    return None;
                }
                self.refreshing = true;
                self.last_error = None;
                Some(Effect::RefreshIndex)
            }
            Message::RefreshFinished(res) => {
                self.refreshing = false;
                match res {
                    Ok(()) => Some(Effect::Rescan),
                    Err(e) => {
                        log::warn!("patch index refresh failed: {e}");
                        self.last_error = Some(e);
                        None
                    }
                }
            }
            Message::Install(key) => {
                self.last_error = None;
                Some(Effect::Install(key))
            }
            // App keeps the download map (see `patch::Downloads`); the
            // tab only reacts to the outcome.
            Message::InstallProgress(..) => None,
            Message::InstallFinished(key, res) => {
                match res {
                    Ok(()) => {
                        // The package now has its own README; drop the
                        // fetched one so the installed copy wins.
                        self.remote_readmes.remove(&key);
                        self.readme_cache_key = None;
                        Some(Effect::Rescan)
                    }
                    Err(e) => {
                        log::warn!("installing {} {}: {e}", key.0, key.1);
                        self.last_error = Some(e);
                        Some(Effect::InstallFailed)
                    }
                }
            }
            Message::Uninstall(key) => Some(Effect::Uninstall(key)),
            Message::ReadmeFetched(key, readme) => {
                self.remote_readmes.insert(key.clone(), readme);
                if self.readme_cache_key.as_ref() == Some(&key) {
                    // The cache key already matches this version, so
                    // `refresh_readme` would no-op — drop it first, then
                    // rebuild, or the fetched text sits in the map
                    // unrendered until the user reselects.
                    self.readme_cache_key = None;
                    return self.refresh_readme(patches);
                }
                None
            }
            Message::ToggleFavorite(name) => Some(Effect::ToggleFavorite(name)),
            Message::SearchChanged(s) => {
                self.search = s;
                None
            }
            Message::FilterChanged(f) => {
                self.filter = f;
                None
            }
        }
    }

    /// Rebuild the parsed-markdown cache for the currently selected
    /// patch+version. No-op if the cache already matches. Returns a
    /// fetch effect when the version isn't installed and we haven't
    /// asked the repo for its README yet.
    pub fn refresh_readme(&mut self, patches: &Catalog) -> Option<Effect> {
        let key = match (&self.selected, &self.version) {
            (Some(n), Some(v)) => (n.clone(), v.clone()),
            _ => {
                self.readme_cache_key = None;
                self.readme_items.clear();
                return None;
            }
        };
        if self.readme_cache_key.as_ref() == Some(&key) {
            return None;
        }

        let installed = patches.version(&key.0, &key.1).and_then(|v| v.readme.clone());
        let mut effect = None;
        let readme = match installed {
            Some(readme) => Some(readme),
            None => match self.remote_readmes.get(&key) {
                Some(cached) => cached.clone(),
                None => {
                    // Only worth asking if the repo says it published one.
                    if patches.entry(&key.0, &key.1).and_then(|e| e.readme.as_ref()).is_some() {
                        effect = Some(Effect::FetchReadme(key.clone()));
                    }
                    None
                }
            },
        };

        self.readme_cache_key = Some(key);
        self.readme_items = readme
            .map(|md| iced::widget::markdown::parse(&md).collect())
            .unwrap_or_default();
        effect
    }

    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        config: &'a crate::config::Config,
        downloads: &'a Downloads,
    ) -> Element<'a, Message> {
        let patches = scanners.patches.read();

        let top = self.top_strip(lang);
        let left = self.patch_list(&patches, config);
        let right: Element<'_, Message> = match self.selected.as_ref().filter(|n| patches.title(n).is_some()) {
            Some(name) => crate::ui::anim::slide_in_opt(
                self.patch_detail(lang, config, &patches, name, downloads),
                self.detail_enter.progress(iced::time::Instant::now()),
                iced::Vector::new(0.0, 28.0),
            ),
            None => widgets::pane_prompt(t!(lang, "patches-select-prompt")),
        };

        widgets::top_split_pane(top, left, right)
    }

    /// Top strip: the search filter, the in-flight / failed refresh
    /// status, and the refresh button.
    fn top_strip<'a>(&'a self, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
        let refresh_msg = if self.refreshing { None } else { Some(Message::Refresh) };

        let search_input = text_input(&t!(lang, "patches-search-placeholder"), &self.search)
            .on_input(Message::SearchChanged)
            .padding(STANDARD_PADDING)
            .width(Length::Fixed(260.0))
            .style(widgets::chunky_text_input);
        let filter = widgets::picker(
            Filter::ALL
                .iter()
                .map(|f| widgets::Choice::new(*f, f.label(lang)))
                .collect::<Vec<_>>(),
            Some(widgets::Choice::new(self.filter, self.filter.label(lang))),
            |c: widgets::Choice<Filter>| Message::FilterChanged(c.value),
        )
        .width(Length::Fixed(130.0));
        let mut top_row = row![search_input, filter].spacing(8).align_y(Alignment::Center);

        if self.refreshing {
            top_row = top_row.push(
                text(t!(lang, "patches-refreshing"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            );
        } else if let Some(err) = &self.last_error {
            top_row = top_row.push(
                text(t!(lang, "patches-refresh-failed", error = err.clone()))
                    .size(TEXT_CAPTION)
                    .style(widgets::danger_text_style),
            );
        }

        top_row = top_row.push(horizontal_space()).push(widgets::icon_button_maybe(
            Icon::CloudSync,
            t!(lang, "patches-refresh"),
            refresh_msg,
            STANDARD_PADDING,
        ));

        container(top_row)
            .padding(style::PANE_PADDING)
            .width(Fill)
            .style(widgets::pane)
            .into()
    }

    /// Left sidebar: the searchable patch list — everything the repo
    /// offers, not just what's downloaded. Favorites first (with a
    /// leading gold star), and anything not installed is captioned as
    /// available.
    fn patch_list<'a>(&'a self, patches: &Catalog, config: &crate::config::Config) -> Element<'a, Message> {
        let query = self.search.trim().to_lowercase();
        let mut names: Vec<&str> = patches
            .names()
            .into_iter()
            .filter(|name| self.filter.accepts(patches.installed.contains_key(*name)))
            .filter(|name| {
                query.is_empty()
                    || name.to_lowercase().contains(&query)
                    || patches.title(name).is_some_and(|t| t.to_lowercase().contains(&query))
            })
            .collect();
        // Favorites first, alphabetical within each group.
        names.sort_by(|a, b| {
            let fa = config.favorite_patches.contains(*a);
            let fb = config.favorite_patches.contains(*b);
            fb.cmp(&fa).then_with(|| a.cmp(b))
        });

        let mut list = column![].spacing(2).padding([8, 0]);
        for (idx, name) in names.into_iter().enumerate() {
            let selected = self.selected.as_deref() == Some(name);
            let is_fav = config.favorite_patches.contains(name);
            let title = patches.title(name).unwrap_or(name).to_owned();
            let title_row: Element<'_, Message> = if is_fav {
                row![
                    text("\u{2605}")
                        .size(TEXT_BODY)
                        .style(|_: &iced::Theme| iced::widget::text::Style {
                            color: Some(FAVORITE_GOLD),
                        }),
                    text(title).size(TEXT_BODY),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            } else {
                text(title).size(TEXT_BODY).into()
            };
            // The caption doubles as the installed indicator: a patch
            // you don't have reads as available rather than absent.
            let caption = if patches.installed.contains_key(name) {
                name.to_owned()
            } else {
                format!("{name}  ·  ↓")
            };
            list = list.push(
                button(
                    column![
                        title_row,
                        text(caption)
                            .size(TEXT_CAPTION)
                            .style(widgets::list_caption_style(selected)),
                    ]
                    .spacing(2),
                )
                .padding(style::ROW_PADDING)
                .width(Fill)
                .style(widgets::list_item(selected, idx))
                .on_press(Message::Selected(name.to_owned())),
            );
        }
        container(scrollable(list).style(widgets::chunky_scrollable).height(Fill))
            .width(Length::Fixed(280.0))
            .height(Fill)
            .style(widgets::pane)
            .into()
    }

    /// Right panel: the selected patch's header (favorite toggle,
    /// title, version picker, install/remove), its key:value details,
    /// and the scrollable markdown README beneath.
    fn patch_detail<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        config: &crate::config::Config,
        patches: &Catalog,
        name: &str,
        downloads: &Downloads,
    ) -> Element<'a, Message> {
        let all_versions = patches.versions(name);
        let mut versions: Vec<semver::Version> = all_versions.keys().cloned().collect();
        versions.sort_by(|a, b| b.cmp(a));
        let selected_version = self
            .version
            .clone()
            .filter(|v| all_versions.contains_key(v))
            .or_else(|| versions.first().cloned());

        let info = selected_version.as_ref().and_then(|v| all_versions.get(v));
        let key = selected_version.as_ref().map(|v| (name.to_owned(), v.clone()));

        let is_fav = config.favorite_patches.contains(name);
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
                .on_press(Message::ToggleFavorite(name.to_string()));
            iced::widget::tooltip(
                btn,
                widgets::tooltip_bubble(label),
                iced::widget::tooltip::Position::Bottom,
            )
            .gap(4)
        };

        // Title in a Fill container so a long patch name takes
        // the leftover space and wraps naturally instead of
        // squashing the version picker / action button on the
        // right.
        let title = patches.title(name).unwrap_or(name).to_owned();
        let header = row![
            favorite_toggle,
            container(text(title).size(TEXT_TITLE)).padding([4, 0]).width(Fill),
            widgets::picker(versions, selected_version.clone(), Message::VersionSelected),
        ]
        .spacing(8)
        // Top-align so the action buttons stay anchored when a long
        // title wraps to multiple lines.
        .align_y(Alignment::Start);

        let detail_row = |label: String, value: String| -> Element<'_, Message> {
            row![
                text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                text(value).size(TEXT_CAPTION),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
        };

        // Authors / license / source come from the installed package
        // when we have it, and from the index otherwise.
        let installed_patch = patches.installed.get(name);
        let indexed = selected_version.as_ref().and_then(|v| patches.entry(name, v));
        // Both sources carry the raw `Name <addr>` form, so the same
        // reduction runs over either — installing a patch mustn't change
        // how its authors read.
        let authors = crate::library::patch::display_authors(
            &installed_patch
                .map(|p| p.authors.clone())
                .filter(|a| !a.is_empty())
                .or_else(|| indexed.map(|e| e.authors.clone()))
                .unwrap_or_default(),
        );
        let license = installed_patch
            .and_then(|p| p.license.clone())
            .or_else(|| indexed.and_then(|e| e.license.clone()));
        let source = installed_patch
            .and_then(|p| p.source.clone())
            .or_else(|| indexed.and_then(|e| e.source.clone()));

        let supported_games_str = selected_version
            .as_ref()
            .map(|v| {
                let mut names: Vec<String> = patches
                    .supported_games(name, v)
                    .iter()
                    .map(|g| game::display_name(lang, g))
                    .collect();
                names.sort();
                if names.is_empty() {
                    "—".to_string()
                } else {
                    names.join(", ")
                }
            })
            .unwrap_or_else(|| "—".to_string());

        let mut details = column![].spacing(3);
        if !authors.is_empty() {
            details = details.push(detail_row(t!(lang, "patches-details-authors"), authors.join(", ")));
        }
        if let Some(license) = license {
            details = details.push(detail_row(t!(lang, "patches-details-license"), license));
        }
        if let Some(source) = source {
            details = details.push(detail_row(t!(lang, "patches-details-source"), source));
        }
        details = details.push(detail_row(t!(lang, "patches-details-games"), supported_games_str));
        if let Some(netplay) = info.and_then(|i| i.netplay()) {
            details = details.push(detail_row(
                t!(lang, "patches-netplay-compatibility"),
                netplay_label(lang, netplay),
            ));
        }

        let actions = self.action_row(lang, info, key, downloads);

        // README is flush with the pane edges (no outer padding) so the
        // scrollbar hugs the pane; the markdown body has its own
        // PANE_PADDING inset so the prose doesn't slam the pane wall.
        let readme_body: Element<'_, Message> = if self.readme_items.is_empty() {
            text(t!(lang, "patches-readme-placeholder")).size(TEXT_CAPTION).into()
        } else {
            let theme = crate::ui::theme::theme_for(config);
            let style = crate::ui::theme::markdown_style(&theme);
            iced::widget::markdown::view(
                &self.readme_items,
                iced::widget::markdown::Settings::with_text_size(TEXT_BODY, style),
            )
            .map(Message::ReadmeLinkClicked)
        };

        let meta_pane = container(column![header, details, actions].spacing(6))
            .width(Fill)
            .padding(style::PANE_PADDING)
            .style(widgets::pane);
        let readme_pane = container(
            scrollable(container(readme_body).padding(style::PANE_PADDING).width(Fill))
                .style(widgets::chunky_scrollable),
        )
        .width(Fill)
        .style(widgets::pane);
        column![meta_pane, readme_pane]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// Install / downloading / installed-with-remove, plus the package
    /// size. One fixed-height row in every state so the panel doesn't
    /// jump as a download starts and finishes.
    fn action_row<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        info: Option<&crate::library::patch::VersionInfo<'_>>,
        key: Option<VersionKey>,
        downloads: &Downloads,
    ) -> Element<'a, Message> {
        let Some((info, key)) = info.zip(key) else {
            return row![].height(Length::Fixed(32.0)).into();
        };

        let mut controls = row![].spacing(8).align_y(Alignment::Center);
        if let Some(download) = downloads.get(&key).filter(|d| d.is_running()) {
            let label = match download.percent() {
                Some(percent) => t!(lang, "patches-downloading-progress", percent = percent as i64),
                None => t!(lang, "patches-downloading"),
            };
            controls = controls.push(text(label).size(TEXT_CAPTION).style(widgets::muted_text_style));
        } else if matches!(downloads.get(&key), Some(Download::Failed)) {
            controls = controls.push(
                text(t!(lang, "patches-download-failed"))
                    .size(TEXT_CAPTION)
                    .style(widgets::danger_text_style),
            );
            controls = controls.push(widgets::icon_button(
                Icon::RefreshCw,
                t!(lang, "patches-retry"),
                Message::Install(key.clone()),
                STANDARD_PADDING,
            ));
        } else if info.is_installed() {
            controls = controls.push(
                text(t!(lang, "patches-installed"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            );
            if let Some(path) = info.installed.map(|v| v.path.clone()) {
                controls = controls.push(widgets::icon_button(
                    Icon::FolderOpen,
                    t!(lang, "patches-reveal-package"),
                    Message::RevealPackage(path),
                    STANDARD_PADDING,
                ));
            }
            controls = controls.push(widgets::icon_button(
                Icon::Trash2,
                t!(lang, "patches-uninstall"),
                Message::Uninstall(key.clone()),
                STANDARD_PADDING,
            ));
        } else if info.indexed.is_some() {
            controls = controls.push(widgets::icon_button(
                Icon::Download,
                t!(lang, "patches-install"),
                Message::Install(key.clone()),
                STANDARD_PADDING,
            ));
        }

        if let Some(size) = info.size() {
            controls = controls.push(
                text(human_size(size))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            );
        }

        container(controls)
            .height(Length::Fixed(32.0))
            .align_y(Alignment::Center)
            .into()
    }
}

/// How a version's netplay declaration reads in the UI. The typed
/// declaration means we can say what it *means* rather than echoing an
/// opaque tag string at the user.
fn netplay_label(lang: &LanguageIdentifier, netplay: &tango_patch::Compatibility) -> String {
    match netplay {
        tango_patch::Compatibility::Isolated => t!(lang, "patches-netplay-isolated"),
        tango_patch::Compatibility::Vanilla => t!(lang, "patches-netplay-vanilla"),
        tango_patch::Compatibility::Group(group) => t!(lang, "patches-netplay-group", group = group.clone()),
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1048575 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_filters_partition_the_catalog() {
        // "Available" means offered but not on disk, so the three
        // choices answer different questions instead of two of them
        // showing nearly the same list. A sideloaded patch (installed,
        // not in the index) is Installed, never Available.
        for installed in [true, false] {
            assert!(Filter::All.accepts(installed));
        }
        assert!(Filter::Installed.accepts(true));
        assert!(!Filter::Installed.accepts(false));
        assert!(Filter::Available.accepts(false));
        assert!(!Filter::Available.accepts(true));
    }

    #[test]
    fn the_default_filter_hides_nothing() {
        assert_eq!(Filter::default(), Filter::All);
    }

    fn v(s: &str) -> semver::Version {
        s.parse().unwrap()
    }

    /// A catalog offering one patch that isn't downloaded, whose index
    /// entry advertises a README sidecar.
    fn offered_with_readme() -> Catalog {
        let mut index = tango_patch::Index::default();
        index.patches.entry("bn6_test".to_owned()).or_default().insert(
            v("1.0.0"),
            tango_patch::index::Entry {
                title: "Test".into(),
                authors: vec![],
                license: None,
                source: None,
                netplay: tango_patch::Compatibility::Isolated,
                games: vec!["BR6E_00".parse().unwrap()],
                path: "bn6_test/bn6_test-1.0.0.tangopatch".into(),
                size: 1,
                sha256: "0".repeat(64),
                readme: Some("bn6_test/bn6_test-1.0.0.README.md".into()),
            },
        );
        Catalog {
            installed: Default::default(),
            index,
        }
    }

    #[test]
    fn selecting_an_undownloaded_patch_asks_for_its_readme() {
        let patches = offered_with_readme();
        let mut state = PatchesState::default();
        let effect = state.update(Message::Selected("bn6_test".into()), &patches);
        assert!(
            matches!(&effect, Some(Effect::FetchReadme(key)) if key == &("bn6_test".to_owned(), v("1.0.0"))),
            "{effect:?}"
        );
        // Nothing to show yet, and the version resolved from the index.
        assert!(state.readme_items.is_empty());
        assert_eq!(state.version, Some(v("1.0.0")));
    }

    #[test]
    fn a_fetched_readme_renders_without_reselecting() {
        let patches = offered_with_readme();
        let mut state = PatchesState::default();
        state.update(Message::Selected("bn6_test".into()), &patches);

        let key = ("bn6_test".to_owned(), v("1.0.0"));
        state.update(Message::ReadmeFetched(key, Some("# hello".into())), &patches);
        assert!(
            !state.readme_items.is_empty(),
            "the fetched readme should be parsed as soon as it lands"
        );
    }

    #[test]
    fn a_patch_with_no_published_readme_is_only_asked_about_once() {
        let patches = offered_with_readme();
        let mut state = PatchesState::default();
        state.update(Message::Selected("bn6_test".into()), &patches);

        // The repo had nothing after all.
        let key = ("bn6_test".to_owned(), v("1.0.0"));
        state.update(Message::ReadmeFetched(key.clone(), None), &patches);
        assert!(state.readme_items.is_empty());

        // Reselecting must not re-request it.
        state.selected = None;
        state.version = None;
        state.readme_cache_key = None;
        let effect = state.update(Message::Selected("bn6_test".into()), &patches);
        assert!(effect.is_none(), "{effect:?}");
    }
}

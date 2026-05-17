use crate::i18n::t;
use crate::icons;
use crate::{config, replays, save_view, Scanners, STANDARD_PADDING, STANDARD_TEXT_SIZE};
use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, pick_list, row, scrollable, text, vertical_rule,
    Space,
};
use iced::{Alignment, Element, Fill, Length};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone)]
pub enum Message {
    FolderFilterSelected(FolderOption),
    Selected(std::path::PathBuf),
    OpenFolder(std::path::PathBuf),
    Watch(std::path::PathBuf),
    Rescan,
    SaveTabSelected(save_view::Tab),
    SaveSideSelected(crate::selection::ReplaySide),
}

#[derive(Default)]
pub struct ReplaysState {
    /// `None` = no folder filter (show all); `Some` = restrict to direct
    /// children of this dir.
    pub folder_filter: Option<std::path::PathBuf>,
    pub selected: Option<std::path::PathBuf>,
    /// Cached Loaded for the currently-selected replay's chosen side.
    /// Rebuilt by the App's Selected/SaveSideSelected handlers; the
    /// view borrows it read-only.
    pub loaded: Option<crate::selection::Loaded>,
    /// Path the cached `loaded` was built for. Used to invalidate the
    /// cache when the selection changes.
    pub loaded_cache_key: Option<(std::path::PathBuf, crate::selection::ReplaySide)>,
    pub save_side: crate::selection::ReplaySide,
    pub save_tab: Option<save_view::Tab>,
}

impl ReplaysState {
    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        config: &'a config::Config,
    ) -> Element<'a, Message> {
        let replays_path = config.replays_path();
        let replays = scanners.replays.read();

        // Top: folder filter dropdown. Default option is "all".
        let all_label = t(lang, "replays-all-replays");
        let mut folder_options = vec![FolderOption::all(all_label.clone())];
        {
            use itertools::Itertools;
            let mut parents: Vec<std::path::PathBuf> = replays
                .iter()
                .flat_map(|r| r.path.parent().map(|p| p.to_path_buf()))
                .unique()
                .collect();
            parents.sort();
            for p in parents {
                let display = replays::format_rel_path(&replays_path, &p);
                folder_options.push(FolderOption {
                    path: Some(p),
                    display,
                });
            }
        }
        let selected_folder = folder_options
            .iter()
            .find(|f| f.path == self.folder_filter)
            .cloned()
            .unwrap_or_else(|| folder_options[0].clone());
        let top = container(
            row![
                text(format!("{}:", t(lang, "replays-folder-label"))),
                pick_list(folder_options, Some(selected_folder), Message::FolderFilterSelected),
                horizontal_space(),
                icons::icon_button(
                    icons::RESCAN,
                    t(lang, "rescan"),
                    Message::Rescan,
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding(8),
        )
        .width(Fill);

        // Left list. Pre-filter by folder, then build rows.
        let folder_filter = self.folder_filter.as_ref();
        let filtered: Vec<&replays::ScannedReplay> = replays
            .iter()
            .filter(|r| {
                folder_filter
                    .map(|f| r.path.parent().map(|p| p == f.as_path()).unwrap_or(false))
                    .unwrap_or(true)
            })
            .collect();

        let mut list = column![].spacing(0).padding(8);
        for r in &filtered {
            let md = &r.metadata;
            let local_nick = md.local_side.as_ref().map(|s| s.nickname.clone()).unwrap_or_default();
            let remote_nick = md.remote_side.as_ref().map(|s| s.nickname.clone()).unwrap_or_default();

            let ts_str = std::time::UNIX_EPOCH
                .checked_add(std::time::Duration::from_millis(md.ts))
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Local> = t.into();
                    dt.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_else(|| "(?)".to_string());

            let game_family = md
                .local_side
                .as_ref()
                .and_then(|s| s.game_info.as_ref())
                .map(|g| g.rom_family.clone())
                .unwrap_or_default();
            let nick_pair = if remote_nick.is_empty() && local_nick.is_empty() {
                md.link_code.clone()
            } else {
                format!("{local_nick} vs {remote_nick}")
            };

            let selected = self.selected.as_ref() == Some(&r.path);
            let style = if selected { button::primary } else { button::text };
            list = list.push(
                button(
                    column![
                        text(ts_str).size(13),
                        text(format!(
                            "{game_family} @ {}  ·  {nick_pair}",
                            md.link_code
                        ))
                        .size(11)
                        .style(save_view::muted_text_style),
                    ]
                    .spacing(2),
                )
                .padding(6)
                .width(Fill)
                .style(style)
                .on_press(Message::Selected(r.path.clone())),
            );
        }
        let left = container(scrollable(list).height(Fill))
            .width(Length::Fixed(360.0))
            .height(Fill);

        // Right panel.
        let right: Element<'_, Message> = if let Some(sel_path) = self.selected.as_ref() {
            if let Some(r) = filtered.iter().find(|r| &r.path == sel_path) {
                replay_detail(lang, r, &replays_path, self)
            } else {
                container(text(t(lang, "replays-select-prompt")).size(13))
                    .center(Fill)
                    .into()
            }
        } else {
            container(text(t(lang, "replays-select-prompt")).size(13))
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

fn replay_detail<'a>(
    lang: &'a LanguageIdentifier,
    r: &'a replays::ScannedReplay,
    replays_path: &'a std::path::Path,
    state: &'a ReplaysState,
) -> Element<'static, Message> {
    let md = &r.metadata;
    let ts_str = std::time::UNIX_EPOCH
        .checked_add(std::time::Duration::from_millis(md.ts))
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d %H:%M:%S %z").to_string()
        })
        .unwrap_or_else(|| "(?)".to_string());

    let row_for_side = |label: String, side: Option<&tango_pvp::replay::metadata::Side>| -> Element<'static, Message> {
        let nick = side.map(|s| s.nickname.clone()).unwrap_or_default();
        let gi = side.and_then(|s| s.game_info.as_ref());
        let game = gi.map(|g| format!("{} v{}", g.rom_family, g.rom_variant)).unwrap_or_default();
        let patch = gi.and_then(|g| g.patch.as_ref()).map(|p| format!("{} v{}", p.name, p.version));
        let mut col = column![
            text(label).size(11).style(save_view::muted_text_style),
            text(nick).size(14),
            text(game).size(12),
        ]
        .spacing(2);
        if let Some(p) = patch {
            col = col.push(
                text(p)
                    .size(11)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().primary),
                    }),
            );
        }
        container(col).width(Length::Fill).into()
    };

    let parent_str = r
        .path
        .parent()
        .map(|p| replays::format_rel_path(replays_path, p))
        .unwrap_or_else(|| "/".to_string());
    let filename = r
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let title = format!("{} @ {}", t(lang, "replays-watch"), md.link_code);

    // Save preview: only renders if the App has built `state.loaded`
    // for the current selection + side. Builds an internal tab strip
    // (cover / navi / folder / patch cards / auto battle data) plus a
    // "You / Opponent" side toggle.
    let preview: Element<'_, Message> = if let Some(loaded) = state.loaded.as_ref() {
        let available = save_view::available_tabs(loaded.save.as_ref(), false);
        if available.is_empty() {
            container(text(t(lang, "save-empty")).size(12).style(save_view::muted_text_style))
                .padding(8)
                .into()
        } else {
            let active = state
                .save_tab
                .filter(|t| available.contains(t))
                .unwrap_or(available[0]);

            let opts = save_view::RenderOpts { folder_grouped: true };

            let tab_button = |tab: save_view::Tab| {
                let style = if tab == active { iced::widget::button::primary } else { iced::widget::button::text };
                icons::labeled_icon_button(
                    save_tab_icon(tab),
                    t(lang, save_view::tab_key(tab)),
                    Message::SaveTabSelected(tab),
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                    style,
                )
            };
            let mut tab_row = row![].spacing(2).align_y(Alignment::Center);
            for tab in &available {
                tab_row = tab_row.push(tab_button(*tab));
            }

            // You / Opponent side toggle.
            let side_button = |side: crate::selection::ReplaySide, label: String| {
                let style = if state.save_side == side {
                    iced::widget::button::primary
                } else {
                    iced::widget::button::text
                };
                iced::widget::button(text(label).size(STANDARD_TEXT_SIZE))
                    .padding(STANDARD_PADDING)
                    .style(style)
                    .on_press(Message::SaveSideSelected(side))
            };
            let side_row = row![
                side_button(crate::selection::ReplaySide::Local, t(lang, "play-you")),
                side_button(crate::selection::ReplaySide::Remote, t(lang, "replays-opponent")),
            ]
            .spacing(4);

            let body = save_view::render::<Message>(lang, active, loaded, opts);

            iced::widget::scrollable(
                column![
                    side_row,
                    tab_row,
                    body,
                ]
                .spacing(8)
                .padding(8),
            )
            .height(Fill)
            .into()
        }
    } else {
        container(text(t(lang, "save-empty")).size(12).style(save_view::muted_text_style))
            .padding(8)
            .into()
    };

    container(
        column![
            row![
                text(title).size(18),
                horizontal_space(),
                icons::icon_button(
                    icons::WATCH,
                    t(lang, "replays-watch"),
                    Message::Watch(r.path.clone()),
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                ),
                icons::icon_button_maybe::<Message>(
                    icons::EXPORT,
                    t(lang, "replays-export"),
                    None,
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                ),
                icons::icon_button(
                    icons::FOLDER,
                    t(lang, "patches-open-folder"),
                    Message::OpenFolder(
                        r.path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
                    ),
                    STANDARD_TEXT_SIZE,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            text(ts_str).size(12).style(save_view::muted_text_style),
            text(format!("{parent_str}{filename}")).size(11).style(save_view::muted_text_style),
            Space::with_height(8),
            horizontal_rule(1),
            Space::with_height(8),
            row![
                row_for_side(t(lang, "play-you"), md.local_side.as_ref()),
                vertical_rule(1),
                row_for_side(t(lang, "replays-opponent"), md.remote_side.as_ref()),
            ]
            .spacing(12)
            .height(Length::Shrink),
            Space::with_height(8),
            text(format!(
                "{}: {}.{}",
                t(lang, "replays-match-type"),
                md.match_type,
                md.match_subtype
            ))
            .size(12),
            Space::with_height(8),
            horizontal_rule(1),
            preview,
        ]
        .spacing(6)
        .padding(16),
    )
    .width(Fill)
    .height(Fill)
    .into()
}

/// Per-save-tab icon glyph. Mirrors `crate::tabs::play::save_tab_icon`.
fn save_tab_icon(tab: save_view::Tab) -> &'static str {
    use crate::icons;
    match tab {
        save_view::Tab::Cover => icons::SAVE_COVER,
        save_view::Tab::Navi => icons::SAVE_NAVI,
        save_view::Tab::Folder => icons::SAVE_FOLDER,
        save_view::Tab::PatchCards => icons::SAVE_PATCH_CARDS,
        save_view::Tab::AutoBattleData => icons::SAVE_AUTO_BATTLE,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FolderOption {
    pub path: Option<std::path::PathBuf>,
    pub display: String,
}
impl FolderOption {
    fn all(label: String) -> Self {
        Self { path: None, display: label }
    }
}
impl std::fmt::Display for FolderOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

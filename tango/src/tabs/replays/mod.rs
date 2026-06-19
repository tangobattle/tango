use crate::app::Scanners;
use crate::i18n::t;
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::widgets;
use crate::{config, replays, save_view};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, pick_list, row, text_input};
use unic_langid::LanguageIdentifier;

mod export;
pub use export::{ExportJob, ExportMessage, ExportSettings, PerReplay};

#[derive(Debug, Clone)]
pub enum Message {
    /// Picked a game from the Game filter dropdown. `None` =
    /// "All games".
    /// `None` = "all games"; otherwise the ROM family (e.g. "bn6").
    GameFilterSelected(Option<String>),
    /// Typed in the opponent-filter text input. Empty = no
    /// filter; otherwise a substring (case-insensitive) match
    /// against the remote side's nickname.
    OpponentFilterChanged(String),
    /// User toggled the "show incomplete" checkbox in the top
    /// filter row. Off by default — incomplete replays (the
    /// recorded stream didn't reach `END_OF_REPLAY`) are hidden
    /// from the list so the default view shows finished matches
    /// only.
    ShowIncompleteToggled(bool),
    Selected(std::path::PathBuf),
    OpenFolder(std::path::PathBuf),
    Watch(std::path::PathBuf),
    /// Export-panel interactions (form, render lifecycle, round
    /// mask), folded under one variant — see [`ExportMessage`] and
    /// [`ReplaysState::update_export`].
    Export(ExportMessage),
    /// Lazy-load result from `replays::compute_stats`. The App
    /// kicks one worker per missing path post-scan; each result
    /// arrives as one of these messages and lands in
    /// [`ReplaysState::stats`].
    StatsLoaded(std::path::PathBuf, crate::replays::ReplayStats),
    Rescan,
    SaveViewAction(save_view::Action),
    /// Used by Tasks that need a Message to return but want no
    /// state mutation. Currently: the user dismissed the Save As
    /// file dialog without picking a path — the export form should
    /// stay open and untouched.
    NoOp,
}

#[derive(Default)]
pub struct ReplaysState {
    /// `(family, variant)` pair the replays' local-side must match.
    /// `None` = "All games". Cleared when the corresponding pair
    /// no longer appears in the scanned replays (e.g. user
    /// deleted them).
    /// Filter replays by ROM family (e.g. "bn6"). `None` = "All
    /// games". Intentionally NOT keyed on variant — "BN6" should
    /// pull both Gregar and Falzar replays since the family is
    /// the unit opponents are paired on.
    pub game_filter: Option<String>,
    /// Substring (case-insensitive) match against the remote
    /// side's nickname. Empty = no filter.
    pub opponent_filter: String,
    /// When false (the default), replays whose loaded stats say
    /// `is_complete = false` are hidden from the list. Replays
    /// without loaded stats yet are always shown — we only know
    /// they're incomplete once the lazy stats worker reports.
    pub show_incomplete: bool,
    pub selected: Option<std::path::PathBuf>,
    /// Cached Loaded for the currently-selected replay's local side.
    /// Rebuilt by the App's `Selected` handler; view borrows read-only.
    pub loaded: Option<crate::selection::Loaded>,
    /// Path the cached `loaded` was built for. Used to invalidate the
    /// cache when the selection changes.
    pub loaded_cache_path: Option<std::path::PathBuf>,
    pub save_view: save_view::State,
    /// Per-replay UI state, keyed by replay path. Entries appear
    /// on first interaction (Selected, ExportPanelOpen, or
    /// ExportStart) and are pruned on navigation if they hold
    /// no in-flight render.
    pub per: std::collections::HashMap<std::path::PathBuf, PerReplay>,
    /// Export form defaults — these are *global* user preferences
    /// (scale, lossless, mute), not per-replay choices, so they
    /// live outside `per`.
    pub export_settings: ExportSettings,
    /// Lazy-loaded duration/round/completion stats keyed by replay
    /// path. Populated by the App's background worker after a
    /// scan; sidebar reads it to render the second caption line.
    /// Missing entries just hide that line until the worker fills
    /// them in.
    pub stats: std::collections::HashMap<std::path::PathBuf, crate::replays::ReplayStats>,
    /// Entrance restarted when a different replay is selected —
    /// the detail panel slides in from the right.
    pub detail_enter: crate::anim::Enter,
}

/// Side-effects the tab can't perform itself (because they touch
/// the file system, clipboard, session host, or async runtime).
/// `ReplaysState::update` returns at most one of these per
/// dispatch; the App handler interprets it.
#[derive(Debug)]
pub enum Effect {
    /// `open::that(_)` — folder or rendered video.
    OpenPath(std::path::PathBuf),
    /// User clicked Watch on a replay; App spawns the playback
    /// session and stuffs it into `session.active`.
    Watch(std::path::PathBuf),
    /// User clicked Rescan; App re-scans roms / saves / patches /
    /// replays + refreshes any cached Loaded.
    Rescan,
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// Open the native Save-File dialog for the given replay's
    /// rendered video. App picks a path async and dispatches
    /// `Message::ExportStart`. `lossless` selects the default
    /// extension/filter: .mkv for lossless (libx264rgb + flac), .mp4
    /// for scaled exports.
    OpenExportSaveDialog { replay: std::path::PathBuf, lossless: bool },
    /// User confirmed an export. App decodes the replay, resolves
    /// hooks + ROMs, spawns the tango_pvp::replay::export task,
    /// and streams `Message::ExportProgress` / `ExportFinished`
    /// back into this module.
    StartExport {
        replay: std::path::PathBuf,
        output: std::path::PathBuf,
        settings: ExportSettings,
        rounds: Vec<bool>,
    },
    /// Task returned from save_view::State::apply. Generic Task
    /// pipe so save_view-internal side effects (currently just
    /// the scroll-to-top snap on tab changes) flow through here
    /// without per-feature Effect variants.
    SaveViewTask(iced::Task<Message>),
}

impl ReplaysState {
    /// Apply a tab message. Pure UI-state mutations happen
    /// in-place; anything that needs the App's collaborators
    /// (clipboard, file dialog, session host, …) is bubbled up
    /// as a single optional [`Effect`].
    pub fn update(&mut self, msg: Message, scanners: &Scanners, config: &config::Config) -> Option<Effect> {
        match msg {
            Message::GameFilterSelected(pair) => {
                self.game_filter = pair;
                // Filter change can hide the current selection;
                // drop the cached Loaded so the next interaction
                // doesn't show a now-filtered-out detail panel.
                self.clear_selection();
                None
            }
            Message::OpponentFilterChanged(s) => {
                // Don't clear the selection on every keystroke —
                // the user might be refining the filter while
                // keeping a replay open. The view simply omits
                // the detail panel when the selected path no
                // longer matches the current filtered list.
                self.opponent_filter = s;
                None
            }
            Message::ShowIncompleteToggled(v) => {
                // Same rule as opponent-filter: don't clear the
                // selection — the user might have an incomplete
                // replay open while toggling. The detail panel
                // re-checks membership itself, so a now-filtered-
                // out selection just hides the right column.
                self.show_incomplete = v;
                None
            }
            Message::Selected(p) => {
                if self.selected.as_ref() != Some(&p) {
                    self.detail_enter.start(iced::time::Instant::now());
                }
                self.selected = Some(p);
                self.refresh_loaded(scanners, config);
                self.sweep_idle_entries();
                None
            }
            Message::OpenFolder(p) => Some(Effect::OpenPath(p)),
            Message::Watch(p) => Some(Effect::Watch(p)),
            Message::Rescan => Some(Effect::Rescan),
            Message::SaveViewAction(action) => {
                let sv_task = self.save_view.apply(&action);
                // Clipboard variants need the App's clipboard
                // collaborator — bubble them up as Effects.
                // Anything else gets folded into save_view-internal
                // state and surfaces as a generic SaveViewTask
                // (currently used for the scroll-to-top snap on a
                // tab change).
                match action {
                    save_view::Action::CopyTab(tab) => {
                        let opts = save_view::RenderOpts {
                            folder_grouped: self.save_view.folder_grouped,
                        };
                        let effect = self
                            .loaded
                            .as_ref()
                            .and_then(|l| save_view::tab_as_text(&config.language, tab, l, opts))
                            .map(Effect::CopyText);
                        // Only a copy that actually produced text
                        // earns the "Copied!" flash.
                        if effect.is_some() {
                            crate::copy_feedback::flash(&save_view::copy_flash_key(tab, false));
                        }
                        effect
                    }
                    save_view::Action::CopyTabImage(tab) => {
                        let effect = self
                            .loaded
                            .as_ref()
                            .and_then(|l| save_view::tab_as_image(tab, l))
                            .map(Effect::CopyImage);
                        if effect.is_some() {
                            crate::copy_feedback::flash(&save_view::copy_flash_key(tab, true));
                        }
                        effect
                    }
                    _ => Some(Effect::SaveViewTask(sv_task.map(Message::SaveViewAction))),
                }
            }
            Message::Export(m) => self.update_export(m),
            Message::StatsLoaded(path, s) => {
                self.stats.insert(path, s);
                None
            }
            Message::NoOp => None,
        }
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.loaded = None;
        self.loaded_cache_path = None;
        self.sweep_idle_entries();
    }

    /// Decode the currently-selected replay just enough to build
    /// its save-view Loaded + populate the round count for the
    /// export form. Cached against the selected path so this only
    /// re-runs on selection change.
    fn refresh_loaded(&mut self, scanners: &Scanners, config: &config::Config) {
        let Some(path) = self.selected.clone() else {
            self.loaded = None;
            self.loaded_cache_path = None;
            return;
        };
        if self.loaded_cache_path.as_ref() == Some(&path) {
            return;
        }
        let res = (|| -> anyhow::Result<(crate::selection::Loaded, usize)> {
            let f = std::fs::File::open(&path)?;
            let replay = tango_pvp::replay::Replay::decode(f)?;
            let rounds = replay.rounds.len();
            let loaded = crate::selection::Loaded::for_replay_local(scanners, config, &replay)?;
            Ok((loaded, rounds))
        })();
        match res {
            Ok((loaded, rounds)) => {
                self.loaded = Some(loaded);
                self.loaded_cache_path = Some(path.clone());
                // Default to all-rounds-checked on every fresh
                // selection; export form reads this snapshot.
                self.per.entry(path).or_default().rounds = vec![true; rounds];
            }
            Err(e) => {
                log::warn!("replay save preview failed: {e}");
                self.loaded = None;
                self.loaded_cache_path = None;
                if let Some(p) = self.selected.as_ref() {
                    if let Some(entry) = self.per.get_mut(p) {
                        entry.rounds.clear();
                    }
                }
            }
        }
    }

    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        config: &'a config::Config,
        netplay_phase: &'a crate::netplay::Phase,
        rescanning: bool,
    ) -> Element<'a, Message> {
        // Replay playback spawns an emulator session that would
        // conflict with an active netplay session. Disable the
        // Watch button anywhere the netplay phase isn't Idle —
        // user has to disconnect / dismiss the lobby first.
        let netplay_active = !matches!(netplay_phase, crate::netplay::Phase::Idle);
        let replays_path = config.replays_path();
        let replays = scanners.replays.read();

        let top = self.filter_strip(lang, &replays, rescanning);

        // Left list — AND of game + opponent + completeness filters.
        let filtered: Vec<&replays::ScannedReplay> = replays.iter().filter(|r| self.matches_filters(r)).collect();
        let mut list = column![].spacing(2).padding([8, 0]);
        for (idx, r) in filtered.iter().enumerate() {
            list = list.push(self.replay_list_row(lang, r, idx));
        }
        let left = container(scrollable(list).style(widgets::chunky_scrollable).height(Fill))
            .width(Length::Fixed(360.0))
            .height(Fill)
            .style(widgets::pane);

        // Right panel: replay_detail returns a column of panes
        // when something is selected; the empty-state collapses to
        // a single centered pane.
        let right: Element<'_, Message> = if let Some(r) = self
            .selected
            .as_ref()
            .and_then(|sel_path| filtered.iter().find(|r| &r.path == sel_path))
        {
            let detail = replay_detail(lang, r, &replays_path, self, scanners, netplay_active);
            // Selection entrance: the detail panel rises up into
            // place.
            match self.detail_enter.progress(iced::time::Instant::now()) {
                Some(p) => crate::anim::slide_in(detail, p, iced::Vector::new(0.0, 28.0)),
                None => detail,
            }
        } else {
            widgets::pane_prompt(t!(lang, "replays-select-prompt"))
        };

        widgets::top_split_pane(top, left, right)
    }

    /// Top strip: game + opponent filter dropdowns plus the
    /// show-incomplete toggle and rescan button. Options are derived
    /// from the distinct values seen across the scanned replays'
    /// local/remote metadata; "All …" is always the first option.
    fn filter_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        replays: &[replays::ScannedReplay],
        rescanning: bool,
    ) -> Element<'a, Message> {
        let all_games = t!(lang, "replays-filter-all-games");
        let mut game_options = vec![widgets::Choice::new(None, all_games.clone())];
        {
            use itertools::Itertools;
            // Dedupe by family only — the filter ignores variant,
            // so listing "BN6" once covers both Gregar and Falzar.
            let mut seen: Vec<String> = replays
                .iter()
                .filter_map(|r| {
                    let gi = r.metadata.local_side.as_ref()?.game_info.as_ref()?;
                    Some(gi.rom_family.clone())
                })
                .unique()
                .collect();
            seen.sort();
            for family in seen {
                let display = family_display_name(lang, &family, 0);
                game_options.push(widgets::Choice::new(Some(family), display));
            }
        }
        let selected_game = game_options
            .iter()
            .find(|o| o.value == self.game_filter)
            .cloned()
            .unwrap_or_else(|| game_options[0].clone());
        let show_incomplete_toggle = iced::widget::checkbox(self.show_incomplete)
            .on_toggle(Message::ShowIncompleteToggled)
            .label(t!(lang, "replays-show-incomplete"))
            .size(TEXT_BODY)
            .text_size(TEXT_BODY)
            .style(widgets::chunky_checkbox);
        container(
            row![
                pick_list(
                    game_options,
                    Some(selected_game),
                    |o: widgets::Choice<Option<String>>| { Message::GameFilterSelected(o.value) }
                )
                .padding(STANDARD_PADDING)
                .style(widgets::chunky_pick_list),
                text_input(&t!(lang, "replays-filter-opponent-placeholder"), &self.opponent_filter,)
                    .on_input(Message::OpponentFilterChanged)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fixed(220.0))
                    .style(widgets::chunky_text_input),
                show_incomplete_toggle,
                horizontal_space(),
                widgets::icon_button_maybe(
                    Icon::RefreshCw,
                    t!(lang, "rescan"),
                    (!rescanning).then_some(Message::Rescan),
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(style::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane)
        .into()
    }

    /// AND of the game + opponent + completeness filters. Opponent
    /// match is case-insensitive substring (mirrors the text-input
    /// UX). Completeness only drops a row once its stats have actually
    /// loaded — unloaded entries pass through so a freshly-scanned
    /// replay isn't hidden during the lazy stats-worker window.
    fn matches_filters(&self, r: &replays::ScannedReplay) -> bool {
        let g_ok = self
            .game_filter
            .as_ref()
            .map(|family| {
                r.metadata
                    .local_side
                    .as_ref()
                    .and_then(|s| s.game_info.as_ref())
                    .map(|gi| gi.rom_family == *family)
                    .unwrap_or(false)
            })
            .unwrap_or(true);
        let opp_needle = self.opponent_filter.trim().to_lowercase();
        let o_ok = if opp_needle.is_empty() {
            true
        } else {
            r.metadata
                .remote_side
                .as_ref()
                .map(|s| s.nickname.to_lowercase().contains(&opp_needle))
                .unwrap_or(false)
        };
        let c_ok = self.show_incomplete || self.stats.get(&r.path).map(|s| s.is_complete).unwrap_or(false);
        g_ok && o_ok && c_ok
    }

    /// One row of the replay list: timestamp + status glyph, the
    /// "game @ code · nicknames" line, an optional stats line once the
    /// lazy stats worker gets here, and a bottom progress strip while
    /// an export render is in flight.
    fn replay_list_row<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        r: &replays::ScannedReplay,
        idx: usize,
    ) -> Element<'a, Message> {
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

        let local_gi = md.local_side.as_ref().and_then(|s| s.game_info.as_ref());
        let game_label = local_gi
            .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
            .and_then(|(family, variant)| crate::game::find_by_family_and_variant(family, variant))
            .map(|g| crate::game::short_name(lang, g))
            .or_else(|| local_gi.map(|g| g.rom_family.clone()))
            .unwrap_or_default();
        let nick_pair = if remote_nick.is_empty() && local_nick.is_empty() {
            link_code_display(lang, &md.link_code).into_owned()
        } else {
            format!("{local_nick} vs {remote_nick}")
        };

        let selected = self.selected.as_ref() == Some(&r.path);
        // Right-edge status glyph: a clapperboard while a render
        // is in flight, a green check on success, a red X on
        // failure. In-flight renders additionally get a progress
        // bar flush along the row's bottom edge (see
        // `progress_strip`).
        let job_state = self.job(&r.path);
        let rendering = matches!(job_state, Some(j) if j.result.is_none());
        let render_done_ok = matches!(job_state, Some(j) if matches!(&j.result, Some(Ok(_))));
        let render_done_err = matches!(job_state, Some(j) if matches!(&j.result, Some(Err(_))));
        let badge: Element<'_, Message> = if rendering {
            container(
                Icon::Clapperboard
                    .widget()
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().primary),
                    }),
            )
            .padding([0, 4])
            .into()
        } else if render_done_ok {
            container(
                Icon::Check
                    .widget()
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(theme.palette().success),
                    }),
            )
            .padding([0, 4])
            .into()
        } else if render_done_err {
            container(Icon::X.widget().style(|theme: &iced::Theme| iced::widget::text::Style {
                color: Some(theme.palette().danger),
            }))
            .padding([0, 4])
            .into()
        } else {
            Space::new().width(Length::Fixed(0.0)).into()
        };
        // Bottom progress strip. While an export is in flight we
        // draw a full-width bar flush with the row's bottom edge
        // (no label — the detail panel carries the percentage);
        // otherwise we reserve the same height with an empty
        // spacer so toggling the bar on never shifts row height.
        let progress_strip: Element<'_, Message> = if rendering {
            let pct = match job_state.filter(|j| j.total > 0) {
                Some(j) => (j.completed as f32 / j.total as f32).clamp(0.0, 1.0),
                None => 0.0,
            };
            iced::widget::progress_bar(0.0..=1.0, pct)
                .girth(Length::Fixed(4.0))
                .style(|theme: &iced::Theme| {
                    iced::widget::progress_bar::Style {
                        // Transparent track lets the row's own
                        // background show through — so it matches
                        // exactly, including the zebra stripe on
                        // alternating rows. Only the filled portion
                        // reads as progress. Square corners, no
                        // border — a flush bottom-edge accent.
                        background: iced::Background::Color(iced::Color::TRANSPARENT),
                        bar: iced::Background::Color(theme.palette().primary),
                        border: iced::Border {
                            radius: 0.0.into(),
                            width: 0.0,
                            color: iced::Color::TRANSPARENT,
                        },
                    }
                })
                .into()
        } else {
            Space::new().height(Length::Fixed(4.0)).into()
        };
        // Match-type name (e.g. "Triple") for the stats line.
        let family = local_gi.map(|g| g.rom_family.clone()).unwrap_or_default();
        let type_name = crate::game::match_type_name(lang, &family, md.match_type as u8, md.match_subtype as u8);
        // Stats line: "Triple (2 rounds) · 0:42" once the lazy
        // stats worker gets here, with " · incomplete" tacked on
        // when the recorded stream didn't reach END_OF_REPLAY.
        // Composed from the per-locale match-type-value + the
        // shared "incomplete" string so we don't carry a
        // dedicated stats-line template just to glue them.
        let stats = self.stats.get(&r.path);
        let stats_line = stats.map(|s| {
            let rounds = t!(lang, "replays-round-count", count = s.round_count as i64);
            let mut parts = vec![type_name.clone(), rounds, format_duration(s.tick_count)];
            if !s.is_complete {
                parts.push(t!(lang, "replays-incomplete"));
            }
            parts.join(" · ")
        });
        let is_complete = stats.map(|s| s.is_complete).unwrap_or(true);
        // Two static caption lines, optionally a third with
        // duration / rounds / incomplete (only when stats
        // have loaded for this row).
        let mut text_col = column![
            // Title line carries the status glyph pinned to its
            // right. Keeping it on this fixed first line (rather
            // than vertically centered across the whole row) means
            // it never moves as the optional stats line loads or
            // the glyph changes.
            row![text(ts_str).size(TEXT_BODY), Space::new().width(Fill), badge].align_y(Alignment::Center),
            text(format!(
                "{game_label} @ {}  ·  {nick_pair}",
                link_code_display(lang, &md.link_code)
            ))
            .size(TEXT_CAPTION)
            .style(move |theme: &iced::Theme| if selected {
                iced::widget::text::Style { color: None }
            } else {
                widgets::muted_text_style(theme)
            }),
        ]
        .spacing(2)
        .width(Fill);
        if let Some(line) = stats_line {
            text_col = text_col.push(text(line).size(TEXT_CAPTION).style(move |theme: &iced::Theme| {
                if !is_complete {
                    widgets::danger_text_style(theme)
                } else if selected {
                    iced::widget::text::Style { color: None }
                } else {
                    widgets::muted_text_style(theme)
                }
            }));
        }
        button(
            column![
                container(text_col).padding(style::ROW_PADDING).width(Fill),
                progress_strip,
            ]
            .width(Fill),
        )
        .padding(0)
        .width(Fill)
        .style(widgets::list_item(selected, idx))
        .on_press(Message::Selected(r.path.clone()))
        .into()
    }
}

fn replay_detail<'a>(
    lang: &'a LanguageIdentifier,
    r: &replays::ScannedReplay,
    replays_path: &std::path::Path,
    state: &'a ReplaysState,
    scanners: &'a Scanners,
    netplay_active: bool,
) -> Element<'a, Message> {
    // Playback needs a scanned ROM for the local-side game; without
    // one the emulator session would error on construction. Resolve
    // now so the Watch button can disable + explain.
    let local_rom_present = r
        .metadata
        .local_side
        .as_ref()
        .and_then(|s| s.game_info.as_ref())
        .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
        .and_then(|(family, variant)| crate::game::find_by_family_and_variant(family, variant))
        .map(|g| scanners.roms.read().contains_key(&g))
        .unwrap_or(false);
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
        let game_line = gi
            .map(|g| {
                let mut s = family_display_name(lang, &g.rom_family, g.rom_variant);
                if let Some(p) = g.patch.as_ref() {
                    s.push_str(&format!(" · {} v{}", p.name, p.version));
                }
                s
            })
            .unwrap_or_default();
        let col = column![
            text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
            text(nick).size(TEXT_TITLE),
            text(game_line).size(TEXT_CAPTION),
        ]
        .spacing(2);
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

    let game_short = md
        .local_side
        .as_ref()
        .and_then(|s| s.game_info.as_ref())
        .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
        .and_then(|(family, variant)| crate::game::find_by_family_and_variant(family, variant))
        .map(|g| crate::game::short_name(lang, g))
        .unwrap_or_else(|| "?".to_string());
    let title = format!("{game_short} @ {}", link_code_display(lang, &md.link_code));

    // Title + metadata pane: title row with action buttons, then
    // export panel, timestamp, file path.
    let title_pane = container(
        column![
            row![
                // Title in a Fill container so a long link code
                // wraps naturally without squashing the action
                // buttons on the right.
                container(text(title).size(18)).width(Fill),
                {
                    // Per-replay toggle. Disabled outright while a
                    // render for this replay is in flight, so the
                    // user can't even attempt to close the panel
                    // mid-render (which would otherwise be a
                    // no-op, but a dead button is more honest).
                    let msg = if state.is_rendering(&r.path) {
                        None
                    } else if state.is_panel_open(&r.path) {
                        Some(Message::Export(ExportMessage::PanelClose(r.path.clone())))
                    } else {
                        Some(Message::Export(ExportMessage::PanelOpen(r.path.clone())))
                    };
                    widgets::icon_button_maybe(Icon::Clapperboard, t!(lang, "replays-export"), msg, STANDARD_PADDING)
                },
                widgets::icon_button(
                    Icon::FolderOpen,
                    t!(lang, "patches-open-folder"),
                    Message::OpenFolder(r.path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),),
                    STANDARD_PADDING,
                ),
                // Watch is the main action of the detail view —
                // promote to primary with a text label so it's
                // visually obvious. Disabled while netplay is in any
                // non-Idle phase: starting a playback session would
                // race with the live emulator. Also disabled when the
                // local-side ROM isn't scanned (playback can't build
                // a core without it); a tooltip carries the reason in
                // that case, since the label alone can't say why.
                {
                    let watch_disabled = netplay_active || !local_rom_present;
                    let btn = widgets::labeled_icon_button_maybe(
                        Icon::Play,
                        t!(lang, "replays-watch"),
                        if watch_disabled {
                            None
                        } else {
                            Some(Message::Watch(r.path.clone()))
                        },
                        STANDARD_PADDING,
                        if watch_disabled {
                            widgets::neutral
                        } else {
                            widgets::primary_button
                        },
                    );
                    if local_rom_present {
                        btn
                    } else {
                        iced::widget::tooltip(
                            btn,
                            widgets::tooltip_bubble(t!(lang, "replays-watch-missing-rom")),
                            iced::widget::tooltip::Position::Top,
                        )
                        .gap(4)
                        .into()
                    }
                },
            ]
            .spacing(6)
            // Top-align so the action buttons stay anchored when
            // a long title wraps to a second line.
            .align_y(Alignment::Start),
            // Metadata rows: file path, timestamp, match type,
            // duration. Stacked tight in a sub-column so the rows
            // read as one block (matches the patches detail-card
            // density at .spacing(3)), with the outer column still
            // breathing at .spacing(6) between sections.
            column![
                text(format!("{parent_str}{filename}"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
                text(ts_str).size(TEXT_CAPTION).style(widgets::muted_text_style),
                {
                    let family = md
                        .local_side
                        .as_ref()
                        .and_then(|s| s.game_info.as_ref())
                        .map(|g| g.rom_family.clone())
                        .unwrap_or_default();
                    let type_name =
                        crate::game::match_type_name(lang, &family, md.match_type as u8, md.match_subtype as u8);
                    // "Triple (2 rounds)" once the lazy stats
                    // worker gets here; just "Triple" until then,
                    // so the row doesn't pop when the count loads.
                    let value = if let Some(s) = state.stats.get(&r.path) {
                        let rounds = t!(lang, "replays-round-count", count = s.round_count as i64);
                        format!("{type_name} · {rounds}")
                    } else {
                        type_name
                    };
                    row![
                        text(t!(lang, "replays-match-type"))
                            .size(TEXT_CAPTION)
                            .style(widgets::muted_text_style),
                        text(value).size(TEXT_CAPTION),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center)
                },
                // Duration row, matching the match-type styling.
                // Value fills in once the lazy stats worker has
                // tallied the tick count for this path; em-dash
                // placeholder until then so the row doesn't pop
                // into existence.
                row![
                    text(t!(lang, "replays-duration"))
                        .size(TEXT_CAPTION)
                        .style(widgets::muted_text_style),
                    text(
                        state
                            .stats
                            .get(&r.path)
                            .map(|s| format_duration(s.tick_count))
                            .unwrap_or_else(|| "—".to_string())
                    )
                    .size(TEXT_CAPTION),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            ]
            .spacing(3),
            export::export_panel(
                lang,
                state.is_panel_open(&r.path),
                &state.export_settings,
                state.rounds_for(&r.path),
                state.job(&r.path),
                &r.path,
            ),
        ]
        .spacing(6),
    )
    .width(Fill)
    .padding(style::PANE_PADDING)
    .style(widgets::pane);

    // Matchup pane: you-vs-opponent cards with a wide gap. The
    // diagonal cut + red/blue halves + VS badge are painted by
    // `widgets::vs_splitter`, layered *under* the row so the
    // cards sit on top of the colored plate.
    let matchup_row = row![
        row_for_side(t!(lang, "play-you"), md.local_side.as_ref()),
        row_for_side(t!(lang, "play-opponent"), md.remote_side.as_ref()),
    ]
    .spacing(56)
    .align_y(iced::Alignment::Start)
    .height(Length::Shrink);
    let matchup_pane = container(
        iced::widget::Stack::new()
            .push(container(matchup_row).padding(style::PANE_PADDING).width(Fill))
            .push_under(widgets::vs_splitter()),
    )
    .width(Fill)
    .style(widgets::pane);

    // Save view contributes its own pane pair (tab strip + body)
    // when a save is loaded; otherwise a single placeholder pane
    // explaining the empty state.
    let preview: Element<'_, Message> = if let Some(loaded) = state.loaded.as_ref() {
        save_view::view(lang, loaded, &state.save_view, false, None, true, false).map(Message::SaveViewAction)
    } else {
        container(
            text(t!(lang, "save-empty"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
        )
        .padding(style::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane)
        .into()
    };

    column![title_pane, matchup_pane, preview]
        .spacing(style::PANE_GAP)
        .width(Fill)
        .height(Fill)
        .into()
}

/// "Mega Man Battle Network 6" — family-only i18n lookup, matching
/// how the lobby renders the game line. Falls back to "{family}
/// v{variant}" for unrecognized families.
fn family_display_name(lang: &LanguageIdentifier, family: &str, variant: u32) -> String {
    crate::game::family_str(family, lang, "name").unwrap_or_else(|| format!("{family} v{variant}"))
}

/// `tick_count` → `"M:SS"` (or `"H:MM:SS"` past an hour). 60
/// ticks = 1 second at GBA native rate; replay export uses the
/// same constant.
fn format_duration(ticks: u32) -> String {
    let secs = ticks / 60;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Display label for the `link_code` field of a replay's
/// metadata. Direct-TCP sessions leave the field blank in the
/// metadata (see `netplay::take_pre_match`); render those as a
/// localized "(direct)" marker so the row / detail panel still
/// has something where the link code would be.
fn link_code_display<'a>(lang: &LanguageIdentifier, code: &'a str) -> std::borrow::Cow<'a, str> {
    if code.is_empty() {
        std::borrow::Cow::Owned(t!(lang, "replays-direct-marker"))
    } else {
        std::borrow::Cow::Borrowed(code)
    }
}

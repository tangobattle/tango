use crate::i18n::t;
use crate::widgets;
use lucide_icons::Icon;
use crate::{
    config, replays, save_view, Scanners, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_HEADING,
};
use iced::widget::rule::{horizontal as horizontal_rule, vertical as vertical_rule};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone)]
pub enum Message {
    /// Picked a game from the Game filter dropdown. `None` =
    /// "All games".
    GameFilterSelected(GameFilterOption),
    /// Typed in the opponent-filter text input. Empty = no
    /// filter; otherwise a substring (case-insensitive) match
    /// against the remote side's nickname.
    OpponentFilterChanged(String),
    Selected(std::path::PathBuf),
    OpenFolder(std::path::PathBuf),
    Watch(std::path::PathBuf),
    /// User clicked Save As in the export panel. App opens an
    /// async file dialog and, on result, dispatches `ExportStart`.
    Export(std::path::PathBuf),
    /// Internal: file dialog returned. Carries the source replay
    /// path + the user-picked output path. App spawns the actual
    /// export task in this handler.
    ExportStart {
        replay: std::path::PathBuf,
        output: std::path::PathBuf,
    },
    /// Progress tick from the running export task: (completed,
    /// total) frame pairs. Includes the source replay path so the
    /// detail view can decide whether to render its status line.
    ExportProgress {
        replay: std::path::PathBuf,
        completed: usize,
        total: usize,
    },
    /// Export task completed. Carries the output path on success
    /// or an error description on failure. Same replay-scoping as
    /// `ExportProgress`.
    ExportFinished {
        replay: std::path::PathBuf,
        result: Result<std::path::PathBuf, String>,
    },
    /// User dismissed the post-export status line.
    /// Dismiss a finished (or failed) export job from the per-
    /// replay job map. Path identifies which job to drop so the
    /// detail panel can offer a per-replay close button.
    ExportDismiss(std::path::PathBuf),
    /// Open the rendered video with the OS's default handler.
    OpenFile(std::path::PathBuf),
    /// Export settings widgets — scale, lossless, disable-BGM.
    SetExportScale(u8),
    SetExportLossless(bool),
    SetExportDisableBgm(bool),
    /// Toggle the Nth round in `selected_rounds`.
    ToggleExportRound(usize, bool),
    /// Open / close the inline export-options panel. Distinct
    /// from `Export(_)` (which actually triggers the export).
    ExportPanelOpen(std::path::PathBuf),
    ExportPanelClose,
    Rescan,
    SaveViewAction(save_view::Action),
}

/// Per-replay export state. Lives in a HashMap keyed by replay
/// path so multiple renders can run concurrently — the sidebar
/// spinner + detail panel both look up by path. `result` flips to
/// `Some` when the export task finishes; until the user dismisses
/// it, the job stays in the map so the detail panel can show the
/// success/failure line.
#[derive(Debug, Clone)]
pub struct ExportJob {
    pub completed: usize,
    pub total: usize,
    pub result: Option<Result<std::path::PathBuf, String>>,
}

pub type ExportJobs = std::collections::HashMap<std::path::PathBuf, ExportJob>;

/// User-tunable settings the export form passes to
/// `tango_pvp::replay::export::export(...)`. Defaults match the
/// legacy replay-dump window.
#[derive(Clone, Copy, Debug)]
pub struct ExportSettings {
    pub scale: u8,
    pub lossless: bool,
    pub disable_bgm: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            scale: 5,
            lossless: false,
            disable_bgm: false,
        }
    }
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
    /// the matchmaking unit.
    pub game_filter: Option<String>,
    /// Substring (case-insensitive) match against the remote
    /// side's nickname. Empty = no filter.
    pub opponent_filter: String,
    pub selected: Option<std::path::PathBuf>,
    /// Cached Loaded for the currently-selected replay's local side.
    /// Rebuilt by the App's `Selected` handler; view borrows read-only.
    pub loaded: Option<crate::selection::Loaded>,
    /// Path the cached `loaded` was built for. Used to invalidate the
    /// cache when the selection changes.
    pub loaded_cache_path: Option<std::path::PathBuf>,
    pub save_view: save_view::State,
    pub export_jobs: ExportJobs,
    pub export_settings: ExportSettings,
    /// Per-round include/exclude mask for the currently-selected
    /// replay's export. Repopulated whenever `loaded_cache_path`
    /// is refreshed. Empty until a replay decodes successfully.
    pub selected_rounds: Vec<bool>,
    /// Inline export-options panel visibility. Toggled on by the
    /// Export button + off by Cancel; the panel itself contains
    /// the actual Save As… button that kicks off the export. Auto-
    /// closes once an export starts (the in-flight status replaces
    /// it visually).
    pub export_panel_open: bool,
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
    /// `Message::ExportStart`.
    OpenExportSaveDialog(std::path::PathBuf),
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
}

impl ReplaysState {
    /// Apply a tab message. Pure UI-state mutations happen
    /// in-place; anything that needs the App's collaborators
    /// (clipboard, file dialog, session host, …) is bubbled up
    /// as a single optional [`Effect`].
    pub fn update(&mut self, msg: Message, scanners: &Scanners, config: &config::Config) -> Option<Effect> {
        match msg {
            Message::GameFilterSelected(o) => {
                self.game_filter = o.pair;
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
            Message::Selected(p) => {
                self.selected = Some(p);
                self.refresh_loaded(scanners, config);
                None
            }
            Message::OpenFolder(p) => Some(Effect::OpenPath(p)),
            Message::Watch(p) => Some(Effect::Watch(p)),
            Message::Rescan => Some(Effect::Rescan),
            Message::SaveViewAction(action) => {
                self.save_view.apply(&action);
                let loaded = self.loaded.as_ref()?;
                match action {
                    save_view::Action::CopyTab(tab) => {
                        save_view::tab_as_text(&config.language, tab, loaded).map(Effect::CopyText)
                    }
                    save_view::Action::CopyTabImage(tab) => save_view::tab_as_image(tab, loaded).map(Effect::CopyImage),
                    _ => None,
                }
            }
            Message::Export(replay_path) => Some(Effect::OpenExportSaveDialog(replay_path)),
            Message::ExportStart { replay, output } => {
                // Snapshot the form + round mask exactly as the
                // user has it right now. Disabling the form
                // widgets while in-flight is the lock — no need
                // to re-read state when progress messages arrive.
                let settings = self.export_settings;
                let mut rounds = self.selected_rounds.clone();
                if rounds.is_empty() {
                    // Single-round replays don't show the rounds
                    // selector at all, so this guards the "user
                    // hit Save As before any rounds were
                    // computed" race.
                    rounds = vec![true];
                }
                self.export_jobs.insert(
                    replay.clone(),
                    ExportJob { completed: 0, total: 0, result: None },
                );
                // Leave the panel open — its body switches to a
                // progress bar (and then to the Open / Reset
                // actions when done) so the user stays anchored
                // to the same surface across the whole render.
                Some(Effect::StartExport {
                    replay,
                    output,
                    settings,
                    rounds,
                })
            }
            Message::ExportProgress {
                replay,
                completed,
                total,
            } => {
                if let Some(job) = self.export_jobs.get_mut(&replay) {
                    if job.result.is_none() {
                        job.completed = completed;
                        job.total = total;
                    }
                }
                None
            }
            Message::ExportFinished { replay, result } => {
                self.export_jobs
                    .entry(replay)
                    .or_insert_with(|| ExportJob { completed: 0, total: 0, result: None })
                    .result = Some(result);
                None
            }
            Message::ExportDismiss(p) => {
                self.export_jobs.remove(&p);
                None
            }
            Message::OpenFile(p) => Some(Effect::OpenPath(p)),
            Message::SetExportScale(s) => {
                self.export_settings.scale = s.clamp(1, 10);
                None
            }
            Message::SetExportLossless(b) => {
                self.export_settings.lossless = b;
                None
            }
            Message::SetExportDisableBgm(b) => {
                self.export_settings.disable_bgm = b;
                None
            }
            Message::ToggleExportRound(idx, picked) => {
                if let Some(slot) = self.selected_rounds.get_mut(idx) {
                    *slot = picked;
                }
                None
            }
            Message::ExportPanelOpen(p) => {
                self.export_panel_open = true;
                // Stale Done status from a previous export of this
                // same replay clutters the panel; reset for a fresh
                // start. Other replays' jobs stay untouched.
                self.export_jobs.remove(&p);
                None
            }
            Message::ExportPanelClose => {
                self.export_panel_open = false;
                None
            }
        }
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.loaded = None;
        self.loaded_cache_path = None;
        self.selected_rounds.clear();
    }

    /// Decode the currently-selected replay just enough to build
    /// its save-view Loaded + populate the round count for the
    /// export form. Cached against the selected path so this only
    /// re-runs on selection change.
    fn refresh_loaded(&mut self, scanners: &Scanners, config: &config::Config) {
        let Some(path) = self.selected.clone() else {
            self.loaded = None;
            self.loaded_cache_path = None;
            self.selected_rounds.clear();
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
                self.loaded_cache_path = Some(path);
                // Default to all-rounds-checked on every fresh
                // selection; export form reads this snapshot.
                self.selected_rounds = vec![true; rounds];
            }
            Err(e) => {
                log::warn!("replay save preview failed: {e}");
                self.loaded = None;
                self.loaded_cache_path = None;
                self.selected_rounds.clear();
            }
        }
    }

    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        config: &'a config::Config,
    ) -> Element<'a, Message> {
        let replays_path = config.replays_path();
        let replays = scanners.replays.read();

        // Top: game + opponent filter dropdowns. Options are
        // derived from the distinct values seen across the
        // scanned replays' local/remote metadata; "All …" is
        // always the first option.
        let all_games = t(lang, "replays-filter-all-games");
        let mut game_options = vec![GameFilterOption::all(all_games.clone())];
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
                // Use any known variant of the family for the
                // short-name lookup; the result ("BN6") is
                // variant-independent. Fall back to the raw
                // family string for unrecognized families.
                let display = tango_gamedb::find_by_family_and_variant(&family, 0)
                    .or_else(|| tango_gamedb::find_by_family_and_variant(&family, 1))
                    .map(|g| crate::game::short_name(lang, g))
                    .unwrap_or_else(|| family.clone());
                game_options.push(GameFilterOption {
                    pair: Some(family),
                    display,
                });
            }
        }
        let selected_game = game_options
            .iter()
            .find(|o| o.pair == self.game_filter)
            .cloned()
            .unwrap_or_else(|| game_options[0].clone());
        let top = container(
            row![
                text(format!("{}:", t(lang, "replays-filter-game"))).size(TEXT_CAPTION),
                pick_list(game_options, Some(selected_game), Message::GameFilterSelected)
                    
                    .padding(STANDARD_PADDING),
                text(format!("{}:", t(lang, "replays-filter-opponent"))).size(TEXT_CAPTION),
                iced::widget::text_input(&t(lang, "replays-filter-opponent-placeholder"), &self.opponent_filter,)
                    .on_input(Message::OpponentFilterChanged)
                    .padding(STANDARD_PADDING)
                    
                    .width(Length::Fixed(180.0)),
                horizontal_space(),
                widgets::icon_button(
                    Icon::RefreshCw,
                    t(lang, "rescan"),
                    Message::Rescan,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding(8),
        )
        .width(Fill);

        // Left list — AND of game + opponent filters. Opponent
        // match is case-insensitive substring (mirrors the
        // text-input UX).
        let game_filter = self.game_filter.as_ref();
        let opp_needle = self.opponent_filter.trim().to_lowercase();
        let filtered: Vec<&replays::ScannedReplay> = replays
            .iter()
            .filter(|r| {
                let g_ok = game_filter
                    .map(|family| {
                        r.metadata
                            .local_side
                            .as_ref()
                            .and_then(|s| s.game_info.as_ref())
                            .map(|gi| gi.rom_family == *family)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true);
                let o_ok = if opp_needle.is_empty() {
                    true
                } else {
                    r.metadata
                        .remote_side
                        .as_ref()
                        .map(|s| s.nickname.to_lowercase().contains(&opp_needle))
                        .unwrap_or(false)
                };
                g_ok && o_ok
            })
            .collect();

        let mut list = column![].spacing(1).padding(8);
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

            let local_gi = md.local_side.as_ref().and_then(|s| s.game_info.as_ref());
            let game_label = local_gi
                .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
                .and_then(|(family, variant)| tango_gamedb::find_by_family_and_variant(family, variant))
                .map(|g| crate::game::short_name(lang, g))
                .or_else(|| local_gi.map(|g| g.rom_family.clone()))
                .unwrap_or_default();
            let nick_pair = if remote_nick.is_empty() && local_nick.is_empty() {
                md.link_code.clone()
            } else {
                format!("{local_nick} vs {remote_nick}")
            };

            let selected = self.selected.as_ref() == Some(&r.path);
            // Show a render-in-progress glyph for replays whose
            // export job is still running. Multiple renders can
            // run at once now, so this is the only way to see
            // background progress without selecting each replay.
            let job_state = self.export_jobs.get(&r.path);
            let rendering = matches!(job_state, Some(j) if j.result.is_none());
            let render_done_ok = matches!(job_state, Some(j) if matches!(&j.result, Some(Ok(_))));
            let render_done_err = matches!(job_state, Some(j) if matches!(&j.result, Some(Err(_))));
            let badge: Element<'_, Message> = if rendering {
                container(Icon::Clapperboard.widget().style(|theme: &iced::Theme| {
                    iced::widget::text::Style { color: Some(theme.palette().primary) }
                }))
                .padding([0, 4])
                .into()
            } else if render_done_ok {
                container(Icon::Check.widget().style(|theme: &iced::Theme| {
                    iced::widget::text::Style { color: Some(theme.palette().success) }
                }))
                .padding([0, 4])
                .into()
            } else if render_done_err {
                container(Icon::X.widget().style(|theme: &iced::Theme| {
                    iced::widget::text::Style { color: Some(theme.palette().danger) }
                }))
                .padding([0, 4])
                .into()
            } else {
                Space::new().width(Length::Fixed(0.0)).into()
            };
            list = list.push(
                button(
                    row![
                        column![
                            text(ts_str).size(TEXT_BODY),
                            // Selected → inherit the button's foreground
                            // (iced picks one readable on the primary-
                            // weak background). Unselected → muted gray.
                            text(format!("{game_label} @ {}  ·  {nick_pair}", md.link_code))
                                .size(TEXT_CAPTION)
                                .style(move |theme: &iced::Theme| if selected {
                                    iced::widget::text::Style { color: None }
                                } else {
                                    save_view::muted_text_style(theme)
                                }),
                        ]
                        .spacing(2)
                        .width(Fill),
                        badge,
                    ]
                    .spacing(0)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Fill)
                .style(widgets::list_item(selected))
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
                container(text(t(lang, "replays-select-prompt")).size(TEXT_BODY))
                    .center(Fill)
                    .into()
            }
        } else {
            container(text(t(lang, "replays-select-prompt")).size(TEXT_BODY))
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
    r: &replays::ScannedReplay,
    replays_path: &std::path::Path,
    state: &'a ReplaysState,
) -> Element<'a, Message> {
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
        // Family short name ("BN6"), not the long variant string
        // — the per-side card is tight and the family identifies
        // the matchup uniquely enough for a replay listing.
        let game = gi
            .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
            .and_then(|(family, variant)| tango_gamedb::find_by_family_and_variant(family, variant))
            .map(|g| crate::game::short_name(lang, g))
            .or_else(|| gi.map(|g| g.rom_family.clone()))
            .unwrap_or_default();
        let patch = gi
            .and_then(|g| g.patch.as_ref())
            .map(|p| format!("{} v{}", p.name, p.version));
        let mut col = column![
            text(label).size(TEXT_CAPTION).style(save_view::muted_text_style),
            text(nick).size(TEXT_HEADING),
            text(game).size(TEXT_CAPTION),
        ]
        .spacing(2);
        if let Some(p) = patch {
            col = col.push(
                text(p)
                    .size(TEXT_CAPTION)
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

    // Title uses the local side's game short tag (BN6 / EXE5K / ...)
    // — same shape as the list rows for at-a-glance recognition.
    let game_short = md
        .local_side
        .as_ref()
        .and_then(|s| s.game_info.as_ref())
        .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
        .and_then(|(family, variant)| tango_gamedb::find_by_family_and_variant(family, variant))
        .map(|g| crate::game::short_name(lang, g))
        .unwrap_or_else(|| "?".to_string());
    let title = format!("{game_short} @ {}", md.link_code);

    // Embedded save view for the local side. App fills `state.loaded`
    // when a replay is selected; until then (or if the parse fails)
    // we show a stub. No outer scrollable — save_view manages its own
    // per-tab scrolling (Folder list etc.). Wrapping again in a
    // scrollable here made Fill-height children inside save_view
    // think they had infinite vertical room, producing a meter-tall
    // scrollbar.
    let preview: Element<'_, Message> = if let Some(loaded) = state.loaded.as_ref() {
        // No outer padding: save_view brings its own tab strip +
        // body insets, and the extra 8 px container padding made
        // the embedded view feel awkwardly hemmed-in against the
        // replay-detail card it sits inside.
        container(save_view::view(lang, loaded, &state.save_view, false).map(Message::SaveViewAction))
            .height(Fill)
            .into()
    } else {
        container(
            text(t(lang, "save-empty"))
                .size(TEXT_CAPTION)
                .style(save_view::muted_text_style),
        )
        .padding(8)
        .into()
    };

    container(
        column![
            row![
                text(title).size(18),
                horizontal_space(),
                // Watch is the main action of the detail view —
                // promote to primary so it's visually obvious.
                widgets::icon_button_styled(
                    Icon::Play,
                    t(lang, "replays-watch"),
                    Some(Message::Watch(r.path.clone())),
                    STANDARD_PADDING,
                    iced::widget::button::primary,
                ),
                widgets::icon_button(
                    Icon::Clapperboard,
                    t(lang, "replays-export"),
                    // Plain toggle. The panel now stays open
                    // across the whole render lifecycle (form →
                    // progress bar → Open / Reset), so the only
                    // job here is showing or hiding the surface.
                    if state.export_panel_open {
                        Message::ExportPanelClose
                    } else {
                        Message::ExportPanelOpen(r.path.clone())
                    },
                    STANDARD_PADDING,
                ),
                widgets::icon_button(
                    Icon::Folder,
                    t(lang, "patches-open-folder"),
                    Message::OpenFolder(r.path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),),
                    STANDARD_PADDING,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            export_panel(
                lang,
                state.export_panel_open,
                &state.export_settings,
                &state.selected_rounds,
                state.export_jobs.get(&r.path),
                &r.path,
            ),
            text(ts_str).size(TEXT_CAPTION).style(save_view::muted_text_style),
            text(format!("{parent_str}{filename}"))
                .size(TEXT_CAPTION)
                .style(save_view::muted_text_style),
            Space::new().height(8),
            horizontal_rule(1),
            Space::new().height(8),
            row![
                row_for_side(t(lang, "play-you"), md.local_side.as_ref()),
                vertical_rule(1),
                row_for_side(t(lang, "replays-opponent"), md.remote_side.as_ref()),
            ]
            .spacing(12)
            .height(Length::Shrink),
            Space::new().height(8),
            text({
                let family = md
                    .local_side
                    .as_ref()
                    .and_then(|s| s.game_info.as_ref())
                    .map(|g| g.rom_family.clone())
                    .unwrap_or_default();
                let label = crate::game::match_type_name(lang, &family, md.match_type as u8, md.match_subtype as u8);
                format!("{}: {}", t(lang, "replays-match-type"), label)
            })
            .size(TEXT_CAPTION),
            Space::new().height(8),
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GameFilterOption {
    /// `None` = "all games" sentinel; otherwise the ROM family
    /// (e.g. "bn6"). Variant is intentionally not part of the
    /// key — the filter groups Gregar + Falzar together.
    pub pair: Option<String>,
    pub display: String,
}
impl GameFilterOption {
    fn all(label: String) -> Self {
        Self {
            pair: None,
            display: label,
        }
    }
}
impl std::fmt::Display for GameFilterOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

/// Inline export panel. Three-state body — the chrome (border +
/// padding) stays put across all of them so the user remains
/// anchored to the same surface during a render:
///
///   * No job: the form (scale / lossless / mute / round mask +
///     Save As…).
///   * In-flight job: progress bar + percentage.
///   * Finished job: success/error line + Open Replay + Reset
///     (Reset clears the job → form returns).
///
/// While `open` is false the panel collapses to a zero-height
/// element so the detail layout reflows around it.
fn export_panel<'a>(
    lang: &'a LanguageIdentifier,
    open: bool,
    settings: &'a ExportSettings,
    selected_rounds: &'a [bool],
    job: Option<&'a ExportJob>,
    replay_path: &std::path::Path,
) -> Element<'a, Message> {
    if !open {
        return Space::new().height(0).into();
    }
    // In-flight + finished states wrap a tighter body — render
    // them first so the rest of the fn deals only with the form.
    if let Some(job) = job {
        let body: Element<'a, Message> = match &job.result {
            None => {
                let pct = if job.total > 0 {
                    (job.completed as f32 / job.total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let pct_label = format!("{}%", (pct * 100.0).round() as u32);
                column![
                    text(t(lang, "replays-export-progress"))
                        .size(TEXT_CAPTION)
                        .style(save_view::muted_text_style),
                    iced::widget::progress_bar(0.0..=1.0, pct).length(Length::Fixed(8.0)),
                    text(pct_label).size(TEXT_CAPTION).style(save_view::muted_text_style),
                ]
                .spacing(4)
                .into()
            }
            Some(Ok(path)) => {
                let path_for_open = path.clone();
                column![
                    text(format!("{}:", t(lang, "replays-export-success")))
                        .size(TEXT_CAPTION)
                        .style(save_view::success_text_style),
                    text(path.display().to_string()).size(TEXT_CAPTION),
                    row![
                        widgets::labeled_icon_button(
                            Icon::Play,
                            t(lang, "replays-export-open"),
                            Message::OpenFile(path_for_open),
                            STANDARD_PADDING,
                            iced::widget::button::primary,
                        ),
                        widgets::labeled_icon_button(
                            Icon::RefreshCw,
                            t(lang, "replays-export-reset"),
                            Message::ExportDismiss(replay_path.to_path_buf()),
                            STANDARD_PADDING,
                            widgets::neutral,
                        ),
                    ]
                    .spacing(8),
                ]
                .spacing(6)
                .into()
            }
            Some(Err(e)) => column![
                text(format!("{}: {e}", t(lang, "replays-export-error")))
                    .size(TEXT_CAPTION)
                    .style(save_view::danger_text_style),
                widgets::labeled_icon_button(
                    Icon::RefreshCw,
                    t(lang, "replays-export-reset"),
                    Message::ExportDismiss(replay_path.to_path_buf()),
                    STANDARD_PADDING,
                    widgets::neutral,
                ),
            ]
            .spacing(6)
            .into(),
        };
        return container(column![body].padding(12))
            .width(Fill)
            .style(iced::widget::container::bordered_box)
            .into();
    }
    // Form path — `in_flight` stays false because the panel only
    // reaches this branch when there's no job at all.
    let in_flight = false;
    let scale_label = text(format!(
        "{}: {}",
        t(lang, "replays-export-scale"),
        if settings.lossless {
            "".into()
        } else {
            format!("{}×", settings.scale)
        }
    ))
    .size(TEXT_CAPTION)
    .style(save_view::muted_text_style);
    // Scale isn't used when lossless (libx264rgb -qp 0 ignores
    // the swscale neighbor filter that would carry the factor);
    // hide the slider in that case to make the disabled state
    // visually unambiguous. iced 0.14 has no `slider.enabled()`,
    // so we just swap the widget.
    let scale_slider: Element<'a, Message> = if settings.lossless {
        text(t(lang, "replays-export-scale-na-lossless"))
            .size(TEXT_CAPTION)
            .width(Length::Fixed(140.0))
            .style(save_view::muted_text_style)
            .into()
    } else {
        iced::widget::slider(1..=10u8, settings.scale, Message::SetExportScale)
            .width(Length::Fixed(140.0))
            .into()
    };
    let lossless_chk = iced::widget::checkbox(settings.lossless)
        .label(t(lang, "replays-export-lossless"))
        ;
    let lossless_chk: Element<'a, Message> = if in_flight {
        lossless_chk.into()
    } else {
        lossless_chk.on_toggle(Message::SetExportLossless).into()
    };
    let bgm_chk = iced::widget::checkbox(settings.disable_bgm)
        .label(t(lang, "replays-export-disable-bgm"))
        ;
    let bgm_chk: Element<'a, Message> = if in_flight {
        bgm_chk.into()
    } else {
        bgm_chk.on_toggle(Message::SetExportDisableBgm).into()
    };
    // Round-selection row — only shown for multi-round replays
    // since a single-round replay's "rounds" selector is pointless.
    let mut col = column![
        row![column![scale_label, scale_slider].spacing(2), lossless_chk, bgm_chk,]
            .spacing(16)
            .align_y(Alignment::Center)
    ]
    .spacing(6);
    if selected_rounds.len() > 1 {
        let label = text(format!("{}:", t(lang, "replays-export-rounds")))
            .size(TEXT_CAPTION)
            .style(save_view::muted_text_style);
        let mut rounds_row = row![label].spacing(6).align_y(Alignment::Center);
        for (i, picked) in selected_rounds.iter().enumerate() {
            let cb = iced::widget::checkbox(*picked)
                .label(format!("{}", i + 1))
                ;
            let cb: Element<'a, Message> = if in_flight {
                cb.into()
            } else {
                cb.on_toggle(move |v| Message::ToggleExportRound(i, v)).into()
            };
            rounds_row = rounds_row.push(cb);
        }
        col = col.push(rounds_row);
    }
    // Action row: Save As… commits + Cancel closes the panel.
    // Save As… is disabled if every round is unchecked.
    let any_round = selected_rounds.is_empty() || selected_rounds.iter().any(|b| *b);
    // Labeled "Save As…" button — text + icon together so it reads
    // as a real call-to-action rather than a bare check-mark glyph.
    // Disabled (no on_press) when nothing is selected for export.
    let save_as_btn: Element<'a, Message> = if any_round {
        widgets::labeled_icon_button(
            Icon::Upload,
            t(lang, "replays-export-save-as"),
            Message::Export(replay_path.to_path_buf()),
            STANDARD_PADDING,
            iced::widget::button::primary,
        )
    } else {
        // No labeled_icon_button_maybe helper, so build the disabled
        // variant inline.
        iced::widget::button(
            iced::widget::row![
                Icon::Upload.widget(),
                text(t(lang, "replays-export-save-as")),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(STANDARD_PADDING)
        .style(widgets::neutral)
        .into()
    };
    let actions = row![horizontal_space(), save_as_btn]
        .spacing(6)
        .align_y(Alignment::Center);
    col = col.push(actions);

    container(col.padding(12))
        .width(Fill)
        .style(iced::widget::container::bordered_box)
        .into()
}

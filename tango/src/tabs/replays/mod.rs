use crate::app::Scanners;
use crate::i18n::t;
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::widgets;
use crate::{config, replays, save_view};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, row, text_input};
use unic_langid::LanguageIdentifier;

mod export;
pub use export::{ExportJob, ExportMessage, ExportSettings, PerReplay};

#[derive(Debug, Clone)]
pub enum Message {
    /// Picked a game from the Game filter dropdown. `None` =
    /// "All games".
    /// `None` = "all games"; otherwise the ROM family (e.g. "bn6").
    GameFilterSelected(Option<String>),
    /// Picked a recency window from the Date filter dropdown.
    DateFilterSelected(DateFilter),
    /// Typed in the search text input. Empty = no filter;
    /// otherwise whitespace-separated terms, ANDed, each matched
    /// case-insensitively against the replay's metadata (nicknames,
    /// link code, game, patch, date, file path) — see
    /// [`search_haystack`].
    SearchChanged(String),
    /// User toggled the "show incomplete" checkbox in the top
    /// filter row. Off by default — incomplete replays (the
    /// recorded stream didn't reach `END_OF_REPLAY`) are hidden
    /// from the list so the default view shows finished matches
    /// only.
    ShowIncompleteToggled(bool),
    Selected(std::path::PathBuf),
    /// Reveal the replay file in the OS file manager, selected.
    RevealReplay(std::path::PathBuf),
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
    /// An [`Effect::AnalyzeReplay`] re-simulation reporting a throttled
    /// partial result, rendered as a live chart that draws itself in
    /// while the analysis runs.
    HpStatsPartial(std::path::PathBuf, tango_pvp::analysis::MatchStats),
    /// An [`Effect::AnalyzeReplay`] re-simulation finished. `None` =
    /// analysis failed (missing ROM, undecodable) — clears the pending
    /// marker so a later re-focus can retry (e.g. after the user
    /// installs the ROM).
    HpStatsLoaded(std::path::PathBuf, Option<tango_pvp::analysis::MatchStats>),
    SaveViewAction(save_view::Action),
    /// Used by Tasks that need a Message to return but want no
    /// state mutation. Currently: the user dismissed the Save As
    /// file dialog without picking a path — the export form should
    /// stay open and untouched.
    NoOp,
}

/// Date-dropdown filter: the replay's timestamp must fall within
/// the window ending now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateFilter {
    #[default]
    Any,
    PastDay,
    PastWeek,
    PastMonth,
    PastYear,
}

impl DateFilter {
    fn matches(self, ts_ms: u64) -> bool {
        let window_secs: u64 = match self {
            DateFilter::Any => return true,
            DateFilter::PastDay => 60 * 60 * 24,
            DateFilter::PastWeek => 60 * 60 * 24 * 7,
            DateFilter::PastMonth => 60 * 60 * 24 * 30,
            DateFilter::PastYear => 60 * 60 * 24 * 365,
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        ts_ms >= now_ms.saturating_sub(window_secs * 1000)
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
    /// Recency window the replays' timestamp must fall in.
    pub date_filter: DateFilter,
    /// Free-text search over the replays' metadata. Empty = no
    /// filter. Split on whitespace into terms; a replay matches
    /// when every term appears somewhere in its haystack (see
    /// [`search_haystack`]).
    pub search: String,
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
    /// Recorded input-pair count per round of the selected replay, from
    /// the same decode that builds `loaded` — the planned segment widths
    /// a live analysis renders into (see [`widgets::cook_hp_rounds`]).
    pub loaded_round_ticks: Vec<u32>,
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
    /// Normalized per-round HP charts keyed by replay path, built on
    /// focus from the replay's `.stats` sidecar when one exists (written
    /// at match teardown, or by an earlier focus) and re-simulated
    /// otherwise — see [`Effect::AnalyzeReplay`]. The detail panel draws
    /// its HP pane from this; paths without an entry just don't get one.
    pub hp_charts: std::collections::HashMap<std::path::PathBuf, HpChart>,
    /// Replays with an analysis in flight — presence stops a re-focus
    /// from stacking a second multi-second re-simulation.
    pub hp_pending: std::collections::HashSet<std::path::PathBuf>,
    /// Entrance restarted when a different replay is selected —
    /// the detail panel slides in from the right.
    pub detail_enter: crate::anim::Enter,
}

/// A replay's match stats, cooked for drawing (see
/// [`widgets::cook_hp_rounds`]). Built once per replay when its
/// [`tango_pvp::analysis::MatchStats`] arrive.
pub struct HpChart {
    pub rounds: Vec<widgets::CookedHpRound>,
    /// The match-wide HP scale the traces were normalized against — the
    /// chart's hover readout multiplies back through it.
    pub max_hp: f32,
}

impl HpChart {
    fn new(
        stats: &tango_pvp::analysis::MatchStats,
        loaded: Option<&crate::selection::Loaded>,
        planned: Option<&[u32]>,
    ) -> Self {
        // Both sides' chip ids resolve through the LOCAL side's chip
        // table (`"???"`/no icon when the ROM/patch wasn't loadable) —
        // right for same-version matches, best-effort across
        // versions/patches. bn1 records no chip events at all.
        let (rounds, max_hp) = widgets::cook_hp_rounds(stats, [loaded, loaded], planned);
        Self { rounds, max_hp }
    }
}

/// Side-effects the tab can't perform itself (because they touch
/// the file system, clipboard, session host, or async runtime).
/// `ReplaysState::update` returns at most one of these per
/// dispatch; the App handler interprets it.
#[derive(Debug)]
pub enum Effect {
    /// `open::that(_)` — folder or rendered video.
    OpenPath(std::path::PathBuf),
    /// Reveal in the OS file manager with the file selected.
    RevealPath(std::path::PathBuf),
    /// User clicked Watch on a replay; App spawns the playback
    /// session and stuffs it into `session.active`.
    Watch(std::path::PathBuf),
    /// A focused replay has no stats sidecar — App spawns
    /// `replays::compute_and_cache_match_stats` on a blocking worker
    /// and posts the result back as [`Message::HpStatsLoaded`].
    AnalyzeReplay(std::path::PathBuf),
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
            Message::DateFilterSelected(d) => {
                self.date_filter = d;
                // Same rule as the game dropdown: a coarse filter
                // change drops the selection.
                self.clear_selection();
                None
            }
            Message::SearchChanged(s) => {
                // Don't clear the selection on every keystroke —
                // the user might be refining the search while
                // keeping a replay open. The view simply omits
                // the detail panel when the selected path no
                // longer matches the current filtered list.
                self.search = s;
                None
            }
            Message::ShowIncompleteToggled(v) => {
                // Same rule as the search box: don't clear the
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
                self.selected = Some(p.clone());
                self.refresh_loaded(scanners, config);
                self.sweep_idle_entries();
                // First focus builds the replay's match stats: try the
                // sidecar (cheap; e.g. written at match teardown, or by a
                // previous focus), and only re-simulate when there isn't
                // one. Failures clear `hp_pending` via the result message,
                // so a later focus retries.
                if !self.hp_charts.contains_key(&p) && !self.hp_pending.contains(&p) {
                    if let Some(stats) =
                        crate::replays::load_match_stats(&config.cache_path(), &config.replays_path(), &p)
                    {
                        // `refresh_loaded` above already pointed `loaded` at
                        // this replay, so chip beads get the right names;
                        // the planned frame keeps every chart of this replay
                        // on one layout convention.
                        self.hp_charts
                            .insert(p.clone(), HpChart::new(&stats, self.loaded.as_ref(), Some(&self.loaded_round_ticks)));
                    } else {
                        // Seed an empty chart immediately — segments at
                        // their final widths, ready for the analysis to
                        // draw into. No placeholder state.
                        self.hp_charts.insert(
                            p.clone(),
                            HpChart::new(
                                &tango_pvp::analysis::MatchStats { rounds: vec![] },
                                self.loaded.as_ref(),
                                Some(&self.loaded_round_ticks),
                            ),
                        );
                        self.hp_pending.insert(p.clone());
                        return Some(Effect::AnalyzeReplay(p));
                    }
                }
                None
            }
            Message::RevealReplay(p) => Some(Effect::RevealPath(p)),
            Message::Watch(p) => Some(Effect::Watch(p)),
            Message::SaveViewAction(action) => {
                // Clipboard outcomes need the App's clipboard collaborator
                // — bubble them up as Effects. Anything else gets folded
                // into save_view-internal state and surfaces as a generic
                // SaveViewTask (currently used for the scroll-to-top snap
                // on a tab change). Edit/Play outcomes can't fire here:
                // the replay save view renders read-only.
                let (sv_task, outcome) = self.save_view.apply(&action, &config.language, self.loaded.as_ref());
                match outcome {
                    Some(save_view::Outcome::CopyText(s)) => Some(Effect::CopyText(s)),
                    Some(save_view::Outcome::CopyImage(img)) => Some(Effect::CopyImage(img)),
                    Some(_) => None,
                    None => Some(Effect::SaveViewTask(sv_task.map(Message::SaveViewAction))),
                }
            }
            Message::Export(m) => self.update_export(m),
            Message::StatsLoaded(path, s) => {
                self.stats.insert(path, s);
                None
            }
            Message::HpStatsPartial(path, partial) => {
                // Live preview: the in-flight analysis renders as a growing
                // chart inside the layout fixed by the planned round spans.
                // Selected-only, like `HpStatsLoaded` — chip names resolve
                // through the selected replay's Loaded.
                if self.selected.as_ref() == Some(&path) {
                    self.hp_charts.insert(
                        path,
                        HpChart::new(&partial, self.loaded.as_ref(), Some(&self.loaded_round_ticks)),
                    );
                }
                None
            }
            Message::HpStatsLoaded(path, stats) => {
                self.hp_pending.remove(&path);
                match stats {
                    // Chip beads bake names through the selected replay's
                    // Loaded; if the user has moved on to another replay by
                    // the time the analysis lands, drop the result rather
                    // than resolving through the wrong game's chip table —
                    // the analysis already wrote the sidecar, so
                    // re-selecting rebuilds the chart straight from disk.
                    Some(stats) if self.selected.as_ref() == Some(&path) => {
                        // Keep the planned frame the live preview rendered
                        // into — the completed chart must not reflow it.
                        self.hp_charts
                            .insert(path, HpChart::new(&stats, self.loaded.as_ref(), Some(&self.loaded_round_ticks)));
                    }
                    // Deselected or failed: also drop any live-preview chart
                    // so a later focus rebuilds from the sidecar (or retries
                    // the analysis) instead of showing a stale partial.
                    _ => {
                        self.hp_charts.remove(&path);
                    }
                }
                None
            }
            Message::NoOp => None,
        }
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.loaded = None;
        self.loaded_cache_path = None;
        self.loaded_round_ticks.clear();
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
        let res = (|| -> anyhow::Result<(crate::selection::Loaded, Vec<u32>)> {
            let f = std::fs::File::open(&path)?;
            let replay = tango_pvp::replay::Replay::decode(f)?;
            let round_ticks = replay.rounds.iter().map(|r| r.len() as u32).collect();
            let loaded = crate::selection::Loaded::for_replay_local(scanners, config, &replay)?;
            Ok((loaded, round_ticks))
        })();
        match res {
            Ok((loaded, round_ticks)) => {
                self.loaded = Some(loaded);
                self.loaded_cache_path = Some(path.clone());
                let rounds = round_ticks.len();
                self.loaded_round_ticks = round_ticks;
                // Default to all-rounds-checked on every fresh
                // selection; export form reads this snapshot.
                self.per.entry(path).or_default().rounds = vec![true; rounds];
            }
            Err(e) => {
                log::warn!("replay save preview failed: {e}");
                self.loaded = None;
                self.loaded_cache_path = None;
                self.loaded_round_ticks.clear();
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
    ) -> Element<'a, Message> {
        // Replay playback spawns an emulator session that would
        // conflict with an active netplay session. Disable the
        // Watch button anywhere the netplay phase isn't Idle —
        // user has to disconnect / dismiss the lobby first.
        let netplay_active = !matches!(netplay_phase, crate::netplay::Phase::Idle);
        let replays_path = config.replays_path();
        let replays = scanners.replays.read();

        let top = self.filter_strip(lang, &replays);

        // Left list — AND of game + search + completeness filters.
        let filtered: Vec<&replays::ScannedReplay> = replays
            .iter()
            .filter(|r| self.matches_filters(lang, &replays_path, r))
            .collect();
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
            crate::anim::slide_in_opt(
                detail,
                self.detail_enter.progress(iced::time::Instant::now()),
                iced::Vector::new(0.0, 28.0),
            )
        } else {
            widgets::pane_prompt(t!(lang, "replays-select-prompt"))
        };

        widgets::top_split_pane(top, left, right)
    }

    /// Top strip: game + date filter dropdowns, the free-text
    /// search box, and the show-incomplete toggle. Game options are
    /// derived from the distinct families seen across the scanned
    /// replays; "All …" is always the first option.
    fn filter_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        replays: &[replays::ScannedReplay],
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
        let date_options = vec![
            widgets::Choice::new(DateFilter::Any, t!(lang, "replays-filter-any-time")),
            widgets::Choice::new(DateFilter::PastDay, t!(lang, "replays-filter-past-day")),
            widgets::Choice::new(DateFilter::PastWeek, t!(lang, "replays-filter-past-week")),
            widgets::Choice::new(DateFilter::PastMonth, t!(lang, "replays-filter-past-month")),
            widgets::Choice::new(DateFilter::PastYear, t!(lang, "replays-filter-past-year")),
        ];
        let selected_date = date_options
            .iter()
            .find(|o| o.value == self.date_filter)
            .cloned()
            .unwrap_or_else(|| date_options[0].clone());
        let show_incomplete_toggle = iced::widget::checkbox(self.show_incomplete)
            .on_toggle(Message::ShowIncompleteToggled)
            .label(t!(lang, "replays-show-incomplete"))
            .size(TEXT_BODY)
            .text_size(TEXT_BODY)
            .style(widgets::chunky_checkbox);
        container(
            row![
                widgets::picker(
                    game_options,
                    Some(selected_game),
                    |o: widgets::Choice<Option<String>>| { Message::GameFilterSelected(o.value) }
                ),
                widgets::picker(date_options, Some(selected_date), |o: widgets::Choice<DateFilter>| {
                    Message::DateFilterSelected(o.value)
                }),
                text_input(&t!(lang, "replays-filter-search-placeholder"), &self.search,)
                    .on_input(Message::SearchChanged)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fixed(220.0))
                    .style(widgets::chunky_text_input),
                show_incomplete_toggle,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(style::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane)
        .into()
    }

    /// AND of the game + date + search + completeness filters.
    /// Search splits into whitespace-separated terms, each of which
    /// must appear (case-insensitive) somewhere in the replay's
    /// haystack — see [`search_haystack`]. Completeness only drops a
    /// row once its stats have actually loaded — unloaded entries
    /// pass through so a freshly-scanned replay isn't hidden during
    /// the lazy stats-worker window.
    fn matches_filters(
        &self,
        lang: &LanguageIdentifier,
        replays_path: &std::path::Path,
        r: &replays::ScannedReplay,
    ) -> bool {
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
        let d_ok = self.date_filter.matches(r.metadata.ts);
        let query = self.search.trim().to_lowercase();
        let s_ok = query.is_empty() || {
            // Haystack is only built for a non-empty query, so the
            // idle (no-search) view pays nothing per row.
            let hay = search_haystack(lang, replays_path, r);
            query.split_whitespace().all(|term| hay.contains(term))
        };
        let c_ok = self.show_incomplete || self.stats.get(&r.path).map(|s| s.is_complete).unwrap_or(false);
        g_ok && d_ok && s_ok && c_ok
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

        let ts_str = format_ts(md.ts, "%Y-%m-%d %H:%M:%S");

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
        let status_badge = |icon: Icon, color: fn(&iced::Theme) -> iced::Color| -> Element<'a, Message> {
            container(
                icon.widget()
                    .style(move |theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(color(theme)),
                    }),
            )
            .padding([0, 4])
            .into()
        };
        let badge: Element<'_, Message> = if rendering {
            status_badge(Icon::Clapperboard, |theme| theme.palette().primary)
        } else if render_done_ok {
            status_badge(Icon::Check, |theme| theme.palette().success)
        } else if render_done_err {
            status_badge(Icon::X, |theme| theme.palette().danger)
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
            .style(widgets::list_caption_style(selected)),
        ]
        .spacing(2)
        .width(Fill);
        if let Some(line) = stats_line {
            text_col = text_col.push(text(line).size(TEXT_CAPTION).style(move |theme: &iced::Theme| {
                if !is_complete {
                    widgets::danger_text_style(theme)
                } else {
                    widgets::list_caption_style(selected)(theme)
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
    let ts_str = format_ts(md.ts, "%Y-%m-%d %H:%M:%S %z");

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
                    Message::RevealReplay(r.path.clone()),
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

    let matchup_pane = widgets::matchup_pane(
        row_for_side(t!(lang, "play-you"), md.local_side.as_ref()),
        row_for_side(t!(lang, "play-opponent"), md.remote_side.as_ref()),
    );

    // HP-over-time pane: the match graph, at a fixed height with the
    // chip-event lanes always present. During a first-focus analysis the
    // chart exists from the start (empty segments at their final widths,
    // seeded on selection) and the re-simulation draws into it live — no
    // placeholder state.
    let hp_pane: Element<'_, Message> = {
        // The pane renders whether or not a chart exists yet (a missing
        // entry — e.g. a failed analysis — draws as an empty frame), so
        // the detail column's layout never shifts with analysis state.
        let chart = state.hp_charts.get(&r.path);
        let chart_rounds: Vec<widgets::HpGraphRound<'_>> = chart
            .map(|c| c.rounds.as_slice())
            .unwrap_or(&[])
            .iter()
            .map(|r| widgets::HpGraphRound {
                trace: &r.trace,
                custom: &r.custom,
                chip_uses: [&r.chip_uses[0], &r.chip_uses[1]],
                outcome: r.outcome,
                weight: r.weight,
            })
            .collect();
        let body = widgets::hp_match_graph(
            chart_rounds,
            chart.map(|c| c.max_hp).unwrap_or(1.0),
            1.0,
            DETAIL_HP_GRAPH_H,
            // Zoomable, keyed on the replay path so switching replays
            // resets the view.
            Some({
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                r.path.hash(&mut hasher);
                hasher.finish()
            }),
        );
        // No pane padding: the chart's own per-round inset panels are the
        // content, so the canvas runs edge to edge and the pane background
        // only peeks through the round dividers.
        container(body).width(Fill).style(widgets::pane).into()
    };

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

    let panes = column![title_pane, matchup_pane, hp_pane]
        .spacing(style::PANE_GAP)
        .width(Fill);
    panes.push(preview).height(Fill).into()
}

/// Height of the HP graph in the detail panel: a 54 px trace field plus
/// the widget's two per-side chip-event lanes (18 px, always present),
/// which also leaves room for the four-line icon hover readout — the
/// canvas clips anything that hangs past its bounds. One fixed height
/// for every chart state, so the layout never jerks as an analysis
/// renders in.
const DETAIL_HP_GRAPH_H: f32 = 72.0;

/// Everything the free-text search matches against, joined into one
/// lowercased blob: both sides' nicknames, game names (raw family
/// plus the localized display/short names, so "exe6" and "battle
/// network" both hit), patch name + version, the link code, the
/// date as `YYYY-MM-DD` (so "2026-07" matches a month), and the
/// path relative to the replays root.
fn search_haystack(lang: &LanguageIdentifier, replays_path: &std::path::Path, r: &replays::ScannedReplay) -> String {
    let md = &r.metadata;
    let mut parts: Vec<String> = Vec::new();
    for side in [md.local_side.as_ref(), md.remote_side.as_ref()].into_iter().flatten() {
        parts.push(side.nickname.clone());
        if let Some(gi) = side.game_info.as_ref() {
            parts.push(gi.rom_family.clone());
            parts.push(family_display_name(lang, &gi.rom_family, gi.rom_variant));
            if let Some(g) = u8::try_from(gi.rom_variant)
                .ok()
                .and_then(|v| crate::game::find_by_family_and_variant(&gi.rom_family, v))
            {
                parts.push(crate::game::short_name(lang, g));
            }
            if let Some(p) = gi.patch.as_ref() {
                parts.push(format!("{} v{}", p.name, p.version));
            }
        }
    }
    parts.push(md.link_code.clone());
    parts.push(format_ts(md.ts, "%Y-%m-%d"));
    let parent = r
        .path
        .parent()
        .map(|p| replays::format_rel_path(replays_path, p))
        .unwrap_or_default();
    let filename = r
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    parts.push(format!("{parent}{filename}"));
    parts.join("\n").to_lowercase()
}

/// "Mega Man Battle Network 6" — family-only i18n lookup, matching
/// how the lobby renders the game line. Falls back to "{family}
/// v{variant}" for unrecognized families.
fn family_display_name(lang: &LanguageIdentifier, family: &str, variant: u32) -> String {
    crate::game::family_str(family, lang, "name").unwrap_or_else(|| format!("{family} v{variant}"))
}

/// A replay's millis-since-epoch timestamp, formatted per `fmt` in
/// local time; `"(?)"` when the value is out of range.
fn format_ts(ms: u64, fmt: &str) -> String {
    std::time::UNIX_EPOCH
        .checked_add(std::time::Duration::from_millis(ms))
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).format(fmt).to_string())
        .unwrap_or_else(|| "(?)".to_string())
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

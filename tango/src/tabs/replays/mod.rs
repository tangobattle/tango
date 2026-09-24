use crate::config;
use crate::i18n::t;
use crate::library::replays;
use crate::library::Catalog;
use crate::ui::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::ui::widgets;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, text_input};
use unic_langid::LanguageIdentifier;

mod detail;
mod export;
mod format;
mod list;
use crate::library::game::family_display_name_or_raw;
use detail::replay_detail;
pub use export::{ExportError, ExportMessage, ExportSettings, PerReplay};
use format::{format_ts, link_code_display};
pub use list::DateFilter;

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
    /// An [`Effect::LoadPreview`] finished: what selecting the replay
    /// needed from disk. The once-taken slot lets iced clone messages
    /// without cloning the loaded saves.
    PreviewLoaded(
        std::path::PathBuf,
        std::sync::Arc<std::sync::Mutex<Option<ReplayPreview>>>,
    ),
    /// Select which replay participant's embedded build is shown in the
    /// save viewer. The You/Opponent cards act as the two-choice control.
    BuildSelected(BuildSide),
    /// Unmask the selected replay's HP chart, which streamer mode otherwise
    /// replaces with a placeholder.
    RevealChart,
    /// Append a replay to the playback queue (the detail panel's Queue
    /// button). The same replay can be queued more than once.
    Enqueue(std::path::PathBuf),
    /// Drop one entry from the queue — the ✕ on its row. By position, not
    /// path: the same replay can appear more than once, and removing one
    /// copy shouldn't take its twin with it.
    Dequeue(usize),
    /// Empty the queue outright.
    ClearQueue,
    /// Start the queue now: pops the front and watches it, rather than
    /// waiting for a currently-playing replay to run out.
    PlayQueue,
    /// Reveal the replay file in the OS file manager, selected.
    RevealReplay(std::path::PathBuf),
    Watch(std::path::PathBuf),
    /// Stop the patch download a Watch click started.
    CancelPatchDownload(crate::library::patch::VersionKey),
    /// Export-panel interactions (form, render lifecycle, round
    /// mask), folded under one variant — see [`ExportMessage`] and
    /// [`ReplaysState::update_export`].
    Export(ExportMessage),
    /// Lazy-load result from `replays::compute_stats`. The App
    /// kicks one worker per missing path post-scan; each result
    /// arrives as one of these messages and lands in
    /// [`ReplaysState::stats`].
    StatsLoaded(std::path::PathBuf, crate::library::replays::ReplayStats),
    /// An [`Effect::AnalyzeReplay`] re-simulation reporting a throttled
    /// partial result, rendered as a live chart that draws itself in
    /// while the analysis runs.
    HpStatsPartial(std::path::PathBuf, tango_match::analysis::MatchStats),
    /// An [`Effect::AnalyzeReplay`] re-simulation finished. `None` =
    /// analysis failed (missing ROM, undecodable) — clears the pending
    /// marker so a later re-focus can retry (e.g. after the user
    /// installs the ROM).
    HpStatsLoaded(std::path::PathBuf, Option<tango_match::analysis::MatchStats>),
    SaveEditor(BuildSide, std::sync::Arc<dyn tango_gamesupport::SaveEditorMessage>),
    /// Used by Tasks that need a Message to return but want no
    /// state mutation. Currently: the user dismissed the Save As
    /// file dialog without picking a path — the export form should
    /// stay open and untouched.
    NoOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildSide {
    #[default]
    You,
    Opponent,
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
    /// Replays to play after the current one, in order. Strictly "what's up
    /// next": [`Message::Watch`] doesn't touch it, and reaching the end of a
    /// playback session pops the front and plays it (see
    /// `App::advance_replay_queue`). Lives here rather than on the App because
    /// this tab is where it's built and shown; the App only drains it.
    pub queue: Vec<std::path::PathBuf>,
    /// The one replay whose HP chart the user has explicitly unmasked in
    /// streamer mode. Held as a path rather than a flag so selecting anything
    /// else re-masks on its own — revealing one match's chip history
    /// shouldn't make the next replay clicked come up bare.
    pub revealed: Option<std::path::PathBuf>,
    /// Cached LoadedSave for the currently-selected replay's local side.
    pub loaded: Option<crate::selection::LoadedSave>,
    /// Cached LoadedSave for the selected replay's remote side. Kept beside
    /// `loaded` so the matchup control can switch the viewer instantly.
    pub opponent_loaded: Option<crate::selection::LoadedSave>,
    /// Which participant currently feeds the read-only save viewer.
    pub viewed_build: BuildSide,
    /// Path the cached `loaded` was built for. Used to invalidate the
    /// cache when the selection changes.
    pub loaded_cache_path: Option<std::path::PathBuf>,
    /// A clicked replay whose [`Effect::LoadPreview`] is still out. The
    /// selection moves to it when its preview lands, so the detail panel
    /// switches in one step, as it did when the load ran inline; a newer
    /// click or a filter change supersedes it.
    pub pending_selection: Option<std::path::PathBuf>,
    /// Recorded tick count of the selected replay, from the same decode
    /// that builds `loaded`. This is what fixes the HP chart's timeline
    /// while its analysis runs (see [`crate::ui::matchup::cook_hp_rounds`]) — the
    /// recording's length is known from the first frame even though its
    /// rounds are not.
    pub loaded_total_ticks: Option<u32>,
    /// Per-replay UI state, keyed by replay path. Entries appear
    /// on first interaction (Selected, ExportPanelOpen, or
    /// ExportStart) and are pruned on navigation if they hold
    /// no in-flight render.
    pub per: std::collections::HashMap<std::path::PathBuf, PerReplay>,
    /// Export form defaults — these are *global* user preferences
    /// (scale, raw output, mute), not per-replay choices, so they
    /// live outside `per`.
    pub export_settings: ExportSettings,
    /// Lazy-loaded duration/round/completion stats keyed by replay
    /// path. Populated by the App's background worker after a
    /// scan; sidebar reads it to render the second caption line.
    /// Missing entries just hide that line until the worker fills
    /// them in.
    pub stats: std::collections::HashMap<std::path::PathBuf, crate::library::replays::ReplayStats>,
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
    pub detail_enter: crate::ui::anim::Enter,
}

/// A replay's match stats, cooked for drawing (see
/// [`crate::ui::matchup::cook_hp_rounds`]). Built once per replay when its
/// [`tango_match::analysis::MatchStats`] arrive.
pub struct HpChart {
    pub rounds: Vec<widgets::CookedHpRound>,
    /// The match-wide HP scale the traces were normalized against — the
    /// chart's hover readout multiplies back through it.
    pub max_hp: f32,
    /// The recording's inter-round boundary ticks, for an export that
    /// wants chapters. Empty on a chart built from a fold still running:
    /// its rounds are a truthful prefix, but sizing an export's round
    /// mask from a prefix would quietly render the last two rounds as
    /// one — see [`ReplaysState::adopt_stats`].
    pub marks: Vec<u32>,
    /// Whether the analysis behind this chart finished. Distinct from
    /// `marks` being empty, which a finished single-round match also is.
    pub complete: bool,
    /// Whether the recording opens on a setup section (bn6 random
    /// battle's pre-round rank/folder phase): its first `marks` entry is
    /// then the setup → round 1 boundary, not an inter-round one, and
    /// the export's sections label/number accordingly. Gated on
    /// `complete` like `marks`.
    pub has_setup: bool,
}

impl HpChart {
    fn new(
        stats: &tango_match::analysis::MatchStats,
        loadeds: [Option<&crate::selection::LoadedSave>; 2],
        total_ticks: Option<u32>,
        complete: bool,
    ) -> Self {
        // Resolve each lane through that participant's own ROM/save assets.
        // A side whose build could not be loaded falls back to `???`/no icon;
        // BN1 records no chip events at all.
        let (rounds, max_hp) = crate::ui::matchup::cook_hp_rounds(stats, loadeds, total_ticks);
        Self {
            rounds,
            max_hp,
            marks: if complete { stats.round_marks() } else { vec![] },
            complete,
            has_setup: complete && stats.has_setup(),
        }
    }
}

/// What selecting a replay needed read from disk, loaded off the UI
/// thread for [`Message::PreviewLoaded`].
pub struct ReplayPreview {
    /// Both participants' builds, or why the recording couldn't be read.
    /// `None` when the cached builds still apply.
    pub builds: Option<anyhow::Result<ReplayBuilds>>,
    /// The replay's stats sidecar, when it has one.
    pub stats: Option<tango_match::analysis::MatchStats>,
}

impl std::fmt::Debug for ReplayPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayPreview").finish_non_exhaustive()
    }
}

/// Each participant's save-view build of a replay (either can be missing
/// — say, a ROM that isn't in the library), plus the recording's length.
pub struct ReplayBuilds {
    pub local: Option<crate::selection::LoadedSave>,
    pub opponent: Option<crate::selection::LoadedSave>,
    /// Recorded tick count, which fixes the HP chart's timeline while
    /// its analysis runs.
    pub total_ticks: u32,
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
    /// Stop the patch download a Watch click started.
    CancelPatchDownload(crate::library::patch::VersionKey),
    /// Read what selecting a replay needs off the UI thread — its stats
    /// sidecar, and, unless `builds` is false because the cached ones
    /// still apply, both participants' builds — and post it back as
    /// [`Message::PreviewLoaded`].
    LoadPreview { replay: std::path::PathBuf, builds: bool },
    /// A focused replay has no stats sidecar — App spawns
    /// `replays::compute_and_cache_match_stats` on a blocking worker
    /// and posts the result back as [`Message::HpStatsLoaded`].
    AnalyzeReplay(std::path::PathBuf),
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy HTML to the clipboard with a plain-text alternative.
    CopyHtml { text: String, html: String },
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// Open the native Save-File dialog for the given replay's
    /// rendered video. App picks a path async and dispatches
    /// `Message::ExportStart`. `raw_output` selects the default
    /// extension/filter: .mkv for raw RGB24 + PCM, .mp4 for
    /// scaled exports.
    OpenExportSaveDialog {
        replay: std::path::PathBuf,
        raw_output: bool,
    },
    /// User confirmed an export. App decodes the replay, resolves
    /// hooks + ROMs, spawns the [`crate::replay_render`] job,
    /// and streams `Message::ExportProgress` / `ExportFinished`
    /// back into this module. `clip` is the player's marked span
    /// (`None` = whole replay, gated by `rounds`; the spawn builds
    /// the degenerate all-of-it clip itself, and `round_marks` is
    /// where it cuts the chapters).
    StartExport {
        replay: std::path::PathBuf,
        output: std::path::PathBuf,
        settings: ExportSettings,
        rounds: Vec<bool>,
        round_marks: Vec<u32>,
        /// Whether the first mark interval is the recording's setup
        /// section rather than round 1 — shifts the chapter titles
        /// (see [`HpChart::has_setup`]).
        has_setup: bool,
        clip: Option<crate::replay_render::Clip>,
        /// Render the opposite seat's perspective — the player's swap
        /// toggle as it stood when the clip export started. Always
        /// false for the panel's whole-replay exports, which have no
        /// swap notion.
        swap_sides: bool,
        /// The job's canceller, which this tab keeps for its Cancel
        /// button.
        canceller: crate::replay_render::Canceller,
    },
    /// Task returned from the save view's `ui.update`. Generic Task
    /// pipe so save_editor-internal side effects (currently just
    /// the scroll-to-top snap on tab changes) flow through here
    /// without per-feature Effect variants.
    SaveEditorTask(iced::Task<Message>),
}

impl ReplaysState {
    /// Apply a tab message. Pure UI-state mutations happen
    /// in-place; anything that needs the App's collaborators
    /// (clipboard, file dialog, session host, …) is bubbled up
    /// as a single optional [`Effect`].
    pub fn update(&mut self, msg: Message, config: &config::Config) -> Option<Effect> {
        match msg {
            Message::GameFilterSelected(pair) => {
                self.game_filter = pair;
                // Filter change can hide the current selection;
                // drop the cached OpenSave so the next interaction
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
            Message::RevealChart => {
                self.revealed = self.selected.clone();
                None
            }
            Message::Enqueue(p) => {
                self.queue.push(p);
                None
            }
            Message::Dequeue(i) => {
                if i < self.queue.len() {
                    self.queue.remove(i);
                }
                None
            }
            Message::ClearQueue => {
                self.queue.clear();
                None
            }
            Message::PlayQueue => {
                // Popping here (rather than letting the App do it) keeps the
                // queue meaning "after this one" no matter which way playback
                // started.
                if self.queue.is_empty() {
                    None
                } else {
                    Some(Effect::Watch(self.queue.remove(0)))
                }
            }
            Message::Selected(p) => {
                let builds = self.loaded_cache_path.as_ref() != Some(&p);
                let stats = !self.hp_charts.contains_key(&p) && !self.hp_pending.contains(&p);
                if !builds && !stats {
                    self.pending_selection = None;
                    return self.select(
                        p,
                        ReplayPreview {
                            builds: None,
                            stats: None,
                        },
                    );
                }
                self.pending_selection = Some(p.clone());
                Some(Effect::LoadPreview { replay: p, builds })
            }
            Message::PreviewLoaded(p, slot) => {
                // A newer click, or a filter change, has moved on.
                if self.pending_selection.as_ref() != Some(&p) {
                    return None;
                }
                self.pending_selection = None;
                let preview = slot.lock().unwrap().take().unwrap_or(ReplayPreview {
                    builds: Some(Err(anyhow::anyhow!("replay preview worker failed"))),
                    stats: None,
                });
                self.select(p, preview)
            }
            Message::BuildSelected(side) => {
                let available = match side {
                    BuildSide::You => self.loaded.is_some(),
                    BuildSide::Opponent => self.opponent_loaded.is_some(),
                };
                if available && self.viewed_build != side {
                    self.viewed_build = side;
                    let data = match side {
                        BuildSide::You => self.loaded.as_mut(),
                        BuildSide::Opponent => self.opponent_loaded.as_mut(),
                    }
                    .expect("availability checked above");
                    data.editor.restart_view_entrance(data.state.as_mut());
                }
                None
            }
            Message::RevealReplay(p) => Some(Effect::RevealPath(p)),
            Message::Watch(p) => Some(Effect::Watch(p)),
            Message::CancelPatchDownload(key) => Some(Effect::CancelPatchDownload(key)),
            Message::SaveEditor(side, msg) => {
                // Clipboard outcomes need the App's clipboard collaborator
                // — bubble them up as Effects. Anything else gets folded
                // into save_editor-internal state and surfaces as a generic
                // SaveEditorTask (currently used for the scroll-to-top snap
                // on a tab change). Edit/Play outcomes can't fire here:
                // the replay save view renders read-only.
                let data = match side {
                    BuildSide::You => self.loaded.as_mut(),
                    BuildSide::Opponent => self.opponent_loaded.as_mut(),
                }?;
                let (sv_task, outcome) = data.editor.update(&config.language, data, &*msg);
                match outcome {
                    Some(tango_gamesupport::SaveEditorEvent::CopyText(s)) => Some(Effect::CopyText(s)),
                    Some(tango_gamesupport::SaveEditorEvent::CopyHtml { text, html }) => {
                        Some(Effect::CopyHtml { text, html })
                    }
                    Some(tango_gamesupport::SaveEditorEvent::CopyImage(img)) => Some(Effect::CopyImage(img)),
                    Some(_) => None,
                    None => Some(Effect::SaveEditorTask(
                        sv_task.map(move |msg| Message::SaveEditor(side, msg)),
                    )),
                }
            }
            Message::Export(m) => self.update_export(m),
            Message::StatsLoaded(path, s) => {
                self.stats.insert(path, s);
                None
            }
            Message::HpStatsPartial(path, partial) => {
                // Live preview: the in-flight analysis renders as a chart
                // that gains a round each time the fold crosses a
                // boundary. Selected-only, like `HpStatsLoaded` — chip
                // names resolve through the selected replay's OpenSave.
                if self.selected.as_ref() == Some(&path) {
                    self.adopt_stats(path, &partial, false);
                }
                None
            }
            Message::HpStatsLoaded(path, stats) => {
                self.hp_pending.remove(&path);
                // The round count is the analysis's to report, and the
                // list row was built before there was one — so fill it
                // in whatever the selection has moved on to, or the
                // caption stays short until the next rescan.
                if let (Some(stats), Some(row)) = (stats.as_ref(), self.stats.get_mut(&path)) {
                    row.round_count = Some(stats.rounds.len() as u32);
                }
                match stats {
                    // Chip beads bake names through the selected replay's
                    // OpenSave; if the user has moved on to another replay by
                    // the time the analysis lands, drop the result rather
                    // than resolving through the wrong game's chip table —
                    // the analysis already wrote the sidecar, so
                    // re-selecting rebuilds the chart straight from disk.
                    Some(stats) if self.selected.as_ref() == Some(&path) => {
                        self.adopt_stats(path, &stats, true);
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
        self.pending_selection = None;
        self.loaded = None;
        self.opponent_loaded = None;
        self.viewed_build = BuildSide::You;
        self.loaded_cache_path = None;
        self.loaded_total_ticks = None;
        self.sweep_idle_entries();
    }

    /// Move the selection to `p`, with whatever its [`ReplayPreview`]
    /// loaded.
    fn select(&mut self, p: std::path::PathBuf, preview: ReplayPreview) -> Option<Effect> {
        if self.selected.as_ref() != Some(&p) {
            self.detail_enter.start(iced::time::Instant::now());
            self.viewed_build = BuildSide::You;
            // Moving off a replay re-masks it, so coming back to one
            // that was unmasked earlier doesn't put its chart straight
            // back on the stream.
            self.revealed = None;
        }
        self.selected = Some(p.clone());
        if let Some(builds) = preview.builds {
            self.adopt_builds(&p, builds);
        }
        self.sweep_idle_entries();
        // A chart from earlier in the session is kept as it is —
        // but the fresh selection cleared this replay's export
        // mask, so re-seat it from what that chart's analysis
        // found. Without this a second visit would silently lose
        // the round selector.
        if let Some(rounds) = self.hp_charts.get(&p).filter(|c| c.complete).map(|c| c.rounds.len()) {
            self.per.entry(p.clone()).or_default().rounds = vec![true; rounds.max(1)];
        }
        // First focus builds the replay's match stats: take the
        // sidecar (cheap; e.g. written at match teardown, or by a
        // previous focus), and only re-simulate when there isn't
        // one. Failures clear `hp_pending` via the result message,
        // so a later focus retries.
        if !self.hp_charts.contains_key(&p) && !self.hp_pending.contains(&p) {
            if let Some(stats) = preview.stats {
                // The builds above already point at this replay, so
                // chip beads get the right names.
                self.adopt_stats(p.clone(), &stats, true);
            } else {
                // Seed an empty chart so the pane has its frame
                // from the start; the analysis fills it in a
                // round at a time.
                self.adopt_stats(p.clone(), &Default::default(), false);
                self.hp_pending.insert(p.clone());
                return Some(Effect::AnalyzeReplay(p));
            }
        }
        None
    }

    /// Take a replay's stats: cook them into the chart, and — only once
    /// the analysis is `complete` — size that replay's export round mask
    /// from them.
    ///
    /// The mask waits because it decides what gets written: sizing it
    /// from a fold that has found two of three rounds would offer "round
    /// 2" as a checkbox and then render rounds 2 AND 3 under it, with
    /// nothing on screen saying so. The chart has no such problem — a
    /// missing round is a missing segment — so it draws throughout.
    fn adopt_stats(&mut self, path: std::path::PathBuf, stats: &tango_match::analysis::MatchStats, complete: bool) {
        if complete {
            // One mask slot per export section: the rounds, plus the
            // setup section when the recording opens on one.
            let sections = (stats.rounds.len() + stats.has_setup() as usize).max(1);
            self.per.entry(path.clone()).or_default().rounds = vec![true; sections];
            // The lazy stats worker ran before this analysis existed, so
            // its row has no round count. Fill it in now rather than
            // leaving the caption short until the next rescan.
            if let Some(row) = self.stats.get_mut(&path) {
                row.round_count = Some(stats.rounds.len() as u32);
            }
        }
        self.hp_charts.insert(
            path,
            HpChart::new(
                stats,
                [self.loaded.as_ref(), self.opponent_loaded.as_ref()],
                self.loaded_total_ticks,
                complete,
            ),
        );
    }

    /// Take the selected replay's freshly loaded builds. Cache only once
    /// both sides are available; a side whose ROM arrives after a rescan
    /// can then be retried by reselecting it.
    fn adopt_builds(&mut self, path: &std::path::Path, builds: anyhow::Result<ReplayBuilds>) {
        match builds {
            Ok(builds) => {
                self.loaded = builds.local;
                self.opponent_loaded = builds.opponent;
                if self.loaded.is_none() && self.opponent_loaded.is_some() {
                    self.viewed_build = BuildSide::Opponent;
                }
                self.loaded_cache_path =
                    (self.loaded.is_some() && self.opponent_loaded.is_some()).then(|| path.to_path_buf());
                self.loaded_total_ticks = Some(builds.total_ticks);
            }
            Err(e) => {
                log::warn!("replay save preview failed: {e}");
                self.loaded = None;
                self.opponent_loaded = None;
                self.loaded_cache_path = None;
                self.loaded_total_ticks = None;
            }
        }
        // The rounds are the analysis's to say, not the file's: a fresh
        // selection starts with no mask, and `adopt_stats` fills one in
        // when this replay's stats land.
        self.per.entry(path.to_path_buf()).or_default().rounds.clear();
    }

    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Catalog,
        config: &'a config::Config,
        netplay_phase: &'a crate::netplay::Phase,
        downloads: &'a crate::library::patch::Downloads,
        // The startup scan hasn't landed yet; until it does, an empty
        // list means "not read yet", not "no replays".
        scanning: bool,
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
        // Say so where the rows would be: an empty pane here is
        // indistinguishable from a library with no replays in it, and
        // the scan is exactly the part that can take a while on a big
        // collection. Gated on the scan rather than on the list being
        // empty — the four scan phases publish as they finish, so a
        // half-scanned list is no more trustworthy than an empty one.
        let left_body: Element<'_, Message> = if scanning {
            container(
                text(t!(lang, "replays-scanning"))
                    .size(TEXT_BODY)
                    .style(widgets::muted_text_style),
            )
            .center(Fill)
            .into()
        } else {
            scrollable(list).style(widgets::chunky_scrollable).height(Fill).into()
        };
        let list_pane = container(left_body).width(Fill).height(Fill).style(widgets::pane);
        // The queue is its own pane under the list it was built from, and
        // only exists when something is in it — an empty bar would be a
        // standing reminder of a feature most sessions never touch.
        let left: Element<'_, Message> = match self.queue_strip(lang, &replays, netplay_active) {
            Some(strip) => column![list_pane, strip]
                .spacing(style::PANE_GAP)
                .width(Length::Fixed(360.0))
                .height(Fill)
                .into(),
            None => container(list_pane).width(Length::Fixed(360.0)).height(Fill).into(),
        };

        // Right panel: replay_detail returns a column of panes
        // when something is selected; the empty-state collapses to
        // a single centered pane.
        let right: Element<'_, Message> = if let Some(r) = self
            .selected
            .as_ref()
            .and_then(|sel_path| filtered.iter().find(|r| &r.path == sel_path))
        {
            let detail = replay_detail(
                lang,
                r,
                &replays_path,
                self,
                scanners,
                netplay_active,
                config.streamer_mode,
                downloads,
            );
            // Selection entrance: the detail panel rises up into
            // place.
            crate::ui::anim::slide_in_opt(
                detail,
                self.detail_enter.progress(iced::time::Instant::now()),
                iced::Vector::new(0.0, 28.0),
            )
        } else {
            widgets::pane_prompt(t!(lang, "replays-select-prompt"))
        };

        widgets::top_split_pane(top, left, right)
    }
}

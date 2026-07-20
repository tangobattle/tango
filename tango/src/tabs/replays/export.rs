//! Replay video export: the per-replay render job + its settings, the
//! inline export panel (form → progress → result), and the message
//! handling that drives them. Split out of the replays tab so the
//! list/detail browsing code in `mod.rs` isn't carrying the whole
//! encode pipeline's UI state.
//!
//! The export *effects* (open the Save-As dialog, spawn the encode
//! task) still flow back to the App through [`super::Effect`] — only
//! the tab-local state machine lives here.

use super::*;
// Explicit so the macros win over iced's prelude `column!`/`row!` (see mod.rs).
use sweeten::widget::{column, row};

/// Export-panel messages, folded under [`super::Message::Export`] so
/// the whole render subsystem routes through one arm of the tab's
/// dispatch ([`ReplaysState::update_export`]).
#[derive(Debug, Clone)]
pub enum ExportMessage {
    /// User clicked Save As. App opens an async file dialog and, on
    /// result, dispatches [`Start`](Self::Start).
    SaveAs(std::path::PathBuf),
    /// Internal: file dialog returned. Carries the source replay path +
    /// the user-picked output path. App spawns the actual export task
    /// in this handler.
    Start {
        replay: std::path::PathBuf,
        output: std::path::PathBuf,
    },
    /// Progress tick from the running export task: (completed, total)
    /// frame pairs. Includes the source replay path so the detail view
    /// can decide whether to render its status line.
    Progress {
        replay: std::path::PathBuf,
        completed: usize,
        total: usize,
    },
    /// Export task completed. Carries the output path on success or an
    /// error description on failure. Same replay-scoping as
    /// [`Progress`](Self::Progress).
    Finished {
        replay: std::path::PathBuf,
        result: Result<std::path::PathBuf, String>,
    },
    /// Dismiss a finished (or failed) export job from the per-replay
    /// job map. Path identifies which job to drop so the detail panel
    /// can offer a per-replay close button.
    Dismiss(std::path::PathBuf),
    /// User clicked Cancel while an export is in flight. Calls `kill()`
    /// on the job's canceller; the export thread sees it next tick (or
    /// via its in-flight ffmpeg pipe failing) and returns, leaving a
    /// partial WebM on disk.
    Cancel(std::path::PathBuf),
    /// Open the rendered video with the OS's default handler.
    OpenFile(std::path::PathBuf),
    /// Export settings widgets. `scale = 0` is the lossless stop.
    SetScale(u8),
    SetDisableBgm(bool),
    SetTwosided(bool),
    /// Toggle the Nth round in the per-replay round mask.
    ToggleRound(usize, bool),
    /// Open / close the inline export-options panel. Distinct from
    /// [`SaveAs`](Self::SaveAs) (which actually triggers the export).
    /// Both carry a path because panel open-state is per-replay — the
    /// same panel can be open on replay A while closed on B.
    PanelOpen(std::path::PathBuf),
    PanelClose(std::path::PathBuf),
    /// The player's Export-clip flow landed a save path: start a clip
    /// export ([`crate::replay_export::Clip`] — the marked span, the
    /// jump-start snapshot, and the session's round marks were all
    /// captured when the scissors chip was pressed).
    StartClip {
        replay: std::path::PathBuf,
        output: std::path::PathBuf,
        clip: crate::replay_export::Clip,
    },
}

impl ReplaysState {
    /// Apply an export-panel message. Mirrors the tab's main
    /// [`update`](ReplaysState::update): pure per-replay state mutates
    /// in place; anything needing the App (file dialog, encode task,
    /// OS open) bubbles up as an [`Effect`].
    pub(super) fn update_export(&mut self, msg: ExportMessage) -> Option<Effect> {
        match msg {
            ExportMessage::SaveAs(replay_path) => Some(Effect::OpenExportSaveDialog {
                replay: replay_path,
                lossless: self.export_settings.scale == 0,
            }),
            ExportMessage::Start { replay, output } => {
                // Snapshot the form + round mask exactly as the
                // user has it right now. With the panel forced
                // open mid-render, the form widgets are out of
                // reach until the render finishes anyway.
                let settings = self.export_settings;
                let entry = self.per.entry(replay.clone()).or_default();
                let mut rounds = entry.rounds.clone();
                if rounds.is_empty() {
                    // Single-round replays don't show the rounds
                    // selector at all, so this guards the "user
                    // hit Save As before any rounds were
                    // computed" race.
                    rounds = vec![true];
                }
                entry.job = Some(ExportJob::new(output.clone()));
                // Pin the panel open so it stays visible if the
                // user navigates elsewhere mid-render. The Done
                // state will collapse naturally on the next
                // navigation sweep.
                entry.panel_open = true;
                Some(Effect::StartExport {
                    replay,
                    output,
                    settings,
                    rounds,
                    clip: None,
                })
            }
            ExportMessage::StartClip { replay, output, clip } => {
                // The player's clip export rides the same per-replay
                // job machinery as the panel's — the tab shows its
                // progress and owns its canceller; the panel is
                // pinned open so the job is visible when the user
                // next visits the tab. The round mask doesn't apply
                // (the span is the gate), so the form's checkboxes
                // are left untouched.
                let settings = self.export_settings;
                let entry = self.per.entry(replay.clone()).or_default();
                entry.job = Some(ExportJob::new(output.clone()));
                entry.panel_open = true;
                Some(Effect::StartExport {
                    replay,
                    output,
                    settings,
                    rounds: vec![],
                    clip: Some(clip),
                })
            }
            ExportMessage::Progress {
                replay,
                completed,
                total,
            } => {
                if let Some(job) = self.per.get_mut(&replay).and_then(|e| e.job.as_mut()) {
                    if job.result.is_none() {
                        job.completed = completed;
                        job.total = total;
                    }
                }
                None
            }
            ExportMessage::Finished { replay, result } => {
                let entry = self.per.entry(replay).or_default();
                let was_cancelled = entry.job.as_ref().is_some_and(|j| j.canceller.is_cancelled());
                if was_cancelled {
                    entry.job = None;
                    entry.panel_open = false;
                } else {
                    entry
                        .job
                        .get_or_insert_with(|| ExportJob::new(std::path::PathBuf::new()))
                        .result = Some(result);
                }
                None
            }
            ExportMessage::Cancel(p) => {
                // Flip the canceller. The export thread sees it via
                // either (a) its per-iteration `is_cancelled()` check
                // or (b) `BrokenPipe` on its next ffmpeg pipe write,
                // and returns Err. The canceller's own `is_cancelled`
                // is what the UI reads for "Cancelling…" chrome and
                // for `Finished` to tell user-cancel apart from a real
                // failure.
                if let Some(job) = self.per.get(&p).and_then(|e| e.job.as_ref()) {
                    job.canceller.kill();
                }
                None
            }
            ExportMessage::Dismiss(p) => {
                // Reset clears the finished/errored job so the panel
                // reverts to its form state. Panel stays open — Reset
                // is "do another render of this replay", not "I'm done
                // with the panel". Closing is the dedicated Render
                // toggle's job.
                if let Some(entry) = self.per.get_mut(&p) {
                    entry.job = None;
                }
                None
            }
            ExportMessage::OpenFile(p) => Some(Effect::OpenPath(p)),
            ExportMessage::SetScale(s) => {
                self.export_settings.scale = s.clamp(0, 10);
                None
            }
            ExportMessage::SetDisableBgm(b) => {
                self.export_settings.disable_bgm = b;
                None
            }
            ExportMessage::SetTwosided(b) => {
                self.export_settings.twosided = b;
                None
            }
            ExportMessage::ToggleRound(idx, picked) => {
                if let Some(entry) = self.selected.as_ref().and_then(|p| self.per.get_mut(p)) {
                    if let Some(slot) = entry.rounds.get_mut(idx) {
                        *slot = picked;
                    }
                }
                None
            }
            ExportMessage::PanelOpen(p) => {
                self.per.entry(p).or_default().panel_open = true;
                None
            }
            ExportMessage::PanelClose(p) => {
                if let Some(entry) = self.per.get_mut(&p) {
                    entry.panel_open = false;
                }
                None
            }
        }
    }
}

/// Export status for a single replay. `result` flips to `Some`
/// when the export task finishes; until the user dismisses it,
/// the job stays in its `PerReplay` slot so the detail panel can
/// show the success/failure line.
#[derive(Debug, Clone)]
pub struct ExportJob {
    pub completed: usize,
    pub total: usize,
    pub result: Option<Result<std::path::PathBuf, String>>,
    /// Where the encoder is writing to. Surfaced under the in-flight
    /// caption so the user can see which file the render is going to.
    /// Empty for jobs created in an error state before a path was
    /// known (e.g. ROM lookup failure).
    pub output: std::path::PathBuf,
    /// Push-cancel handle for the export thread. `kill()` flips the
    /// canceller's internal flag (which the export checks every loop
    /// iteration + at each ffmpeg-free boundary) AND terminates the
    /// in-flight ffmpeg subprocesses. Same handle doubles as the UI's
    /// "cancel was clicked" check via `is_cancelled()` — drives panel
    /// chrome (button greys out, caption flips to "Cancelling…") and
    /// lets `Message::ExportFinished` distinguish a user-cancelled
    /// run from a real failure.
    pub canceller: crate::replay_export::Canceller,
}

impl ExportJob {
    pub fn new(output: std::path::PathBuf) -> Self {
        Self {
            completed: 0,
            total: 0,
            result: None,
            output,
            canceller: crate::replay_export::Canceller::new(),
        }
    }
}

/// Every piece of UI state that's specific to one replay path,
/// bundled into one record so the parent state holds a single
/// `HashMap<PathBuf, PerReplay>` instead of three sibling
/// collections. The view function reads one entry; messages
/// mutate one entry. iced has no widget-local state for user
/// data, so this is the per-replay "instance" — owned by the
/// parent, looked up by path.
#[derive(Debug, Default, Clone)]
pub struct PerReplay {
    /// Inline export panel visibility. The view forces it open
    /// while a render is in flight (see [`ReplaysState::is_panel_open`]),
    /// so a closed bool here only takes effect after the render
    /// settles.
    pub panel_open: bool,
    /// Active or finished export job for this replay.
    pub job: Option<ExportJob>,
    /// Per-round include mask. Rebuilt from the decoded replay
    /// in [`ReplaysState::refresh_loaded`].
    pub rounds: Vec<bool>,
}

/// User-tunable settings the export form passes to
/// `crate::replay_export::export(...)`. Defaults match the
/// legacy replay-dump window.
#[derive(Clone, Copy, Debug)]
pub struct ExportSettings {
    /// 0 = lossless (libx264rgb -qp 0, no upscale). 1..=10 = lossy
    /// `scale`× nearest-neighbor upscale. The form surfaces this as a
    /// single 0..=10 slider with "lossless" as the leftmost stop.
    pub scale: u8,
    pub disable_bgm: bool,
    /// Render both players' screens side-by-side (480x160 frame)
    /// instead of just the local POV. Passed through as the export's
    /// `twosided` flag.
    pub twosided: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            scale: 5,
            disable_bgm: false,
            twosided: false,
        }
    }
}

impl ReplaysState {
    /// Drop per-replay entries that hold no in-flight render — i.e.
    /// just stale form / Done-state UI. Called on navigation so
    /// panels collapse when the user moves on, while in-progress
    /// renders keep their state pinned. The currently-selected
    /// replay is also exempt so navigating to a fresh replay
    /// doesn't immediately blow away the entry we just created
    /// for it (rounds defaults, panel-open intent, etc.).
    pub(super) fn sweep_idle_entries(&mut self) {
        let keep = self.selected.clone();
        self.per
            .retain(|p, e| Some(p) == keep.as_ref() || e.job.as_ref().is_some_and(|j| j.result.is_none()));
    }

    /// True iff the panel for `path` should render — either the
    /// user explicitly opened it or there's an in-flight render
    /// keeping it pinned. View pulls this rather than reading the
    /// flag directly so the "in-flight = always open" invariant
    /// lives in one place.
    pub fn is_panel_open(&self, path: &std::path::Path) -> bool {
        self.per
            .get(path)
            .is_some_and(|e| e.panel_open || e.job.as_ref().is_some_and(|j| j.result.is_none()))
    }

    /// True iff `path` has a render currently in progress. The
    /// Render-toggle button uses this to disable itself so the
    /// user can't even try to close the panel mid-render.
    pub fn is_rendering(&self, path: &std::path::Path) -> bool {
        self.per
            .get(path)
            .and_then(|e| e.job.as_ref())
            .is_some_and(|j| j.result.is_none())
    }

    /// Job lookup for the sidebar's per-row render badge.
    pub fn job(&self, path: &std::path::Path) -> Option<&ExportJob> {
        self.per.get(path).and_then(|e| e.job.as_ref())
    }

    /// Round mask for the currently-selected replay, or `&[]` if
    /// nothing's loaded yet. Always returns the live slice — the
    /// caller can pass it straight to the export form view.
    pub fn rounds_for(&self, path: &std::path::Path) -> &[bool] {
        self.per.get(path).map(|e| e.rounds.as_slice()).unwrap_or(&[])
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
pub(super) fn export_panel<'a>(
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
                let cancel_requested = job.canceller.is_cancelled();
                // Disabled (None on_press) once the user has clicked
                // cancel — the encoder thread still has to wind down
                // its current tick + flush the partial WebM, and we
                // don't want a second click queueing anything.
                let cancel_button = widgets::icon_button_maybe(
                    Icon::X,
                    t!(lang, "replays-export-cancel").to_string(),
                    if cancel_requested {
                        None
                    } else {
                        Some(Message::Export(ExportMessage::Cancel(replay_path.to_path_buf())))
                    },
                    STANDARD_PADDING,
                );
                let caption = if cancel_requested {
                    t!(lang, "replays-export-cancelling")
                } else {
                    t!(lang, "replays-export-progress")
                };
                column![
                    text(caption).size(TEXT_CAPTION).style(widgets::muted_text_style),
                    text(job.output.display().to_string()).size(TEXT_CAPTION),
                    // Progress bar + percentage + Cancel side-by-side.
                    // The bar takes the remaining width via `length()`
                    // = Fill so the icon-only cancel hugs the right
                    // edge and the percent label sits between them.
                    row![
                        // Long + thin. `length()` is the primary axis
                        // (width in horizontal) and defaults to Fill;
                        // `girth()` is the secondary axis (height) and
                        // defaults to 30 px, which is the chunky look
                        // we don't want.
                        iced::widget::progress_bar(0.0..=1.0, pct)
                            .girth(Length::Fixed(4.0))
                            .style(widgets::slim_progress_bar),
                        text(pct_label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                        cancel_button,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ]
                .spacing(6)
                .into()
            }
            Some(Ok(path)) => {
                let path_for_open = path.clone();
                column![
                    text(t!(lang, "replays-export-success"))
                        .size(TEXT_CAPTION)
                        .style(widgets::success_text_style),
                    text(path.display().to_string()).size(TEXT_CAPTION),
                    row![
                        widgets::labeled_icon_button(
                            Icon::Play,
                            t!(lang, "replays-export-open"),
                            Message::Export(ExportMessage::OpenFile(path_for_open)),
                            STANDARD_PADDING,
                            widgets::primary_button,
                        ),
                        widgets::labeled_icon_button(
                            Icon::RefreshCw,
                            t!(lang, "replays-export-reset"),
                            Message::Export(ExportMessage::Dismiss(replay_path.to_path_buf())),
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
                text(t!(lang, "replays-export-error", error = format!("{e}")))
                    .size(TEXT_CAPTION)
                    .style(widgets::danger_text_style),
                widgets::labeled_icon_button(
                    Icon::RefreshCw,
                    t!(lang, "replays-export-reset"),
                    Message::Export(ExportMessage::Dismiss(replay_path.to_path_buf())),
                    STANDARD_PADDING,
                    widgets::neutral,
                ),
            ]
            .spacing(6)
            .into(),
        };
        return container(column![body].padding(12))
            .width(Fill)
            .style(widgets::panel)
            .into();
    }
    // Form path — there's no job for THIS replay (the `if let
    // Some(job)` branch above returned). Multiple concurrent
    // renders are allowed, so the form is always live here.
    let in_flight = false;
    // Scale slider goes 0..=10. The leftmost stop (0) is the
    // lossless mode (libx264rgb -qp 0); 1..=10 is the lossy
    // nearest-neighbor upscale factor.
    let scale_label = text(format!(
        "{}: {}",
        t!(lang, "replays-export-scale"),
        if settings.scale == 0 {
            t!(lang, "replays-export-scale-lossless").to_string()
        } else {
            format!("{}×", settings.scale)
        }
    ))
    .size(TEXT_CAPTION)
    .style(widgets::muted_text_style);
    let scale_slider: Element<'a, Message> = iced::widget::slider(0..=10u8, settings.scale, |s| {
        Message::Export(ExportMessage::SetScale(s))
    })
    .style(widgets::chunky_slider)
    .width(Length::Fixed(140.0))
    .into();
    let bgm_chk = iced::widget::checkbox(settings.disable_bgm)
        .label(t!(lang, "replays-export-disable-bgm"))
        .style(widgets::chunky_checkbox);
    let bgm_chk: Element<'a, Message> = if in_flight {
        bgm_chk.into()
    } else {
        bgm_chk
            .on_toggle(|b| Message::Export(ExportMessage::SetDisableBgm(b)))
            .into()
    };
    let twosided_chk = iced::widget::checkbox(settings.twosided)
        .label(t!(lang, "replays-export-twosided"))
        .style(widgets::chunky_checkbox);
    let twosided_chk: Element<'a, Message> = if in_flight {
        twosided_chk.into()
    } else {
        twosided_chk
            .on_toggle(|b| Message::Export(ExportMessage::SetTwosided(b)))
            .into()
    };
    // Save As… commits the form. Disabled when nothing is selected
    // for export. Floats to the right of the controls row, bottom-
    // aligned so it sits level with the slider widget itself (not
    // the caption above it).
    let any_round = selected_rounds.is_empty() || selected_rounds.iter().any(|b| *b);
    let can_start = any_round && !in_flight;
    let save_as_btn: Element<'a, Message> = if can_start {
        widgets::labeled_icon_button(
            Icon::Upload,
            t!(lang, "replays-export-save-as"),
            Message::Export(ExportMessage::SaveAs(replay_path.to_path_buf())),
            STANDARD_PADDING,
            widgets::primary_button,
        )
    } else {
        widgets::labeled_icon_button_maybe(
            Icon::Upload,
            t!(lang, "replays-export-save-as"),
            None,
            STANDARD_PADDING,
            widgets::neutral,
        )
    };
    // Left column stacks the controls (scale + checkboxes) and the
    // optional rounds row. The Save As button lives in the outer
    // row so it can float all the way to the right and bottom-align
    // against whatever vertical extent the left column ends up at
    // (which grows when the rounds row is present).
    let controls_row = row![column![scale_label, scale_slider].spacing(2), bgm_chk, twosided_chk,]
        .spacing(16)
        .align_y(Alignment::Center);
    let mut left_col = column![controls_row].spacing(6);
    if selected_rounds.len() > 1 {
        let label = text(t!(lang, "replays-export-rounds"))
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style);
        let mut rounds_row = row![label].spacing(6).align_y(Alignment::Center);
        for (i, picked) in selected_rounds.iter().enumerate() {
            let cb = iced::widget::checkbox(*picked)
                .label(format!("{}", i + 1))
                .style(widgets::chunky_checkbox);
            let cb: Element<'a, Message> = if in_flight {
                cb.into()
            } else {
                cb.on_toggle(move |v| Message::Export(ExportMessage::ToggleRound(i, v)))
                    .into()
            };
            rounds_row = rounds_row.push(cb);
        }
        left_col = left_col.push(rounds_row);
    }
    let body = row![left_col, horizontal_space(), save_as_btn]
        .spacing(16)
        .align_y(Alignment::End);

    container(body.padding(12)).width(Fill).style(widgets::panel).into()
}

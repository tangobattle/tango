use fluent_templates::Loader;

use crate::{i18n, session};

const SEEK_BAR_WIDTH: f32 = 480.0;
const SEEK_BAR_HEIGHT: f32 = 18.0;
const TRACK_HEIGHT: f32 = 4.0;

/// While dragging, only re-issue a seek when the target moves at least this
/// many ticks from the last issued one. Each scrub-seek runs frames
/// synchronously on the mgba thread; throttling avoids stutter.
const DRAG_SEEK_TICK_THRESHOLD: u32 = 30;

pub struct State {
    drag_value: Option<u32>,
    last_drag_seek_tick: Option<u32>,
    was_dragging: bool,
    was_paused_before_drag: bool,
}

impl State {
    pub fn new() -> State {
        Self {
            drag_value: None,
            last_drag_seek_tick: None,
            was_dragging: false,
            was_paused_before_drag: false,
        }
    }
}

/// Mirror the status bar's auto-hide: show the overlay only while the
/// mouse has moved within the last `HIDE_AFTER`. A drag in progress
/// overrides this — releasing on a hidden bar would orphan the drag
/// state because there'd be no widget to receive `drag_stopped`.
const HIDE_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

pub fn show(
    ctx: &egui::Context,
    session: &session::Session,
    state: &mut State,
    language: &unic_langid::LanguageIdentifier,
    last_mouse_motion_time: &Option<std::time::Instant>,
) {
    let total_ticks = session.replay_total_ticks().unwrap_or(0);
    if total_ticks == 0 {
        return;
    }
    let mouse_recently_active = last_mouse_motion_time
        .map(|t| std::time::Instant::now() - t < HIDE_AFTER)
        .unwrap_or(false);
    if !mouse_recently_active && !state.was_dragging {
        return;
    }
    let current_tick = session.replay_current_tick().unwrap_or(0);
    let prefetched_tick = session.replay_prefetch_progress().unwrap_or(0);
    let round_boundaries = session.replay_round_boundaries().unwrap_or_default();
    let paused = session.is_paused();
    // Use completion_token rather than `current_tick >= total_ticks`: ticks
    // run out a few frames before the round-end animation finishes, and we
    // still want the buttons live during that tail. completion_token only
    // fires once playback is fully done. A seek resets it.
    let at_end = session.completed();

    // Repaint while prefetch is still building so the buffered bar grows
    // without needing mouse motion.
    if prefetched_tick < total_ticks {
        ctx.request_repaint();
    }

    egui::Window::new("")
        .id(egui::Id::new("replay-controls-window"))
        .resizable(false)
        .title_bar(false)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::Vec2::new(0.0, -50.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button("❌")
                    .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-exit").unwrap())
                    .clicked()
                {
                    session.request_close();
                }
                ui.add(egui::Separator::default().vertical());

                if ui
                    .selectable_label(paused, "⏸️")
                    .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-pause").unwrap())
                    .clicked()
                {
                    if at_end {
                        // Pressing play after playback finished restarts from
                        // the top instead of bouncing off the auto-pause guard.
                        let _ = session.replay_seek_to(0);
                        session.set_paused(false);
                    } else {
                        session.set_paused(!paused);
                    }
                }
                ui.add_enabled_ui(!at_end, |ui| {
                    if ui
                        .button("⏯️")
                        .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-step").unwrap())
                        .clicked()
                    {
                        session.frame_step();
                    }
                });
                ui.add(egui::Separator::default().vertical());

                ui.label(format_position(current_tick));

                let action = seek_bar(
                    ui,
                    session,
                    current_tick,
                    prefetched_tick,
                    total_ticks,
                    &round_boundaries,
                    state,
                );

                ui.label(format_position(total_ticks));

                if let Some(target) = action.seek_target {
                    if let Err(e) = session.replay_seek_to(target) {
                        log::debug!("seek to {} skipped: {}", target, e);
                    }
                }
                if action.restore_pause_state {
                    session.set_paused(state.was_paused_before_drag);
                }
            });
        });
}

#[derive(Default)]
struct SeekAction {
    seek_target: Option<u32>,
    restore_pause_state: bool,
}

fn seek_bar(
    ui: &mut egui::Ui,
    session: &session::Session,
    current: u32,
    prefetched: u32,
    total: u32,
    round_boundaries: &[u32],
    state: &mut State,
) -> SeekAction {
    let desired_size = egui::Vec2::new(SEEK_BAR_WIDTH, SEEK_BAR_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    // Pointer position translated to a tick, clamped to the prefetched
    // range (or current, whichever is larger) so the user can't seek past
    // what's been simulated. At startup `prefetched` may briefly lag
    // `current` until the prefetch thread gets going — keeping the bound
    // at max(current, prefetched) prevents the bar getting stuck behind
    // playback in that window.
    let max_seekable = current.max(prefetched);
    let pointer_target = response
        .interact_pointer_pos()
        .or_else(|| ui.input(|i| i.pointer.hover_pos()))
        .filter(|p| rect.x_range().contains(p.x))
        .map(|pos| {
            let frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let raw = (frac * total as f32).round() as u32;
            raw.min(max_seekable)
        });

    let mut action = SeekAction::default();

    if response.drag_started() {
        state.was_dragging = true;
        state.was_paused_before_drag = session.is_paused();
        if !state.was_paused_before_drag {
            session.set_paused(true);
        }
        state.last_drag_seek_tick = None;
        if let Some(t) = pointer_target {
            state.drag_value = Some(t);
            action.seek_target = Some(t);
            state.last_drag_seek_tick = Some(t);
        }
    } else if response.dragged() {
        if let Some(t) = pointer_target {
            state.drag_value = Some(t);
            let should_issue = state
                .last_drag_seek_tick
                .map_or(true, |last| t.abs_diff(last) >= DRAG_SEEK_TICK_THRESHOLD);
            if should_issue {
                action.seek_target = Some(t);
                state.last_drag_seek_tick = Some(t);
            }
        }
    } else if response.drag_stopped() {
        let final_target = state.drag_value.take().unwrap_or(current);
        action.seek_target = Some(final_target);
        action.restore_pause_state = true;
        state.was_dragging = false;
        state.last_drag_seek_tick = None;
    } else if response.clicked() {
        if let Some(t) = pointer_target {
            action.seek_target = Some(t);
        }
        state.drag_value = None;
        state.last_drag_seek_tick = None;
    }

    let displayed = state.drag_value.unwrap_or(current);
    let played_progress = if total > 0 {
        (displayed as f32 / total as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let buffered_progress = if total > 0 {
        (prefetched as f32 / total as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let visuals = &ui.style().visuals;
    let painter = ui.painter();

    // Track: full-width inactive background.
    let track_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::new(rect.width(), TRACK_HEIGHT));
    painter.rect_filled(track_rect, TRACK_HEIGHT * 0.5, visuals.widgets.inactive.bg_fill);

    // Buffered fill: 0..prefetched_progress, dim color (between inactive
    // and selection) so the user can see how far prefetch has reached.
    let buffered_color = blend(visuals.widgets.inactive.bg_fill, visuals.selection.bg_fill, 0.45);
    let mut buffered_rect = track_rect;
    buffered_rect.max.x = buffered_rect.left() + buffered_rect.width() * buffered_progress;
    if buffered_rect.width() > 0.0 {
        painter.rect_filled(buffered_rect, TRACK_HEIGHT * 0.5, buffered_color);
    }

    // Played fill: 0..displayed_progress, full selection color (overdraws
    // the buffered fill in the played region).
    let mut played_rect = track_rect;
    played_rect.max.x = played_rect.left() + played_rect.width() * played_progress;
    if played_rect.width() > 0.0 {
        painter.rect_filled(played_rect, TRACK_HEIGHT * 0.5, visuals.selection.bg_fill);
    }

    // Round boundary tick marks. Drawn on top of the fills so they remain
    // visible regardless of buffered/played state. Short vertical lines
    // (taller than the track but shorter than the marker dot).
    if total > 0 {
        let tick_half_height = TRACK_HEIGHT * 1.5;
        let tick_color = blend(visuals.text_color(), egui::Color32::TRANSPARENT, 0.45);
        for &boundary in round_boundaries {
            if boundary == 0 || boundary >= total {
                continue;
            }
            let frac = boundary as f32 / total as f32;
            let x = rect.left() + rect.width() * frac;
            painter.line_segment(
                [
                    egui::Pos2::new(x, rect.center().y - tick_half_height),
                    egui::Pos2::new(x, rect.center().y + tick_half_height),
                ],
                egui::Stroke::new(1.5, tick_color),
            );
        }
    }

    // Marker dot at the playback (or drag) position.
    let marker_x = rect.left() + rect.width() * played_progress;
    let marker_radius = if response.hovered() || state.was_dragging {
        7.0
    } else {
        5.0
    };
    let marker_pos = egui::Pos2::new(marker_x, rect.center().y);
    painter.circle_filled(marker_pos, marker_radius, visuals.selection.bg_fill);
    painter.circle_stroke(marker_pos, marker_radius, egui::Stroke::new(1.5, visuals.text_color()));

    action
}

fn blend(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    egui::Color32::from_rgba_premultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

fn format_position(ticks: u32) -> String {
    let total_seconds = (ticks as f32 / session::EXPECTED_FPS) as u32;
    let m = total_seconds / 60;
    let s = total_seconds % 60;
    format!("{:02}:{:02}", m, s)
}

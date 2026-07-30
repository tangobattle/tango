//! The match-analysis chart: the continuous-timeline HP graph (with
//! its per-side chip-event lanes) and the round outcome marks. Pure
//! drawing — everything arrives pre-cooked as normalized traces and
//! display strings, so the toolkit stays free of both the match engine
//! and gamesupport. The cooking (resolving chip names/icons through a
//! loaded save, restating engine verdicts) lives in the private
//! gamesupport layer; tango merges both halves into one
//! `crate::ui::widgets` namespace so call sites read the same.

use super::*;

/// A round's verdict, from the local player's perspective. The chart
/// and the outcome marks speak this instead of the match engine's own
/// outcome type so the toolkit stays engine-free; the gamesupport cook
/// restates verdicts into it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundOutcome {
    Win,
    Loss,
    Draw,
}

/// This side's HP-trace color (see [`FIELD_RED`]). Kept as a style fn so
/// legend chips and canvas draws share one signature.
pub fn hp_you_color(_theme: &Theme) -> iced::Color {
    FIELD_RED
}

/// The opponent's HP-trace color (see [`FIELD_BLUE`]).
pub fn hp_opponent_color(_theme: &Theme) -> iced::Color {
    FIELD_BLUE
}

/// The list-row / results-card mark for a round outcome. `None` — a round
/// the recording never decided — gets its own open mark rather than the
/// draw's dash: it was played, it just has no verdict.
pub fn outcome_mark(outcome: Option<RoundOutcome>) -> (Icon, fn(&Theme) -> iced::widget::text::Style) {
    match outcome {
        Some(RoundOutcome::Win) => (Icon::Check, success_text_style),
        Some(RoundOutcome::Loss) => (Icon::X, danger_text_style),
        Some(RoundOutcome::Draw) => (Icon::Minus, muted_text_style),
        None => (Icon::CircleDashed, muted_text_style),
    }
}

/// One match's HP graph: every round on a single continuous timeline,
/// each round a segment whose width is proportional to its tick span,
/// separated by small gaps. Within a segment both navis' HP run as
/// step-lines (HP holds between hits — a diagonal would invent a ramp
/// that never happened) over an inset wash, a zero baseline, and a
/// slightly lighter band under each custom-screen span; the segment's
/// background carries the round's outcome as a tint (win = success
/// wash, loss = danger wash, draw/undecided = neutral). The bottom of
/// the canvas is always two thin per-side chip-event lanes ("you" over
/// the opponent, in the trace colors); each use is a tick in its
/// owner's lane — exact timing without crowding the traces. `sweep`
/// (0..=1 of the whole timeline) reveals the chart left to right with a
/// head dot on each line while mid-sweep. Trace/custom/chip x values
/// are 0..=1 within their round; HP values are normalized to the
/// match-wide maximum by the caller, so every segment shares one
/// vertical scale.
pub struct HpGraphRound<'a> {
    pub trace: &'a [(f32, f32, f32)],
    pub custom: &'a [(f32, f32)],
    /// Chip-use events per side (`[you, opponent]`), sorted by x.
    /// Drawn as ticks in that side's event lane; the hover readout
    /// shows the icon + name of the tick nearest the cursor. Empty on
    /// games whose traps don't report chips.
    pub chip_uses: [&'a [ChipUseMark]; 2],
    pub outcome: Option<RoundOutcome>,
    /// Tick span of the round — its share of the timeline's width.
    pub weight: f32,
}

/// One chip-use event on an [`HpGraphRound`] trace: normalized x within
/// the round, plus the chip's display name and its 14×14 icon (when the
/// game's assets provide one) for the hover readout.
#[derive(Clone)]
pub struct ChipUseMark {
    pub x: f32,
    pub name: String,
    pub icon: Option<iced::widget::image::Handle>,
}

/// A round of match stats cooked for
/// [`hp_match_graph`]: the outcome carried through, everything else
/// normalized — trace `(x, you, opponent)` with x 0..=1 over the round's
/// span and HP against the match-wide maximum, custom spans and
/// chip-use marks on the same x scale. Rounds with fewer than two HP
/// points (torn down mid-intro) cook to an empty trace with weight 0.
#[derive(Clone)]
pub struct CookedHpRound {
    pub outcome: Option<RoundOutcome>,
    pub trace: Vec<(f32, f32, f32)>,
    pub custom: Vec<(f32, f32)>,
    pub chip_uses: [Vec<ChipUseMark>; 2],
    /// Tick span of the round — its share of the continuous timeline.
    pub weight: f32,
}

/// `max_hp` is the match-wide scale the traces were normalized against;
/// hovering the chart shows a crosshair with both navis' HP numbers read
/// back through it, plus the name of any chip-use tick near the cursor.
///
/// `zoom_key` = `Some(key)` makes the timeline zoomable: scroll to zoom
/// about the cursor, drag to pan, double-click to reset, with a thin
/// viewport bar along the top edge while zoomed. The key must identify
/// the match being drawn (e.g. a hash of the replay path) — iced keeps
/// widget state by tree position, so without it the view state would
/// leak across selection changes; a key change resets the view. `None`
/// draws a static chart (the results card, whose reveal choreography
/// shouldn't be scrubbed around in).
///
pub fn hp_match_graph<'a, M: 'a>(
    rounds: Vec<HpGraphRound<'a>>,
    max_hp: f32,
    sweep: f32,
    height: f32,
    zoom_key: Option<u64>,
) -> Element<'a, M> {
    use iced::widget::canvas;

    struct HpMatchGraph<'a> {
        rounds: Vec<HpGraphRound<'a>>,
        max_hp: f32,
        sweep: f32,
        zoom_key: Option<u64>,
    }

    /// Interaction state, persisted by iced across frames. Everything is
    /// in `Cell`s because `draw` (which only gets `&State`) must be able
    /// to apply the key reset — the first frame after a selection change
    /// renders before any event reaches `update`.
    struct ZoomState {
        /// The `zoom_key` the view state was accumulated on.
        key: std::cell::Cell<Option<u64>>,
        /// Horizontal magnification of the timeline, ≥ 1 (1 = the whole
        /// match fits the canvas).
        zoom: std::cell::Cell<f32>,
        /// Left edge of the viewport on the zoomed (virtual) timeline,
        /// in px; kept within [0, virtual width − canvas width].
        offset: std::cell::Cell<f32>,
        /// Cursor x of the last pan event while dragging.
        drag: std::cell::Cell<Option<f32>>,
        /// Time of the previous left press, for double-click reset.
        last_press: std::cell::Cell<Option<iced::time::Instant>>,
        /// Whether the cursor was over the chart on the last mouse event —
        /// lets the leave-event redraw clear the crosshair.
        hovered: std::cell::Cell<bool>,
    }

    impl Default for ZoomState {
        fn default() -> Self {
            Self {
                key: Default::default(),
                zoom: std::cell::Cell::new(1.0),
                offset: Default::default(),
                drag: Default::default(),
                last_press: Default::default(),
                hovered: Default::default(),
            }
        }
    }

    impl ZoomState {
        /// Reset the view when this widget slot switches to drawing a
        /// different match than the state was accumulated on.
        fn sync_key(&self, key: Option<u64>) {
            if self.key.get() != key {
                self.key.set(key);
                self.zoom.set(1.0);
                self.offset.set(0.0);
                self.drag.set(None);
            }
        }
    }

    impl<M> canvas::Program<M> for HpMatchGraph<'_> {
        type State = ZoomState;

        fn update(
            &self,
            state: &mut ZoomState,
            event: &iced::Event,
            bounds: iced::Rectangle,
            cursor: iced::mouse::Cursor,
        ) -> Option<canvas::Action<M>> {
            state.sync_key(self.zoom_key);
            let iced::Event::Mouse(mouse_event) = event else {
                return None;
            };
            let zoomable = self.zoom_key.is_some();
            match *mouse_event {
                iced::mouse::Event::CursorMoved { .. } => {
                    // Pan while dragging a zoomed chart. Deltas come off the
                    // window-level cursor position so the drag survives the
                    // pointer briefly leaving the (short) canvas.
                    if let (Some(last), Some(pos)) = (state.drag.get(), cursor.position()) {
                        let max_off = (bounds.width * (state.zoom.get() - 1.0)).max(0.0);
                        state
                            .offset
                            .set((state.offset.get() - (pos.x - last)).clamp(0.0, max_off));
                        state.drag.set(Some(pos.x));
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                    // The hover crosshair is drawn straight from the cursor
                    // in `draw`, so cursor motion over (or off) the chart
                    // must trigger a redraw — without this the readout only
                    // refreshes when something else invalidates the view
                    // (e.g. a click).
                    let over = cursor.is_over(bounds);
                    let was_over = state.hovered.replace(over);
                    (over || was_over).then(canvas::Action::request_redraw)
                }
                iced::mouse::Event::WheelScrolled { delta } if zoomable => {
                    let pos = cursor.position_in(bounds)?;
                    // Exponential steps feel uniform across notched wheels
                    // (line deltas) and trackpads (pixel deltas).
                    let steps = match delta {
                        iced::mouse::ScrollDelta::Lines { y, .. } => y * 0.25,
                        iced::mouse::ScrollDelta::Pixels { y, .. } => y * 0.01,
                    };
                    let zoom = state.zoom.get();
                    let new_zoom = (zoom * steps.exp()).clamp(1.0, 64.0);
                    // Zoom about the cursor: the timeline point under it
                    // stays put.
                    let anchor = (pos.x + state.offset.get()) * (new_zoom / zoom) - pos.x;
                    state.zoom.set(new_zoom);
                    state
                        .offset
                        .set(anchor.clamp(0.0, (bounds.width * (new_zoom - 1.0)).max(0.0)));
                    Some(canvas::Action::request_redraw().and_capture())
                }
                iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left) if zoomable => {
                    // Presses only count when they land on the chart.
                    cursor.position_in(bounds)?;
                    // Double-click resets the view.
                    let now = iced::time::Instant::now();
                    let double = state
                        .last_press
                        .replace(Some(now))
                        .is_some_and(|prev| now.duration_since(prev) < std::time::Duration::from_millis(350));
                    if double {
                        state.zoom.set(1.0);
                        state.offset.set(0.0);
                        state.drag.set(None);
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                    if state.zoom.get() > 1.0 {
                        state.drag.set(cursor.position().map(|p| p.x));
                        return Some(canvas::Action::capture());
                    }
                    None
                }
                iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) => {
                    state.drag.replace(None).is_some().then(canvas::Action::request_redraw)
                }
                _ => None,
            }
        }

        fn mouse_interaction(
            &self,
            state: &ZoomState,
            bounds: iced::Rectangle,
            cursor: iced::mouse::Cursor,
        ) -> iced::mouse::Interaction {
            if state.drag.get().is_some() {
                iced::mouse::Interaction::Grabbing
            } else if state.zoom.get() > 1.0 && cursor.is_over(bounds) {
                iced::mouse::Interaction::Grab
            } else {
                iced::mouse::Interaction::None
            }
        }

        fn draw(
            &self,
            state: &ZoomState,
            renderer: &iced::Renderer,
            theme: &Theme,
            bounds: iced::Rectangle,
            cursor: iced::mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            use canvas::{Frame, LineCap, Path, Stroke};
            use iced::Point;

            let mut frame = Frame::new(renderer, bounds.size());
            let palette = theme.extended_palette();
            let text_color = theme.palette().text;
            let (w, h) = (bounds.width, bounds.height);
            // Inset vertically so full-HP traces keep their line width
            // on-canvas.
            const PAD: f32 = 3.0;
            const GAP: f32 = 3.0;
            // The bottom of the canvas is always reserved for the two
            // thin per-side chip-event lanes — a fixed layout, whether or
            // not this game/moment has events to show (an analysis
            // renders into this chart live, and the canvas must not jump
            // when the first event lands).
            const LANES_H: f32 = 18.0;
            let field_h = h - LANES_H;
            let y_at = |yf: f32| PAD + (1.0 - yf.clamp(0.0, 1.0)) * (field_h - 2.0 * PAD);
            // Center line of a side's event lane (0 = you, 1 = opponent).
            let lane_y = |side: usize| field_h + 5.0 + side as f32 * 8.0;

            // Zoom: layout runs on a "virtual" timeline `zoom` times the
            // canvas width, and everything drawn is shifted left by
            // `offset` (the canvas clips the rest). Gaps don't scale — they
            // are dividers, not time.
            state.sync_key(self.zoom_key);
            let zoom = state.zoom.get().max(1.0);
            // Re-clamp against the current width — the canvas can resize
            // between events.
            let offset = state.offset.get().clamp(0.0, (w * (zoom - 1.0)).max(0.0));
            state.offset.set(offset);
            let vw = w * zoom;

            // Segments tile the whole timeline, each taking its share of it
            // by tick span — the dividers are carved out of the panels
            // below rather than taking width of their own. That is what
            // makes a tick's position depend only on the tick: a segment's
            // offset is the ticks before it and its width is the ticks in
            // it, so `seg_x + xf * seg_w` reduces to the tick itself and a
            // round arriving mid-analysis (which splits the last segment
            // in two) moves nothing already drawn. Reserving width for the
            // dividers instead would have every panel shrink each time
            // another round was found.
            let total: f32 = self.rounds.iter().map(|r| r.weight.max(1.0)).sum::<f32>().max(1.0);

            let mut segments: Vec<(f32, f32)> = Vec::with_capacity(self.rounds.len());
            let mut seg_x = 0.0f32;
            // The sweep runs over the whole (virtual) timeline; convert to
            // a px cursor so segment boundaries don't distort its pace.
            let sweep_px = self.sweep.clamp(0.0, 1.0) * vw;
            for (i, round) in self.rounds.iter().enumerate() {
                let seg_w = round.weight.max(1.0) / total * vw;
                segments.push((seg_x, seg_w));
                let x_at = |xf: f32| seg_x + xf.clamp(0.0, 1.0) * seg_w - offset;
                // Local reveal fraction of this segment under the global
                // px cursor. A finished sweep pins this to exactly 1.0:
                // `seg_x` accumulates float error across the earlier
                // segments, so the last segment's ratio can land a hair
                // below 1.0 and would otherwise never count as fully swept
                // (which used to hide the final round's outcome mark).
                let local_sweep = if self.sweep >= 1.0 {
                    1.0
                } else {
                    ((sweep_px - seg_x) / seg_w).clamp(0.0, 1.0)
                };

                // Recessed background so each round reads as its own inset
                // panel; the gaps between them are the round dividers,
                // each taken off the END of the round it follows so the
                // next panel still opens exactly on its own first tick.
                // The panel is chrome over the segment, not the segment
                // itself, so a round arriving mid-analysis only shortens
                // the panel that now precedes it — nothing plotted moves.
                // Once the sweep has fully crossed a round, its outcome
                // tints the whole panel — win reads green, loss red; draws
                // and rounds the recording never resolved stay neutral.
                let trail = if i + 1 == self.rounds.len() { 0.0 } else { GAP };
                let bg = Path::rounded_rectangle(
                    Point::new(seg_x - offset, 0.0),
                    iced::Size::new((seg_w - trail).max(1.0), h),
                    3.0.into(),
                );
                frame.fill(
                    &bg,
                    iced::Color {
                        a: if palette.is_dark { 0.10 } else { 0.05 },
                        ..text_color
                    },
                );
                if local_sweep >= 1.0 {
                    let tint = match round.outcome {
                        Some(RoundOutcome::Win) => Some(palette.success.base.color),
                        Some(RoundOutcome::Loss) => Some(palette.danger.base.color),
                        _ => None,
                    };
                    if let Some(color) = tint {
                        frame.fill(
                            &bg,
                            iced::Color {
                                a: if palette.is_dark { 0.14 } else { 0.08 },
                                ..color
                            },
                        );
                    }
                }

                // Custom-screen bands: the stretches where the battle stood
                // paused while chips were picked. They mark time in the
                // trace field only — the event lanes below stay clear.
                for &(x0, x1) in round.custom {
                    let (bx0, bx1) = (x_at(x0.min(local_sweep)), x_at(x1.min(local_sweep)));
                    if bx1 > bx0 {
                        frame.fill_rectangle(
                            Point::new(bx0, 0.0),
                            iced::Size::new(bx1 - bx0, field_h),
                            iced::Color { a: 0.07, ..text_color },
                        );
                    }
                }

                // Zero baseline — where a KO'd navi's trace lands. Runs the
                // panel, not the segment, so it stops at the divider.
                let base_y = y_at(0.0);
                frame.stroke(
                    &Path::line(
                        Point::new(seg_x - offset, base_y),
                        Point::new(seg_x + seg_w - trail - offset, base_y),
                    ),
                    Stroke::default()
                        .with_color(iced::Color { a: 0.22, ..text_color })
                        .with_width(1.0),
                );

                if round.trace.len() >= 2 && local_sweep > 0.0 {
                    // Draw the opponent under this side, so "you" stays
                    // legible where the traces overlap (equal HP at round
                    // start).
                    for you in [false, true] {
                        let color = if you {
                            hp_you_color(theme)
                        } else {
                            hp_opponent_color(theme)
                        };
                        let value = |p: &(f32, f32, f32)| if you { p.1 } else { p.2 };
                        let mut head = None;
                        let path = Path::new(|b| {
                            let mut prev_y = y_at(value(&round.trace[0]));
                            b.move_to(Point::new(x_at(round.trace[0].0), prev_y));
                            for point in &round.trace[1..] {
                                let x = x_at(point.0.min(local_sweep));
                                // Step-line: run flat to the new x, then
                                // drop/rise there.
                                b.line_to(Point::new(x, prev_y));
                                if point.0 > local_sweep {
                                    head = Some(Point::new(x, prev_y));
                                    break;
                                }
                                prev_y = y_at(value(point));
                                b.line_to(Point::new(x, prev_y));
                                head = Some(Point::new(x, prev_y));
                            }
                        });
                        frame.stroke(
                            &path,
                            Stroke::default()
                                .with_color(color)
                                .with_width(1.5)
                                .with_line_cap(LineCap::Round),
                        );
                        // Sweep-head dot: the "now" cursor of the miniature
                        // replay.
                        if local_sweep < 1.0 && local_sweep > 0.0 {
                            if let Some(head) = head {
                                frame.fill(&Path::circle(head, 2.0), color);
                            }
                        }
                    }
                }

                // Chip-use ticks, each side in its own lane: a small comb of
                // events in the side's color, revealed with the sweep like
                // everything else.
                for side in 0..2 {
                    let color = if side == 0 {
                        hp_you_color(theme)
                    } else {
                        hp_opponent_color(theme)
                    };
                    for m in round.chip_uses[side].iter().take_while(|m| m.x <= local_sweep) {
                        frame.fill(
                            &Path::rounded_rectangle(
                                Point::new(x_at(m.x) - 0.75, lane_y(side) - 3.0),
                                iced::Size::new(1.5, 6.0),
                                0.75.into(),
                            ),
                            color,
                        );
                    }
                }

                seg_x += seg_w;
            }

            // Viewport indicator: while zoomed in, a thin bar along the top
            // edge shows which slice of the whole timeline is on screen.
            if zoom > 1.001 {
                frame.fill(
                    &Path::rounded_rectangle(
                        Point::new(offset / zoom, 0.0),
                        iced::Size::new(w / zoom, 2.0),
                        1.0.into(),
                    ),
                    iced::Color { a: 0.30, ..text_color },
                );
            }

            // Hover readout: a crosshair over the hovered segment with the
            // step values under the cursor, read back through the shared
            // scale — dots carry which number is whose, ink stays neutral.
            if let Some(pos) = cursor.position_in(bounds) {
                // The cursor's position on the zoomed (virtual) timeline —
                // segments were laid out there.
                let vx = pos.x + offset;
                let hovered = segments
                    .iter()
                    .zip(&self.rounds)
                    .find(|((sx, sw), _)| vx >= *sx && vx < sx + sw && vx <= sweep_px);
                if let Some((&(sx, sw), round)) = hovered {
                    let xf = ((vx - sx) / sw).clamp(0.0, 1.0);
                    // Only read out where the line actually is: past the
                    // trace's sampled extent (the planned tail an analysis
                    // hasn't reached yet, or the clamped round-end stretch)
                    // there's nothing under the cursor, so no crosshair,
                    // dots, or tooltip.
                    let in_trace = round
                        .trace
                        .first()
                        .zip(round.trace.last())
                        .is_some_and(|(a, b)| xf >= a.0 && xf <= b.0);
                    // Step semantics: the value in force at xf is the last
                    // point at or before it.
                    let at = round.trace.iter().take_while(|p| p.0 <= xf).last();
                    if let Some(&(_, you, opp)) = at.filter(|_| in_trace) {
                        frame.stroke(
                            &Path::line(Point::new(pos.x, 0.0), Point::new(pos.x, h)),
                            Stroke::default()
                                .with_color(iced::Color { a: 0.35, ..text_color })
                                .with_width(1.0),
                        );
                        for (yf, color) in [(opp, hp_opponent_color(theme)), (you, hp_you_color(theme))] {
                            frame.fill(&Path::circle(Point::new(pos.x, y_at(yf)), 2.5), color);
                        }

                        let you_hp = (you * self.max_hp).round() as u32;
                        let opp_hp = (opp * self.max_hp).round() as u32;
                        // Readout lines: both HP numbers, plus the icon and
                        // name of any chip-use tick within grabbing distance
                        // of the cursor (nearest per side). The named tick
                        // gets a ring so the label visibly points at a mark.
                        type ReadoutLine = (String, iced::Color, Option<iced::widget::image::Handle>);
                        let mut lines: Vec<ReadoutLine> = vec![
                            (you_hp.to_string(), hp_you_color(theme), None),
                            (opp_hp.to_string(), hp_opponent_color(theme), None),
                        ];
                        const NEAR_PX: f32 = 4.0;
                        for (side, color) in [(0, hp_you_color(theme)), (1, hp_opponent_color(theme))] {
                            let near = round.chip_uses[side]
                                .iter()
                                .map(|m| (sx + m.x * sw - offset, m))
                                .filter(|(px, _)| (px - pos.x).abs() <= NEAR_PX && px + offset <= sweep_px)
                                .min_by(|a, b| (a.0 - pos.x).abs().total_cmp(&(b.0 - pos.x).abs()));
                            if let Some((px, mark)) = near {
                                frame.stroke(
                                    &Path::circle(Point::new(px, lane_y(side)), 3.5),
                                    Stroke::default().with_color(color).with_width(1.0),
                                );
                                lines.push((mark.name.clone(), color, mark.icon.clone()));
                            }
                        }

                        // Chip lines carry a 14 px icon, so give every row a
                        // little more pitch when one is present. Per-char
                        // width is an estimate tuned for digits and short
                        // latin chip names; long names just run a bit snug
                        // rather than being measured.
                        const ICON: f32 = 14.0;
                        let pitch = if lines.iter().any(|(_, _, icon)| icon.is_some()) {
                            16.0
                        } else {
                            14.0
                        };
                        let box_w = lines
                            .iter()
                            .map(|(s, _, icon)| {
                                16.0 + if icon.is_some() { ICON + 3.0 } else { 0.0 } + s.chars().count() as f32 * 7.0
                            })
                            .fold(1.0f32, f32::max);
                        let box_h = lines.len() as f32 * pitch + 2.0;
                        // Flip to the cursor's left near the right edge so
                        // the readout stays on-canvas.
                        let bx = if pos.x + 10.0 + box_w > w {
                            pos.x - 10.0 - box_w
                        } else {
                            pos.x + 10.0
                        };
                        let by = (pos.y - box_h / 2.0).clamp(0.0, (h - box_h).max(0.0));
                        frame.fill(
                            &Path::rounded_rectangle(Point::new(bx, by), iced::Size::new(box_w, box_h), 4.0.into()),
                            iced::Color {
                                a: 0.92,
                                ..theme.palette().background
                            },
                        );
                        for (i, (content, color, icon)) in lines.into_iter().enumerate() {
                            let line_y = by + 1.0 + pitch / 2.0 + i as f32 * pitch;
                            frame.fill(&Path::circle(Point::new(bx + 7.0, line_y), 2.5), color);
                            let mut text_x = bx + 13.0;
                            if let Some(handle) = icon {
                                frame.draw_image(
                                    iced::Rectangle::new(
                                        Point::new(text_x, line_y - ICON / 2.0),
                                        iced::Size::new(ICON, ICON),
                                    ),
                                    canvas::Image::new(handle)
                                        .filter_method(iced::widget::image::FilterMethod::Nearest)
                                        .snap(true),
                                );
                                text_x += ICON + 3.0;
                            }
                            frame.fill_text(canvas::Text {
                                content,
                                position: Point::new(text_x, line_y),
                                color: text_color,
                                size: 11.0.into(),
                                align_y: iced::alignment::Vertical::Center.into(),
                                ..Default::default()
                            });
                        }
                    }
                }
            }

            vec![frame.into_geometry()]
        }
    }

    iced::widget::canvas::Canvas::new(HpMatchGraph {
        rounds,
        max_hp,
        sweep,
        zoom_key,
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}

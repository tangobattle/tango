//! The HP match chart, HTML/SVG flavor: the desktop's `hp_match_graph`
//! + `hp_hover_strip` canvases (`tango/src/ui/widgets.rs`) rebuilt on
//! an SVG with a stretched 1000-unit x axis (`preserveAspectRatio:
//! none` + non-scaling strokes keep line widths in screen pixels while
//! x geometry stays proportional) and an HTML overlay for the hover
//! crosshair/readout. Zoom/pan is desktop-only for now; everything
//! else — per-round inset panels, outcome washes, custom-screen bands,
//! step traces, chip-event lanes, the step-value readout — matches the
//! desktop spec constant for constant.

use dioxus::prelude::*;
use tango_pvp::analysis::{BattleOutcome, MatchStats};

use crate::save_view::Loaded;

/// The desktop's fixed seat colors — red = you, blue = opponent, in
/// every theme.
pub const FIELD_RED: &str = "#d93847";
pub const FIELD_BLUE: &str = "#2e66d9";

/// Logical x width of the SVG. Rendered stretched to the container, so
/// unit ≈ px at the pane's typical width; the 3-unit round gaps and
/// hover hit radii ride that approximation.
const XU: f32 = 1000.0;

/// Full-graph geometry (the desktop's constants at height 72).
const GRAPH_H: f32 = 72.0;
const PAD: f32 = 3.0;
const GAP: f32 = 3.0;
const LANES_H: f32 = 18.0;
const FIELD_H: f32 = GRAPH_H - LANES_H;

fn y_at(yf: f32) -> f32 {
    PAD + (1.0 - yf.clamp(0.0, 1.0)) * (FIELD_H - 2.0 * PAD)
}

fn lane_y(side: usize) -> f32 {
    FIELD_H + 5.0 + side as f32 * 8.0
}

/// One chip-use event, cooked to segment-relative x with its display
/// name + 14x14 icon data URL resolved through the local `Loaded`.
#[derive(Clone, PartialEq)]
pub struct ChipUseMark {
    pub x: f32,
    pub name: String,
    pub icon: Option<String>,
}

/// One round, cooked to normalized geometry (the desktop's
/// `CookedHpRound`).
#[derive(Clone, PartialEq)]
pub struct CookedHpRound {
    pub outcome: Option<BattleOutcome>,
    /// `(x, you, opponent)`, all 0..=1. Step semantics — HP holds flat
    /// between entries.
    pub trace: Vec<(f32, f32, f32)>,
    pub custom: Vec<(f32, f32)>,
    /// `[you, opponent]`.
    pub chip_uses: [Vec<ChipUseMark>; 2],
    /// The round's tick span — its share of the timeline.
    pub weight: f32,
}

/// Cook `stats` into normalized rounds + the match-wide max HP the
/// traces were scaled by (the readout multiplies back through it).
/// `planned` = per-round tick counts from the recording's round
/// markers: it fixes segment widths up front (and pads not-yet-covered
/// rounds) so the layout matches the scrubber's tick scale exactly.
/// Chip names/icons resolve through the local side's `Loaded` for both
/// sides, the replays-detail best-effort.
pub fn cook_hp_rounds(
    stats: &MatchStats,
    loaded: Option<&Loaded>,
    planned: Option<&[u32]>,
) -> (Vec<CookedHpRound>, f32) {
    let max_hp = stats
        .rounds
        .iter()
        .flat_map(|r| r.hp.iter())
        .map(|p| p.local.max(p.remote))
        .max()
        .unwrap_or(0)
        .max(1) as f32;

    let starts: Option<Vec<u32>> = planned.map(|p| {
        let mut acc = 0u32;
        p.iter()
            .map(|&w| {
                let s = acc;
                acc = acc.saturating_add(w);
                s
            })
            .collect()
    });

    let marks_of = |uses: &[(u32, u16)], x_of: &dyn Fn(u32) -> f32| -> Vec<ChipUseMark> {
        uses.iter()
            .map(|&(tick, id)| ChipUseMark {
                x: x_of(tick),
                name: loaded
                    .and_then(|l| l.assets.chip(id as usize))
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| "???".to_string()),
                icon: loaded.and_then(|l| l.chip_icons.get(id as usize).cloned().flatten()),
            })
            .collect()
    };

    let n = planned.map(|p| p.len()).unwrap_or(0).max(stats.rounds.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let planned_span = planned.and_then(|p| p.get(i).copied()).map(|w| w.max(1) as f32);
        let empty = |weight: f32| CookedHpRound {
            outcome: stats.rounds.get(i).and_then(|r| r.outcome),
            trace: Vec::new(),
            custom: Vec::new(),
            chip_uses: [Vec::new(), Vec::new()],
            weight,
        };
        let Some(r) = stats.rounds.get(i) else {
            // The plan covers rounds the (in-flight or truncated) stats
            // don't — pad at planned width so the frame is stable.
            out.push(empty(planned_span.unwrap_or(1.0)));
            continue;
        };
        let (Some(first), Some(last)) = (r.hp.first(), r.hp.last()) else {
            out.push(empty(planned_span.unwrap_or(1.0)));
            continue;
        };
        if r.hp.len() < 2 {
            // Torn down mid-intro — no drawable trace.
            out.push(empty(planned_span.unwrap_or(1.0)));
            continue;
        }
        let base = starts
            .as_ref()
            .and_then(|s| s.get(i).copied())
            .unwrap_or(first.tick);
        let span = planned_span.unwrap_or_else(|| (last.tick.saturating_sub(base) as f32).max(1.0));
        let x_of = move |tick: u32| (tick.saturating_sub(base) as f32 / span).clamp(0.0, 1.0);
        out.push(CookedHpRound {
            outcome: r.outcome,
            trace: r
                .hp
                .iter()
                .map(|p| (x_of(p.tick), p.local as f32 / max_hp, p.remote as f32 / max_hp))
                .collect(),
            custom: r.custom.iter().map(|&(a, b)| (x_of(a), x_of(b))).collect(),
            chip_uses: [
                marks_of(&r.chip_uses[0], &x_of),
                marks_of(&r.chip_uses[1], &x_of),
            ],
            weight: span,
        });
    }
    (out, max_hp)
}

/// Segment layout on the virtual axis `XU * zoom`: `(seg_x, seg_w)`
/// per round, gaps fixed (they don't represent time, so they don't
/// scale with zoom) and subtracted before distributing width.
fn segments(rounds: &[CookedHpRound], zoom: f32) -> Vec<(f32, f32)> {
    let total: f32 = rounds.iter().map(|r| r.weight.max(1.0)).sum::<f32>().max(1.0);
    let gaps = GAP * rounds.len().saturating_sub(1) as f32;
    let usable = (XU * zoom - gaps).max(1.0);
    let mut out = Vec::with_capacity(rounds.len());
    let mut seg_x = 0.0;
    for r in rounds {
        let seg_w = r.weight.max(1.0) / total * usable;
        out.push((seg_x, seg_w));
        seg_x += seg_w + GAP;
    }
    out
}

/// A step-line SVG path: run flat to each new x, then step to the new
/// value — never a diagonal (a ramp that never happened).
fn step_path(trace: &[(f32, f32, f32)], you: bool, seg_x: f32, seg_w: f32) -> String {
    let value = |p: &(f32, f32, f32)| if you { p.1 } else { p.2 };
    let x_at = |xf: f32| seg_x + xf.clamp(0.0, 1.0) * seg_w;
    let mut d = String::new();
    let mut prev_y = y_at(value(&trace[0]));
    d.push_str(&format!("M {:.1} {:.1}", x_at(trace[0].0), prev_y));
    for point in &trace[1..] {
        let x = x_at(point.0);
        d.push_str(&format!(" L {x:.1} {prev_y:.1}"));
        prev_y = y_at(value(point));
        d.push_str(&format!(" L {x:.1} {prev_y:.1}"));
    }
    d
}

/// What the hover resolved to, for the readout overlay.
struct HoverReadout {
    /// Crosshair x as a fraction of the full width.
    fx: f32,
    you_hp: u32,
    opp_hp: u32,
    you_y: f32,
    opp_y: f32,
    /// `(name, color, icon)` per side with a chip mark within reach.
    chips: Vec<(String, &'static str, Option<String>)>,
}

/// Cursor x as a fraction of the chart's width, via its live rect.
fn graph_fx(client_x: f64) -> Option<f32> {
    let el = web_sys::window()?.document()?.get_element_by_id("hp-graph-hit")?;
    let rect = el.get_bounding_client_rect();
    Some((((client_x - rect.left()) / rect.width().max(1.0)) as f32).clamp(0.0, 1.0))
}

/// The full chart (the desktop's `hp_match_graph` at height 72):
/// per-round panels + outcome washes, custom bands, zero baseline,
/// step traces, chip lanes, the crosshair/readout on hover, and the
/// desktop's scroll-zoom / drag-pan / double-click-reset. `zoom_key`
/// identifies the match — a change resets the view.
#[component]
pub fn HpMatchGraph(rounds: std::rc::Rc<Vec<CookedHpRound>>, max_hp: f32, zoom_key: String) -> Element {
    let mut hover = use_signal(|| None::<f32>);
    let mut zoom = use_signal(|| 1.0f32);
    let mut offset = use_signal(|| 0.0f32);
    // Last cursor fraction while panning.
    let mut pan = use_signal(|| None::<f32>);
    let mut last_key = use_signal(|| zoom_key.clone());
    if *last_key.peek() != zoom_key {
        last_key.set(zoom_key.clone());
        zoom.set(1.0);
        offset.set(0.0);
        pan.set(None);
    }

    let z = *zoom.read();
    let off = (*offset.read()).clamp(0.0, (XU * (z - 1.0)).max(0.0));
    let segs = segments(&rounds, z);

    // Resolve the hover against the cooked data (the desktop's
    // draw-time hit-test): the containing segment on the virtual
    // timeline, the step value in force at that x, and any chip mark
    // within ~4 units.
    let readout: Option<HoverReadout> = hover.read().and_then(|fx| {
        let vx = fx * XU + off;
        let (i, &(sx, sw)) = segs
            .iter()
            .enumerate()
            .find(|(_, &(sx, sw))| vx >= sx && vx < sx + sw)?;
        let r = &rounds[i];
        let xf = ((vx - sx) / sw.max(1.0)).clamp(0.0, 1.0);
        let (first, last) = (r.trace.first()?, r.trace.last()?);
        if xf < first.0 || xf > last.0 {
            return None;
        }
        let at = r.trace.iter().rev().find(|p| p.0 <= xf)?;
        const NEAR: f32 = 4.0;
        let mut chips = Vec::new();
        for (side, color) in [(0usize, FIELD_RED), (1, FIELD_BLUE)] {
            let nearest = r.chip_uses[side]
                .iter()
                .map(|m| (m, (sx + m.x * sw - vx).abs()))
                .filter(|(_, d)| *d <= NEAR)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((m, _)) = nearest {
                chips.push((m.name.clone(), color, m.icon.clone()));
            }
        }
        Some(HoverReadout {
            fx,
            you_hp: (at.1 * max_hp).round() as u32,
            opp_hp: (at.2 * max_hp).round() as u32,
            you_y: y_at(at.1),
            opp_y: y_at(at.2),
            chips,
        })
    });

    let panning = pan.read().is_some();
    let graph_class = if panning {
        "hp-graph panning"
    } else if z > 1.001 {
        "hp-graph zoomed"
    } else {
        "hp-graph"
    };
    rsx! {
        div {
            class: graph_class,
            onmousemove: move |evt| {
                let Some(fx) = graph_fx(evt.client_coordinates().x) else {
                    return;
                };
                let panning_at = { *pan.peek() };
                if let Some(last) = panning_at {
                    // Pan by the cursor's travel, clamped to the
                    // virtual timeline.
                    let z = *zoom.peek();
                    let next = (*offset.peek() - (fx - last) * XU)
                        .clamp(0.0, (XU * (z - 1.0)).max(0.0));
                    offset.set(next);
                    pan.set(Some(fx));
                }
                hover.set(Some(fx));
            },
            onmousedown: move |evt| {
                if *zoom.peek() > 1.001 {
                    pan.set(graph_fx(evt.client_coordinates().x));
                }
            },
            onmouseup: move |_| pan.set(None),
            onmouseleave: move |_| {
                hover.set(None);
                pan.set(None);
            },
            ondoubleclick: move |_| {
                zoom.set(1.0);
                offset.set(0.0);
                pan.set(None);
            },
            onwheel: move |evt| {
                evt.prevent_default();
                let Some(fx) = graph_fx(evt.client_coordinates().x) else {
                    return;
                };
                // Scroll up = zoom in, anchored so the timeline point
                // under the cursor stays put.
                let steps = match evt.delta() {
                    dioxus::html::geometry::WheelDelta::Pixels(v) => -v.y as f32 * 0.01,
                    dioxus::html::geometry::WheelDelta::Lines(v) => -v.y as f32 * 0.25,
                    dioxus::html::geometry::WheelDelta::Pages(v) => -v.y as f32 * 0.5,
                };
                let old = *zoom.peek();
                let new = (old * steps.exp()).clamp(1.0, 64.0);
                let pos = fx * XU;
                let anchor = (pos + *offset.peek()) * (new / old) - pos;
                zoom.set(new);
                offset.set(anchor.clamp(0.0, (XU * (new - 1.0)).max(0.0)));
            },
            div { id: "hp-graph-hit", class: "hp-graph-hit",
                svg {
                    view_box: "0 0 1000 72",
                    preserve_aspect_ratio: "none",
                    for (i, (sx, sw)) in segs
                        .iter()
                        .map(|&(sx, sw)| (sx - off, sw))
                        .enumerate()
                        // Cull segments fully outside the viewport —
                        // the SVG would clip them anyway, but at deep
                        // zoom the chip-lane ticks add up.
                        .filter(|(_, (sx, sw))| sx + sw > -10.0 && *sx < XU + 10.0)
                    {
                        // Segment panel wash, full height.
                        rect {
                            class: "hp-panel",
                            x: "{sx}", y: "0", width: "{sw}", height: "{GRAPH_H}", rx: "3",
                        }
                        // Outcome tint over it.
                        match rounds[i].outcome {
                            Some(BattleOutcome::Win) => rsx! {
                                rect { class: "hp-tint win", x: "{sx}", y: "0", width: "{sw}", height: "{GRAPH_H}", rx: "3" }
                            },
                            Some(BattleOutcome::Loss) => rsx! {
                                rect { class: "hp-tint loss", x: "{sx}", y: "0", width: "{sw}", height: "{GRAPH_H}", rx: "3" }
                            },
                            _ => rsx! {},
                        }
                        // Custom-screen bands (trace field only).
                        for (x0, x1) in rounds[i].custom.iter().copied() {
                            rect {
                                class: "hp-custom",
                                x: "{sx + x0 * sw}", y: "0",
                                width: "{(x1 - x0).max(0.0) * sw}", height: "{FIELD_H}",
                            }
                        }
                        // Zero baseline.
                        line {
                            class: "hp-baseline",
                            x1: "{sx}", y1: "{y_at(0.0)}", x2: "{sx + sw}", y2: "{y_at(0.0)}",
                        }
                        // Traces: opponent first so the local red wins
                        // overlaps.
                        if rounds[i].trace.len() >= 2 {
                            path { class: "hp-trace opp", d: step_path(&rounds[i].trace, false, sx, sw) }
                            path { class: "hp-trace you", d: step_path(&rounds[i].trace, true, sx, sw) }
                        }
                        // Chip-use lane ticks.
                        for (side, class) in [(0usize, "hp-chip you"), (1, "hp-chip opp")] {
                            for m in rounds[i].chip_uses[side].iter() {
                                line {
                                    class,
                                    x1: "{sx + m.x * sw}", y1: "{lane_y(side) - 3.0}",
                                    x2: "{sx + m.x * sw}", y2: "{lane_y(side) + 3.0}",
                                }
                            }
                        }
                    }
                    // Viewport indicator along the top while zoomed.
                    if z > 1.001 {
                        rect {
                            class: "hp-viewport",
                            x: "{off / z}", y: "0", width: "{XU / z}", height: "2", rx: "1",
                        }
                    }
                }
                // Crosshair + value dots, in the overlay space (percent
                // x, px y — the SVG's x stretch doesn't apply here).
                if let Some(r) = readout.as_ref() {
                    div { class: "hp-crosshair", style: "left: {r.fx * 100.0}%;" }
                    div {
                        class: "hp-dot opp",
                        style: "left: {r.fx * 100.0}%; top: {r.opp_y}px;",
                    }
                    div {
                        class: "hp-dot you",
                        style: "left: {r.fx * 100.0}%; top: {r.you_y}px;",
                    }
                    div {
                        class: if r.fx > 0.7 { "hp-readout flip" } else { "hp-readout" },
                        style: "left: {r.fx * 100.0}%;",
                        div { class: "line",
                            span { class: "dot you" }
                            span { class: "mono", "{r.you_hp}" }
                        }
                        div { class: "line",
                            span { class: "dot opp" }
                            span { class: "mono", "{r.opp_hp}" }
                        }
                        for (name, color, icon) in r.chips.iter() {
                            div { class: "line",
                                span { class: "dot", style: "background: {color};" }
                                if let Some(icon) = icon {
                                    img { class: "chip-icon", src: "{icon}" }
                                }
                                span { "{name}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The minimal strip above the replay scrubber (the desktop's
/// `hp_hover_strip`): traces + custom bands only, one continuous
/// timebase with NO inter-round gaps so every point sits over the
/// scrubber position that seeks to it. Non-interactive.
#[component]
pub fn HpHoverStrip(rounds: std::rc::Rc<Vec<CookedHpRound>>, height: f32) -> Element {
    const SPAD: f32 = 2.0;
    let h = height;
    let sy_at = move |yf: f32| SPAD + (1.0 - yf.clamp(0.0, 1.0)) * (h - 2.0 * SPAD);
    let total: f32 = rounds.iter().map(|r| r.weight.max(1.0)).sum::<f32>().max(1.0);
    let mut start = 0.0f32;
    let mut placed: Vec<(f32, f32, &CookedHpRound)> = Vec::with_capacity(rounds.len());
    for r in rounds.iter() {
        let span = r.weight.max(1.0);
        placed.push((start / total * XU, span / total * XU, r));
        start += span;
    }
    let strip_path = |trace: &[(f32, f32, f32)], you: bool, sx: f32, sw: f32| -> String {
        let value = |p: &(f32, f32, f32)| if you { p.1 } else { p.2 };
        let mut d = String::new();
        let mut prev_y = sy_at(value(&trace[0]));
        d.push_str(&format!("M {:.1} {:.1}", sx + trace[0].0 * sw, prev_y));
        for point in &trace[1..] {
            let x = sx + point.0.clamp(0.0, 1.0) * sw;
            d.push_str(&format!(" L {x:.1} {prev_y:.1}"));
            prev_y = sy_at(value(point));
            d.push_str(&format!(" L {x:.1} {prev_y:.1}"));
        }
        d
    };
    rsx! {
        svg {
            class: "hp-strip",
            style: "height: {h}px;",
            view_box: "0 0 1000 {h}",
            preserve_aspect_ratio: "none",
            for (sx, sw, r) in placed.iter() {
                for (x0, x1) in r.custom.iter().copied() {
                    rect {
                        class: "hp-custom",
                        x: "{sx + x0 * sw}", y: "0",
                        width: "{(x1 - x0).max(0.0) * sw}", height: "{h}",
                    }
                }
                if r.trace.len() >= 2 {
                    path { class: "hp-trace opp", d: strip_path(&r.trace, false, *sx, *sw) }
                    path { class: "hp-trace you", d: strip_path(&r.trace, true, *sx, *sw) }
                }
            }
        }
    }
}

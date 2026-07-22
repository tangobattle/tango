//! The fullscreen session view: the framebuffer canvas, a compact
//! header, the Escape-toggled menu overlay, and the end-of-session
//! overlay (the session itself is already torn down by then; the
//! runtime keeps the end readable until it's dismissed).

use std::sync::atomic::Ordering;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;

use super::{icons, touch, use_ctx, Ctx};
use crate::t;
use crate::platform::input::{self, MappedKey};
use crate::runtime::{FRAME_REV, MENU_OPEN, SESSION_EPOCH};
use crate::session::{SessionEnd, SessionKind};

#[component]
pub fn SessionView() -> Element {
    let Ctx {
        runtime,
        config,
        ..
    } = use_ctx();

    // Attach the presenter once the canvas exists; detach on unmount so
    // the next mount starts from a fresh WebGL context. Reading the
    // filter reactively re-attaches (rebuilding the pipeline) when the
    // setting changes mid-session.
    {
        let runtime = runtime.clone();
        use_effect(move || {
            let filter = config.read().video_filter.clone();
            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("framebuffer"))
                .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok());
            match canvas {
                Some(canvas) => runtime.borrow_mut().attach_canvas(&canvas, &filter),
                None => log::error!("canvas missing"),
            }
        });
    }
    {
        let runtime = runtime.clone();
        use_drop(move || {
            runtime.borrow_mut().detach_canvas();
            // No rescan on the way out: an SRAM write-back may still
            // be in flight, and its completion bumps SAVES_REV
            // itself. (The created save's adoption as the game's pick
            // lives in the app root — signal writes from a dropping
            // scope are unreliable.)
        });
    }

    let lang = crate::i18n::LANG.read().clone();
    // Reactive inputs: per-frame stats, structural session changes, and
    // the Escape-toggled menu.
    let _ = FRAME_REV.read();
    let _ = SESSION_EPOCH.read();
    let menu_open = *MENU_OPEN.read();

    let (title, running, paused, end, kind) = {
        let rt = runtime.borrow();
        let title = rt
            .descriptor()
            .map(|d| crate::library::display_name(d.game))
            .unwrap_or_else(|| "Session".to_string());
        let kind = rt.descriptor().map(|d| d.kind);
        let end = rt.last_end();
        match rt.shared() {
            Some(shared) => {
                let paused = shared.paused.load(Ordering::Relaxed);
                (title, true, paused, end, kind)
            }
            None => (title, false, false, end, kind),
        }
    };

    rsx! {
        document::Title { "{title} — Tango" }
        div { class: "session",
            div { class: "stage",
                // Backing store per scaling mode: native 240x160 for
                // integer mode (pixelated CSS upscale stays square), a
                // 6x nearest-neighbour render for fit mode (the browser
                // then bilinears it to the window — sharp, no shimmer).
                // A video filter always renders at the 6x backing —
                // its magnification happens in the fragment shader, and
                // the LCD grid's screen-space lines need the pixels.
                canvas {
                    id: "framebuffer",
                    width: if config.read().integer_scaling && config.read().video_filter.is_empty() { "240" } else { "1440" },
                    height: if config.read().integer_scaling && config.read().video_filter.is_empty() { "160" } else { "960" },
                    class: if !config.read().integer_scaling { "fit" },
                }
                // Top-right corner commands (the desktop's auto-hide
                // chrome idiom): the gear drops the session menu card,
                // the X tears the session down. Only the end overlay is
                // modal.
                if end.is_none() && running {
                    div { class: "corner-commands",
                        button {
                            class: "btn ghost icon-btn",
                            title: "{t!(&lang, \"web-menu\")} (Esc)",
                            onclick: move |_| {
                                let open = *MENU_OPEN.peek();
                                *MENU_OPEN.write() = !open;
                            },
                            icons::Menu {}
                        }
                        button {
                            class: "btn ghost icon-btn quit",
                            title: "{t!(&lang, \"window-quit\")}",
                            onclick: {
                                let runtime = runtime.clone();
                                move |_| runtime.borrow_mut().close_session()
                            },
                            icons::X {}
                        }
                    }
                    if menu_open {
                        div { class: "card-anchor", SessionMenuCard {} }
                    }
                    // The desktop's per-kind overlays: the replay
                    // transport bar and the PvP telemetry deck.
                    if kind == Some(SessionKind::Replay) {
                        ReplayTransport {}
                    }
                    if kind == Some(SessionKind::Pvp) {
                        TelemetryDeck {}
                        PvpSetupDrawers {}
                    }
                    // Coarse-pointer screens get on-screen controls (CSS
                    // decides; it renders inert elsewhere). They stay put
                    // under an open card — the backdrop (z 7) covers them,
                    // so a touch there dismisses the card instead.
                    touch::TouchControls {}
                }
                if paused && !menu_open && end.is_none() {
                    span { class: "badge pause-badge", {t!(&lang, "playback-pause")} }
                }
            }
            if let Some(end) = end {
                div { class: "overlay",
                    div { class: "overlay-panel results-card",
                        match &end {
                            // The desktop results card, static flavor:
                            // styled headline, the 44px score row, the
                            // draws line.
                            SessionEnd::MatchEnded { wins, losses, draws, stats } => {
                                let (headline, tone) = if wins > losses {
                                    (t!(&lang, "session-results-victory"), "win")
                                } else if wins < losses {
                                    (t!(&lang, "session-results-defeat"), "loss")
                                } else {
                                    (t!(&lang, "session-results-draw"), "draw")
                                };
                                // The results HP graph (the desktop's, at
                                // rest): no plan — each round anchors at its
                                // own first sample — and only when a round
                                // actually has a drawable trace (the
                                // desktop's static fallback drops it too).
                                let chart = stats.as_ref().map(|s| {
                                    let (rounds, max_hp) =
                                        super::hp_chart::cook_hp_rounds(s, None, None);
                                    (std::rc::Rc::new(rounds), max_hp)
                                }).filter(|(rounds, _)| rounds.iter().any(|r| r.trace.len() >= 2));
                                let (wins, losses, draws) = (*wins, *losses, *draws);
                                rsx! {
                                    p { class: "headline {tone}", "{headline}" }
                                    div { class: "score-row",
                                        div { class: "score-side",
                                            span { class: "numeral", "{wins}" }
                                            span { class: "sub", {t!(&lang, "session-results-you")} }
                                        }
                                        span { class: "numeral dash", "–" }
                                        div { class: "score-side",
                                            span { class: "numeral", "{losses}" }
                                            span { class: "sub", {t!(&lang, "play-opponent")} }
                                        }
                                    }
                                    if let Some((rounds, max_hp)) = chart {
                                        div { class: "results-graph",
                                            super::hp_chart::HpMatchGraph {
                                                rounds,
                                                max_hp,
                                                zoom_key: "results".to_string(),
                                                zoomable: false,
                                            }
                                        }
                                    }
                                    if draws > 0 {
                                        p { class: "sub", {t!(&lang, "session-results-draws", count = draws as i64)} }
                                    }
                                }
                            }
                            SessionEnd::Error(e) => rsx! {
                                p { class: "headline draw", "{e}" }
                            },
                            _ => rsx! {},
                        }
                        button {
                            class: "btn primary",
                            onclick: {
                                let runtime = runtime.clone();
                                move |_| runtime.borrow_mut().dismiss_end()
                            },
                            {t!(&lang, "session-results-done")}
                        }
                    }
                }
            }
        }
    }
}

/// The session menu as a card dropped from the chip row.
#[component]
pub fn SessionMenuCard() -> Element {
    let Ctx {
        runtime,
        mut config,
        ..
    } = use_ctx();
    let lang = crate::i18n::LANG.read().clone();

    let (title, caption, is_pvp) = {
        let rt = runtime.borrow();
        let title = rt
            .descriptor()
            .map(|d| crate::library::display_name(d.game))
            .unwrap_or_else(|| "Session".to_string());
        let is_pvp = rt.descriptor().map(|d| d.kind) == Some(SessionKind::Pvp);
        let caption = if is_pvp {
            crate::i18n::t(&crate::i18n::LANG.peek().clone(), "discord-presence-in-progress")
        } else {
            crate::i18n::t(
                &crate::i18n::LANG.peek().clone(),
                "discord-presence-in-single-player",
            )
        };
        (title, caption, is_pvp)
    };

    let volume_pct = (config.read().volume * 100.0).round() as u32;
    let hints = {
        let cfg = config.read();
        let mut hints = vec!["Esc — menu".to_string()];
        if let Some(physical) = cfg.mapping.slot(MappedKey::SpeedUp).first() {
            let (_, label) = input::describe(physical);
            hints.push(format!("hold {label} — fast-forward"));
        }
        hints.join("  ·  ")
    };

    rsx! {
        div { class: "tele-card",
            div { class: "tele-head",
                div {
                    h3 { "{title}" }
                    p { class: "sub", "{caption}" }
                }
                button {
                    class: "btn ghost icon-btn",
                    onclick: move |_| *MENU_OPEN.write() = false,
                    icons::ChevronUp {}
                }
            }
            div { class: "menu-volume",
                label { {t!(&lang, "settings-volume")} " · {volume_pct}%" }
                input {
                    r#type: "range",
                    min: "0",
                    max: "100",
                    value: "{volume_pct}",
                    oninput: move |evt: FormEvent| {
                        if let Ok(v) = evt.value().parse::<f32>() {
                            config.with_mut(|c| c.volume = (v / 100.0).clamp(0.0, 1.0));
                        }
                    },
                }
            }
            div { class: "menu-actions",
                // The console's reset button — solo only: one side
                // rebooting isn't an input a netplay link could replay.
                if !is_pvp {
                    button {
                        class: "btn",
                        onclick: {
                            let runtime = runtime.clone();
                            move |_| {
                                runtime.borrow_mut().reset_session();
                                *MENU_OPEN.write() = false;
                            }
                        },
                        "Reset"
                    }
                }
                button {
                    class: "btn danger",
                    onclick: {
                        let runtime = runtime.clone();
                        move |_| runtime.borrow_mut().close_session()
                    },
                    {t!(&lang, "window-quit")}
                }
            }
            p { class: "hint", "{hints}" }
        }
    }
}

/// A scrub drag in flight: the previewed tick, whether playback was
/// running when it started (the commit resumes), and whether a
/// keyframe preview has replaced the live frame yet (until it has,
/// the live frame beats a farther keyframe — the desktop's `blitted`
/// hysteresis).
#[derive(Clone, Copy)]
struct ScrubDrag {
    preview: u32,
    resume: bool,
    blitted: bool,
}

/// Cursor x within the scrub track → (x, track width), via the
/// track's live bounding rect (the only place its layout width is
/// knowable).
fn scrub_metrics(client_x: f64) -> Option<(f64, f64)> {
    let el = web_sys::window()?.document()?.get_element_by_id("scrub-track")?;
    let rect = el.get_bounding_client_rect();
    Some((client_x - rect.left(), rect.width()))
}

/// A keyframe's native BGR555 frame as a PNG data URL (the hover
/// thumbnail). 5-bit channels expand as `(c << 3) | (c >> 2)` so white
/// maps to 255.
fn bgr555_thumb_url(fb: &[u8]) -> Option<String> {
    let mut img = image::RgbaImage::new(240, 160);
    for (i, px) in img.pixels_mut().enumerate() {
        let v = u16::from_le_bytes([fb[i * 2], fb[i * 2 + 1]]);
        let r = (v & 0x1f) as u8;
        let g = ((v >> 5) & 0x1f) as u8;
        let b = ((v >> 10) & 0x1f) as u8;
        *px = image::Rgba([(r << 3) | (r >> 2), (g << 3) | (g >> 2), (b << 3) | (b >> 2), 0xff]);
    }
    crate::save_view::png_data_url(&img)
}

/// The replay transport bar (the desktop's `replay_bar`): scrubber on
/// top, then play/pause + `cur / total` readout with the toggle chips
/// right — speed menu, input display, PiP, swap. Pause parks the
/// pump's pacing; seeks chase cooperatively in the pump.
#[component]
fn ReplayTransport() -> Element {
    let Ctx {
        runtime,
        mut config,
        storage,
        library,
        ..
    } = use_ctx();
    let lang = crate::i18n::LANG.read().clone();
    // Per-drag / per-hover scrub state, plus the speed dropdown.
    let mut drag = use_signal(|| None::<ScrubDrag>);
    let mut hover = use_signal(|| None::<u32>);
    // The hover thumbnail, cached by the backing keyframe's tick so
    // cursor moves within one keyframe reuse the PNG.
    let mut thumb = use_signal(|| None::<(u32, String)>);
    let mut speed_open = use_signal(|| false);
    // Clip tools: the scissors strip, its marks, and the last export's
    // quiet outcome line.
    let mut tools_open = use_signal(|| false);
    let mut marks = use_signal(|| (None::<u32>, None::<u32>));
    let mut clip_status = use_signal(|| None::<Result<(), String>>);
    // The analysis strip above the scrubber: cooked from the CACHED
    // stats sidecar only (a re-sim here would fight the running
    // emulation for the one thread; the replays tab computes). `None`
    // = not looked up yet; empty = no sidecar.
    let mut strip_rounds = use_signal(|| None::<std::rc::Rc<Vec<super::hp_chart::CookedHpRound>>>);
    let mut bar_hover = use_signal(|| false);
    {
        let runtime = runtime.clone();
        use_effect(move || {
            if strip_rounds.peek().is_some() {
                return;
            }
            let Some((file, planned)) = ({
                let rt = runtime.borrow();
                match (rt.replay_source_file(), rt.replay_scrub()) {
                    (Some(file), Some(scrub)) => {
                        // The scrubber's exact tick scale: spans between
                        // round boundaries.
                        let mut starts = vec![0u32];
                        starts.extend(scrub.round_boundaries.iter().copied());
                        let total = scrub.total.max(1);
                        let planned: Vec<u32> = starts
                            .iter()
                            .enumerate()
                            .map(|(i, &s)| {
                                starts.get(i + 1).copied().unwrap_or(total).saturating_sub(s).max(1)
                            })
                            .collect();
                        Some((file, planned))
                    }
                    _ => None,
                }
            }) else {
                return;
            };
            let storage = storage.peek().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                let rounds = match crate::analysis::cached_stats(&storage, &file).await {
                    Some(stats) => {
                        let (rounds, _) =
                            super::hp_chart::cook_hp_rounds(&stats, None, Some(&planned));
                        std::rc::Rc::new(rounds)
                    }
                    None => std::rc::Rc::new(Vec::new()),
                };
                strip_rounds.set(Some(rounds));
            });
        });
    }
    let Some((shared, paused, speed, scrub)) = ({
        let rt = runtime.borrow();
        match (rt.shared(), rt.replay_scrub()) {
            (Some(shared), Some(scrub)) => Some((
                shared.clone(),
                shared.paused.load(Ordering::Relaxed),
                shared.speed.load(Ordering::Relaxed),
                scrub,
            )),
            _ => None,
        }
    }) else {
        return rsx! {};
    };
    let total = scrub.total.max(1);
    // The playhead everything reads: drag preview, else an in-flight
    // seek's target (no snap-back while the chase catches up), else
    // the cursor.
    let playhead = drag()
        .map(|d| d.preview)
        .or(scrub.pending)
        .unwrap_or(scrub.cursor)
        .min(total);
    // Mid-drag (and mid-chase-that-will-resume) the session is
    // logically still playing; a Play glyph there reads as a stuck
    // pause.
    let logically_playing = drag().map(|d| d.resume).unwrap_or(false) || scrub.will_resume;
    let show_paused = paused && !logically_playing;
    let show_inputs = config.read().show_replay_inputs;
    let show_pip = config.read().show_opponent_pip;
    let shared_toggle = shared.clone();
    let shared_swap = shared.clone();

    let tick_at = move |client_x: f64| -> Option<u32> {
        let (x, w) = scrub_metrics(client_x)?;
        Some((((x / w.max(1.0)).clamp(0.0, 1.0) * total as f64).round() as u32).min(total))
    };

    // Track geometry, all in percent of the full track. Segments are
    // the desktop's chapter treatment: a small gap at each recorded
    // round boundary.
    let pct = |t: u32| t as f64 / total as f64 * 100.0;
    let mut edges: Vec<u32> = vec![0];
    for &b in &scrub.round_boundaries {
        if b > 0 && b < total {
            edges.push(b);
        }
    }
    edges.push(total);
    let segments: Vec<(f64, f64, f64)> = edges
        .windows(2)
        .map(|pair| {
            let (s, e) = (pair[0], pair[1]);
            // Fill fraction inside this segment, 0..100.
            let fill = ((playhead.min(e).saturating_sub(s)) as f64 / (e - s).max(1) as f64) * 100.0;
            (pct(s), pct(e) - pct(s), fill)
        })
        .collect();
    let seg_count = segments.len();

    let on_track_down = {
        let runtime = runtime.clone();
        let shared = shared.clone();
        move |evt: PointerEvent| {
            let Some(t) = tick_at(evt.client_coordinates().x) else {
                return;
            };
            let was_paused = shared.paused.load(Ordering::Relaxed);
            shared.paused.store(true, Ordering::Release);
            // A press previews only an exact keyframe — blitting the
            // nearest would flash a wrong frame until the chase lands.
            let blitted = runtime.borrow().replay_scrub_preview(t, true);
            drag.set(Some(ScrubDrag {
                preview: t,
                resume: !was_paused,
                blitted,
            }));
            hover.set(None);
        }
    };
    let on_shield_move = {
        let runtime = runtime.clone();
        move |evt: PointerEvent| {
            let Some(mut d) = *drag.peek() else { return };
            let Some(t) = tick_at(evt.client_coordinates().x) else {
                return;
            };
            if t == d.preview {
                return;
            }
            d.preview = t;
            let rt = runtime.borrow();
            // Nearest-keyframe scrub feedback, with the live frame
            // winning until the first blit.
            let blit = d.blitted
                || rt
                    .replay_nearest_keyframe(t)
                    .is_some_and(|n| n.abs_diff(t) <= scrub.cursor.abs_diff(t));
            if blit && rt.replay_scrub_preview(t, false) {
                d.blitted = true;
            }
            drag.set(Some(d));
        }
    };
    let on_shield_up = {
        let runtime = runtime.clone();
        move |_| {
            let Some(d) = *drag.peek() else { return };
            runtime.borrow().replay_seek(d.preview, d.resume);
            drag.set(None);
        }
    };

    // The strip expands while the transport is engaged (the desktop's
    // bar_engaged), collapsing — not unmounting — otherwise.
    let engaged = bar_hover() || speed_open() || drag.read().is_some();
    let strip = strip_rounds.read().clone().filter(|r| !r.is_empty());
    let strip_h: f32 = if engaged && strip.is_some() { 40.0 } else { 0.0 };

    rsx! {
        div {
            class: "transport-bar hud-chip",
            onmouseenter: move |_| bar_hover.set(true),
            onmouseleave: move |_| bar_hover.set(false),
            // --- analysis strip (traces + custom bands over the
            // scrubber's exact tick scale) ---
            div { class: "strip-slot", style: "height: {strip_h}px;",
                if let Some(rounds) = strip {
                    super::hp_chart::HpHoverStrip { rounds, height: 40.0 }
                }
            }
            // --- scrubber ---
            div {
                id: "scrub-track",
                class: "scrub-track",
                onpointerdown: on_track_down,
                onpointermove: {
                    let runtime = runtime.clone();
                    move |evt: PointerEvent| {
                        if drag.peek().is_some() {
                            return;
                        }
                        let t = tick_at(evt.client_coordinates().x);
                        hover.set(t);
                        // Refresh the floating thumbnail from the
                        // nearest keyframe, re-encoding only when the
                        // backing keyframe actually changes.
                        if let Some(t) = t {
                            if let Some((kt, fb)) = runtime.borrow().replay_keyframe_frame(t) {
                                if thumb.peek().as_ref().map(|(t2, _)| *t2) != Some(kt) {
                                    if let Some(url) = bgr555_thumb_url(&fb) {
                                        thumb.set(Some((kt, url)));
                                    }
                                }
                            }
                        }
                    }
                },
                onpointerleave: move |_| hover.set(None),
                for (i, (left, width, fill)) in segments.into_iter().enumerate() {
                    div {
                        class: "scrub-seg",
                        style: format!(
                            "left: calc({left}% + {}px); width: calc({width}% - {}px);",
                            if i == 0 { 0.0 } else { 1.5 },
                            match (i == 0, i == seg_count - 1) {
                                (true, true) => 0.0,
                                (true, false) | (false, true) => 1.5,
                                (false, false) => 3.0,
                            },
                        ),
                        div { class: "scrub-fill", style: "width: {fill}%;" }
                    }
                }
                // Clip selection: a translucent band between the marks
                // plus a notch per mark — over the fills, under the
                // playhead.
                if let (Some(a), Some(b)) = *marks.read() {
                    div {
                        class: "scrub-clip-band",
                        style: "left: {pct(a)}%; width: {pct(b) - pct(a)}%;",
                    }
                }
                for m in [marks.read().0, marks.read().1].into_iter().flatten() {
                    div { class: "scrub-mark", style: "left: {pct(m)}%;" }
                }
                if let Some(h) = *hover.read() {
                    div { class: "scrub-ghost", style: "left: {pct(h)}%;" }
                    // The floating keyframe thumbnail + timestamp,
                    // clamped inside the bar; the stamp rides alone
                    // when no keyframe is in reach yet.
                    if let Some((_, url)) = thumb.read().as_ref() {
                        div {
                            class: "scrub-thumb hud-chip",
                            style: "left: clamp(97px, {pct(h)}%, calc(100% - 97px));",
                            img { src: "{url}" }
                            span { class: "mono", {crate::session::format_tick(h)} }
                        }
                    } else {
                        div { class: "scrub-stamp mono", style: "left: {pct(h)}%;",
                            {crate::session::format_tick(h)}
                        }
                    }
                }
                div {
                    class: if drag.read().is_some() { "scrub-handle dragging" } else { "scrub-handle" },
                    style: "left: {pct(playhead)}%;",
                }
            }
            // Full-viewport drag shield: pointer capture the HTML way —
            // moves and the release keep landing here even when the
            // cursor leaves the bar.
            if drag.read().is_some() {
                div {
                    class: "scrub-shield",
                    onpointermove: on_shield_move,
                    onpointerup: on_shield_up,
                }
            }
            // --- clip strip: mark stamps + export, swapped wholesale
            // for the running export's progress line ---
            if tools_open() {
                if let Some(p) = *crate::export::EXPORT_PROGRESS.read() {
                    div { class: "clip-strip",
                        span { class: "sub",
                            if *crate::export::EXPORT_CANCEL.read() {
                                {t!(&lang, "replays-export-cancelling")}
                            } else {
                                {t!(&lang, "replays-export-progress")}
                                " {p.frame * 100 / p.total.max(1)}%"
                            }
                        }
                        div { class: "clip-progress",
                            div {
                                class: "clip-progress-fill",
                                style: "width: {p.frame * 100 / p.total.max(1)}%;",
                            }
                        }
                        button {
                            class: "btn chip",
                            title: t!(&lang, "replays-export-cancel"),
                            onclick: move |_| *crate::export::EXPORT_CANCEL.write() = true,
                            icons::X {}
                        }
                    }
                } else {
                    div { class: "clip-strip",
                        // Stamping past the other mark drops it — the
                        // pair can never invert.
                        button {
                            class: if marks.read().0.is_some() { "btn chip active" } else { "btn chip" },
                            title: t!(&lang, "playback-clip-start"),
                            onclick: move |_| {
                                marks.with_mut(|m| {
                                    m.0 = Some(playhead);
                                    if m.1.is_some_and(|o| o <= playhead) {
                                        m.1 = None;
                                    }
                                });
                            },
                            icons::ArrowRightFromLine {}
                        }
                        span { class: if marks.read().0.is_some() { "clip-stamp mono set" } else { "clip-stamp mono" },
                            {marks.read().0.map(crate::session::format_tick).unwrap_or_else(|| "–:––".to_string())}
                        }
                        button {
                            class: if marks.read().1.is_some() { "btn chip active" } else { "btn chip" },
                            title: t!(&lang, "playback-clip-end"),
                            onclick: move |_| {
                                marks.with_mut(|m| {
                                    m.1 = Some(playhead);
                                    if m.0.is_some_and(|i| i >= playhead) {
                                        m.0 = None;
                                    }
                                });
                            },
                            icons::ArrowRightToLine {}
                        }
                        span { class: if marks.read().1.is_some() { "clip-stamp mono set" } else { "clip-stamp mono" },
                            {marks.read().1.map(crate::session::format_tick).unwrap_or_else(|| "–:––".to_string())}
                        }
                        if let (Some(a), Some(b)) = *marks.read() {
                            span { class: "sub",
                                "({crate::session::format_tick(b - a)})"
                            }
                        }
                        div { class: "grow" }
                        match &*clip_status.read() {
                            Some(Ok(())) => rsx! {
                                span { class: "sub", {t!(&lang, "replays-export-success")} }
                            },
                            Some(Err(e)) => rsx! {
                                span { class: "sub", title: "{e}",
                                    {t!(&lang, "replays-export-error", error = e.clone())}
                                }
                            },
                            None => rsx! {},
                        }
                        button {
                            class: "btn chip",
                            title: t!(&lang, "playback-clip-clear"),
                            disabled: marks.read().0.is_none() && marks.read().1.is_none(),
                            onclick: move |_| marks.set((None, None)),
                            icons::Delete {}
                        }
                        button {
                            class: "btn primary clip-export",
                            disabled: !matches!(*marks.read(), (Some(a), Some(b)) if a < b),
                            onclick: {
                                let runtime = runtime.clone();
                                move |_| {
                                    let (Some(a), Some(b)) = *marks.peek() else {
                                        return;
                                    };
                                    if crate::export::EXPORT_PROGRESS.peek().is_some() {
                                        return;
                                    }
                                    let Some(file) = runtime.borrow().replay_source_file() else {
                                        return;
                                    };
                                    let storage_v = storage.read().clone().flatten();
                                    let lib = library.read().clone().flatten();
                                    clip_status.set(None);
                                    spawn(async move {
                                        let stem = format!(
                                            "{}-clip",
                                            file.strip_suffix(".tangoreplay").unwrap_or(&file)
                                        );
                                        let result: anyhow::Result<bool> = async {
                                            // Ask for the destination inside the
                                            // click's user activation, same as the
                                            // replays tab's export.
                                            let target = if crate::export::save_picker_available() {
                                                match crate::export::pick_save_file(&format!("{stem}.webm")).await? {
                                                    Some(handle) => crate::export::ExportTarget::Picked(handle),
                                                    None => return Ok(false),
                                                }
                                            } else {
                                                let Some(storage) = storage_v.clone() else {
                                                    anyhow::bail!("storage unavailable");
                                                };
                                                crate::export::ExportTarget::OpfsTemp(storage)
                                            };
                                            let (replay, local_rom, remote_rom) =
                                                crate::ui::replays::load_pair(storage_v, lib, &file).await?;
                                            crate::export::export_replay(
                                                replay,
                                                local_rom,
                                                remote_rom,
                                                stem,
                                                target,
                                                Some((a, b)),
                                            )
                                            .await?;
                                            Ok(true)
                                        }
                                        .await;
                                        match result {
                                            Ok(true) => clip_status.set(Some(Ok(()))),
                                            Ok(false) => {}
                                            Err(e) => clip_status.set(Some(Err(format!("{e:#}")))),
                                        }
                                    });
                                }
                            },
                            {t!(&lang, "playback-clip-export")}
                        }
                    }
                }
            }
            // --- transport row ---
            div { class: "transport-row",
                button {
                    class: if show_paused { "btn round primary" } else { "btn round" },
                    title: if show_paused { t!(&lang, "playback-play") } else { t!(&lang, "playback-pause") },
                    onclick: move |_| {
                        if shared_toggle.paused.load(Ordering::Relaxed) {
                            shared_toggle.resume();
                        } else {
                            shared_toggle.paused.store(true, Ordering::Release);
                        }
                    },
                    if show_paused {
                        icons::Play {}
                    } else {
                        icons::Pause {}
                    }
                }
                span { class: "readout mono", {crate::session::format_tick(playhead)} }
                span { class: "readout-sep", "/" }
                span { class: "readout-total mono", {crate::session::format_tick(total)} }
                div { class: "grow" }
                // Clip tools behind one scissors toggle, so the
                // resting bar stays a transport rather than an editor.
                button {
                    class: if tools_open() { "btn chip active" } else { "btn chip" },
                    title: t!(&lang, "playback-clip-tools"),
                    onclick: move |_| tools_open.set(!tools_open()),
                    icons::Scissors {}
                }
                // Speed: the desktop's Gauge dropdown, lit off-realtime.
                div { class: "menu-anchor",
                    button {
                        class: if speed != 100 || speed_open() { "btn chip active" } else { "btn chip" },
                        title: t!(&lang, "playback-speed"),
                        onclick: move |_| speed_open.set(!speed_open()),
                        icons::Gauge {}
                    }
                    if speed_open() {
                        div { class: "menu-backdrop", onclick: move |_| speed_open.set(false) }
                        div { class: "speed-menu",
                            for (label, pct) in [("0.5×", 50u32), ("1×", 100), ("2×", 200), ("4×", 400)] {
                                button {
                                    class: if speed == pct { "menu-item active" } else { "menu-item" },
                                    onclick: {
                                        let shared = shared.clone();
                                        move |_| {
                                            shared.speed.store(pct, Ordering::Relaxed);
                                            speed_open.set(false);
                                        }
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }
                // The recorded joypads, persisted like the desktop's toggle.
                button {
                    class: if show_inputs { "btn chip active" } else { "btn chip" },
                    title: t!(&lang, "playback-input-display"),
                    onclick: move |_| {
                        config.with_mut(|c| c.show_replay_inputs = !c.show_replay_inputs);
                    },
                    icons::Gamepad2 {}
                }
                // The other player's screen, persisted like the desktop's.
                button {
                    class: if show_pip { "btn chip active" } else { "btn chip" },
                    title: t!(&lang, "playback-pip"),
                    onclick: move |_| {
                        config.with_mut(|c| c.show_opponent_pip = !c.show_opponent_pip);
                    },
                    icons::PictureInPicture2 {}
                }
                // Swap perspective: the driver re-reads view_player every
                // tick, so this flips the presented screen (and audio) live.
                button {
                    class: "btn chip",
                    title: t!(&lang, "playback-swap-perspective"),
                    onclick: move |_| {
                        let v = shared_swap.view_player.load(Ordering::Relaxed);
                        shared_swap.view_player.store(1 - v.min(1), Ordering::Relaxed);
                    },
                    icons::ArrowLeftRight {}
                }
            }
        }
        if show_inputs {
            ReplayInputPads { playhead }
        }
        if show_pip {
            ReplayPip {}
        }
    }
}

/// The other player's screen, top-right (the desktop's PiP): a second
/// canvas driven by its own presenter over the replay driver's vbuf2.
#[component]
fn ReplayPip() -> Element {
    let Ctx { runtime, .. } = use_ctx();
    {
        let runtime = runtime.clone();
        use_effect(move || {
            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("pip-framebuffer"))
                .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok());
            match canvas {
                Some(canvas) => runtime.borrow_mut().attach_pip_canvas(&canvas),
                None => log::error!("pip canvas missing"),
            }
        });
    }
    {
        let runtime = runtime.clone();
        use_drop(move || {
            runtime.borrow_mut().detach_pip_canvas();
        });
    }
    rsx! {
        div { class: "pip-frame hud-chip",
            canvas { id: "pip-framebuffer", width: "240", height: "160" }
        }
    }
}

/// The recorded joypads (the desktop's `input_display_overlay`): the
/// viewed player bottom-left, the other side bottom-right, each pad
/// lighting the keys held at the playhead — which follows scrub
/// previews and in-flight seeks, same as the transport readout.
#[component]
fn ReplayInputPads(playhead: u32) -> Element {
    let Ctx { runtime, .. } = use_ctx();
    let Some((inputs, view)) = ({
        let rt = runtime.borrow();
        match (rt.replay_inputs(), rt.shared()) {
            (Some(inputs), Some(shared)) => Some((
                inputs,
                shared.view_player.load(Ordering::Relaxed).min(1),
            )),
            _ => None,
        }
    }) else {
        return rsx! {};
    };
    let idx = (playhead as usize).saturating_sub(1).min(inputs.len().saturating_sub(1));
    let Some(pair) = inputs.get(idx).copied() else {
        return rsx! {};
    };
    let (left, right) = (pair[view], pair[1 - view]);
    rsx! {
        div { class: "input-pads",
            {input_pad(left)}
            div { class: "grow" }
            {input_pad(right)}
        }
    }
}

/// One joypad face (the desktop's `input_pad`, HTML flavor): shoulders,
/// the D-pad cross + Start/Select pills, and the B/A diagonal — held
/// keys light primary. mGBA key bits: A 0x001, B 0x002, Select 0x004,
/// Start 0x008, Right 0x010, Left 0x020, Up 0x040, Down 0x080, R 0x100,
/// L 0x200.
fn input_pad(joyflags: u32) -> Element {
    let on = |bit: u32| if joyflags & bit != 0 { "on" } else { "" };
    rsx! {
        div { class: "input-pad hud-chip",
            div { class: "shoulders",
                span { class: "shoulder {on(0x200)}", "L" }
                div { class: "grow" }
                span { class: "shoulder {on(0x100)}", "R" }
            }
            div { class: "face",
                div { class: "left-col",
                    div { class: "dpad",
                        span { class: "arm blank" }
                        span { class: "arm up {on(0x040)}", "▲" }
                        span { class: "arm blank" }
                        span { class: "arm left {on(0x020)}", "◀" }
                        span { class: "arm hub" }
                        span { class: "arm right {on(0x010)}", "▶" }
                        span { class: "arm blank" }
                        span { class: "arm down {on(0x080)}", "▼" }
                        span { class: "arm blank" }
                    }
                    div { class: "pills",
                        span { class: "pill start {on(0x008)}", "START" }
                        span { class: "pill select {on(0x004)}", "SELECT" }
                    }
                }
                div { class: "grow" }
                div { class: "ab",
                    span { class: "round-key b {on(0x002)}", "B" }
                    span { class: "round-key a {on(0x001)}", "A" }
                }
            }
        }
    }
}

/// The in-match setup drawers (the desktop's docked sidebars): the red
/// left handle opens your own setup, the blue right handle the
/// opponent's (absent when they went in blind). Each drawer embeds the
/// read-only save view, built lazily on first open from the boot
/// inputs the session retained.
#[component]
fn PvpSetupDrawers() -> Element {
    let Ctx {
        runtime,
        config,
        storage,
        ..
    } = use_ctx();
    let lang = crate::i18n::LANG.read().clone();
    let mut open_self = use_signal(|| false);
    let mut open_remote = use_signal(|| false);
    let handle_self = use_signal(|| Option::<crate::save_view::SaveHandle>::None);
    let handle_remote = use_signal(|| Option::<crate::save_view::SaveHandle>::None);
    // Settings → Netplay's auto-open: once per session.
    let mut auto_opened = use_signal(|| false);

    let Some((setup_local, setup_remote)) = runtime.borrow().pvp_setup() else {
        return rsx! {};
    };
    let has_remote = setup_remote.is_some();

    // Build a side's save view on first open (icon baking is a one-off
    // ~100ms; the desktop pays it at boot instead). The slot guard makes
    // repeat calls free; `building` guards the async gap so per-frame
    // re-renders can't stack duplicate builds.
    let building = use_hook(|| std::rc::Rc::new(std::cell::Cell::new((false, false))));
    let build = {
        let building = building.clone();
        move |side: crate::session::pvp::SetupSide,
              mut slot: Signal<Option<crate::save_view::SaveHandle>>,
              is_remote: bool| {
            if slot.peek().is_some() {
                return;
            }
            let flags = building.get();
            if (is_remote && flags.1) || (!is_remote && flags.0) {
                return;
            }
            building.set(if is_remote {
                (flags.0, true)
            } else {
                (true, flags.1)
            });
            let storage = storage.peek().clone().flatten();
            spawn(async move {
                let overrides = match (&storage, &side.patch) {
                    (Some(storage), Some((name, ver))) => {
                        crate::patches::version_overrides(storage, name, ver).await
                    }
                    _ => Default::default(),
                };
                if let Ok(l) = crate::save_view::Loaded::build(
                    side.game,
                    &side.rom,
                    String::new(),
                    &side.save,
                    side.patch.clone(),
                    overrides,
                ) {
                    slot.set(Some(crate::save_view::SaveHandle(std::rc::Rc::new(
                        std::cell::RefCell::new(l),
                    ))));
                }
            });
        }
    };

    // Settings → Netplay's auto-open, once per session.
    {
        let build = build.clone();
        let setup_remote = setup_remote.clone();
        use_effect(move || {
            if config.read().show_opponent_setup && !*auto_opened.peek() {
                if let Some(side) = setup_remote.clone() {
                    auto_opened.set(true);
                    open_remote.set(true);
                    build(side, handle_remote, true);
                }
            }
        });
    }

    rsx! {
        // Edge handles: red = you (left), blue = opponent (right).
        button {
            class: if open_self() { "setup-handle left open" } else { "setup-handle left" },
            title: t!(&lang, "session-self"),
            onclick: {
                let setup_local = setup_local.clone();
                let build = build.clone();
                move |_| {
                    let now_open = !open_self();
                    open_self.set(now_open);
                    if now_open {
                        build(setup_local.clone(), handle_self, false);
                    }
                }
            },
            if open_self() { "◀" } else { "▶" }
        }
        if has_remote {
            button {
                class: if open_remote() { "setup-handle right open" } else { "setup-handle right" },
                title: t!(&lang, "session-opponent"),
                onclick: {
                    let setup_remote = setup_remote.clone();
                    let build = build.clone();
                    move |_| {
                        let now_open = !open_remote();
                        open_remote.set(now_open);
                        if now_open {
                            if let Some(side) = setup_remote.clone() {
                                build(side, handle_remote, true);
                            }
                        }
                    }
                },
                if open_remote() { "▶" } else { "◀" }
            }
        }
        if open_self() {
            div { class: "setup-drawer left",
                if let Some(handle) = handle_self() {
                    crate::save_view::SaveView { handle, editable: false }
                }
            }
        }
        if open_remote() {
            div { class: "setup-drawer right",
                if let Some(handle) = handle_remote() {
                    crate::save_view::SaveView { handle, editable: false }
                }
            }
        }
    }
}

/// One telemetry history sample; the deck keeps the desktop's 180-deep
/// rolling window.
struct Sample {
    tps: f32,
    skew: i32,
    lead: i64,
    depth: u32,
    ping: Option<f32>,
}

const METRIC_HISTORY_LEN: usize = 180;

/// The PvP telemetry deck (the desktop's bottom-right chip + panel):
/// collapsed, a skew-toned signal chip; expanded, the five metric cards
/// with sparklines and the live frame-delay slider. Visible from match
/// start — the desktop's `latency()` reads `Some(ZERO)` until the
/// first sample, so its panel never waits on a pong; only a dropped
/// link hides it.
#[component]
fn TelemetryDeck() -> Element {
    let Ctx {
        runtime, mut config, ..
    } = use_ctx();
    let lang = crate::i18n::LANG.read().clone();
    let mut expanded = use_signal(|| false);
    // History rides a plain ring (not a signal): the deck already
    // re-renders per frame via the parent's FRAME_REV read, and a
    // reactive write per frame would double the render rate.
    let history = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new((0u32, std::collections::VecDeque::<Sample>::new()))));

    let Some((shared, stats, local_player)) = ({
        let rt = runtime.borrow();
        match (rt.shared(), rt.descriptor()) {
            (Some(shared), Some(d)) => Some((shared.clone(), shared.stats.lock().unwrap().clone(), d.local_player)),
            _ => None,
        }
    }) else {
        return rsx! {};
    };
    // The desktop's pre-first-sample reading: zero, not hidden. (An
    // earlier gate here returned nothing until the first ack-derived
    // sample — which hid the deck for the whole match if sampling ever
    // hiccuped, and was stricter than the desktop.)
    let ping = stats.rtt_ms.unwrap_or(0.0);

    // One sample per simulated tick, newest pinned right.
    {
        let mut h = history.borrow_mut();
        if h.0 != stats.frontier {
            h.0 = stats.frontier;
            let lead = stats.frontier as i64 - stats.confirmed as i64;
            h.1.push_back(Sample {
                tps: stats.tps,
                skew: stats.skew,
                lead,
                depth: stats.rolled_back,
                ping: stats.rtt_ms,
            });
            while h.1.len() > METRIC_HISTORY_LEN {
                h.1.pop_front();
            }
        }
    }

    let skew = stats.skew;
    let skew_mag = skew.unsigned_abs();
    let tone_of = |good: bool, warn: bool| if good { "good" } else if warn { "warn" } else { "bad" };

    if !expanded() {
        // Collapsed: the signal-bars chip, toned by skew.
        return rsx! {
            div { class: "telemetry-anchor",
                button {
                    class: "btn round hud-chip-btn tone-{tone_of(skew_mag <= 3, skew_mag <= 7)}",
                    title: "{skew:+}",
                    onclick: move |_| expanded.set(true),
                    if skew_mag <= 3 {
                        icons::SignalHigh {}
                    } else if skew_mag <= 7 {
                        icons::SignalMedium {}
                    } else {
                        icons::SignalLow {}
                    }
                }
            }
        };
    }

    let h = history.borrow();
    let target = stats.fps_target.max(1.0);
    let lead = stats.frontier as i64 - stats.confirmed as i64;
    let frame_delay = shared.present_delay.load(Ordering::Relaxed).min(10);
    let cards = [
        (
            rsx! { icons::Gauge {} },
            t!(&lang, "session-stat-tps"),
            spark(&h.1, |s| Some(s.tps), target - 8.0, target + 2.0),
            format!("{:.0}", stats.tps),
            tone_of(stats.tps >= target - 1.0, stats.tps >= target - 5.0),
        ),
        (
            rsx! { icons::ArrowLeftRight {} },
            t!(&lang, "session-stat-skew"),
            spark(&h.1, |s| Some(s.skew as f32), -8.0, 8.0),
            format!("{skew:+}"),
            tone_of(skew_mag <= 3, skew_mag <= 7),
        ),
        (
            rsx! { icons::Footprints {} },
            t!(&lang, "session-stat-lead"),
            spark(&h.1, |s| Some(s.lead as f32), -24.0, 24.0),
            format!("{lead:+}"),
            tone_of(lead.unsigned_abs() <= 8, lead.unsigned_abs() <= 16),
        ),
        (
            rsx! { icons::GitMerge {} },
            t!(&lang, "session-stat-depth"),
            spark(&h.1, |s| Some(s.depth as f32), 0.0, 8.0),
            format!("{}", stats.rolled_back),
            tone_of(stats.rolled_back <= 2, stats.rolled_back <= 5),
        ),
        (
            rsx! { icons::Wifi {} },
            t!(&lang, "session-stat-ping"),
            spark(&h.1, |s| s.ping, 0.0, 200.0),
            format!("{:.0} ms", ping),
            tone_of(ping < 80.0, ping < 140.0),
        ),
    ];
    let shared_fd = shared.clone();

    rsx! {
        div { class: "telemetry-anchor",
            div { class: "telemetry-panel hud-chip",
                div { class: "tele-header",
                    span { class: "seat", "P{local_player + 1}" }
                    span { class: "sub", {t!(&lang, "play-you")} }
                    div { class: "grow" }
                    button {
                        class: "btn ghost icon-btn",
                        onclick: move |_| expanded.set(false),
                        icons::ChevronUp {}
                    }
                }
                for (icon, label, sparkline, value, tone) in cards {
                    div { class: "tele-card",
                        div { class: "caption-row",
                            span { class: "icon", {icon} }
                            span { class: "sub", "{label}" }
                        }
                        div { class: "value-row",
                            {sparkline}
                            span { class: "value mono tone-{tone}", "{value}" }
                        }
                    }
                }
                div { class: "tele-card",
                    div { class: "caption-row",
                        span { class: "sub", {t!(&lang, "settings-netplay-frame-delay")} }
                    }
                    div { class: "value-row",
                        input {
                            r#type: "range",
                            min: "0",
                            max: "10",
                            value: "{frame_delay}",
                            oninput: move |evt: FormEvent| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    let v = v.min(10);
                                    // Applied live by the PvP driver next
                                    // tick; persisted for the next match.
                                    shared_fd.present_delay.store(v, Ordering::Relaxed);
                                    config.with_mut(|c| c.present_delay = v);
                                }
                            },
                        }
                        span { class: "value mono", "{frame_delay}" }
                    }
                }
            }
        }
    }
}

/// A 180-slot sparkline as an inline SVG polyline, newest pinned right;
/// `None` readings break the line.
fn spark(
    history: &std::collections::VecDeque<Sample>,
    pick: impl Fn(&Sample) -> Option<f32>,
    min: f32,
    max: f32,
) -> Element {
    const W: f32 = 160.0;
    const H: f32 = 24.0;
    let span = (max - min).max(f32::EPSILON);
    let dx = W / (METRIC_HISTORY_LEN - 1) as f32;
    // Newest sample rides the right edge.
    let offset = METRIC_HISTORY_LEN.saturating_sub(history.len());
    let mut runs: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, s) in history.iter().enumerate() {
        match pick(s) {
            Some(v) => {
                let x = (offset + i) as f32 * dx;
                let y = H - ((v - min) / span).clamp(0.0, 1.0) * H;
                current.push_str(&format!("{x:.1},{y:.1} "));
            }
            None => {
                if !current.is_empty() {
                    runs.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    rsx! {
        svg { class: "sparkline", view_box: "0 0 {W} {H}",
            for points in runs {
                polyline {
                    points: "{points}",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "1",
                    stroke_linecap: "round",
                }
            }
        }
    }
}


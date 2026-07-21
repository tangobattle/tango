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
    // the next mount starts from a fresh WebGL context.
    {
        let runtime = runtime.clone();
        use_effect(move || {
            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("framebuffer"))
                .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok());
            match canvas {
                Some(canvas) => runtime.borrow_mut().attach_canvas(&canvas),
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
                canvas {
                    id: "framebuffer",
                    width: if config.read().integer_scaling { "240" } else { "1440" },
                    height: if config.read().integer_scaling { "160" } else { "960" },
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
                            SessionEnd::MatchEnded { wins, losses, draws } => {
                                let (headline, tone) = if wins > losses {
                                    (t!(&lang, "session-results-victory"), "win")
                                } else if wins < losses {
                                    (t!(&lang, "session-results-defeat"), "loss")
                                } else {
                                    (t!(&lang, "session-results-draw"), "draw")
                                };
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

/// The replay transport bar (the desktop's, minus the scrubber/clip
/// machinery that rides the seek port): play/pause, the tick readout,
/// and the speed chips. Pause parks the pump's pacing; speed rides the
/// replay driver's own knob (the pump only overwrites it for local
/// sessions).
#[component]
fn ReplayTransport() -> Element {
    let Ctx { runtime, mut config, .. } = use_ctx();
    let lang = crate::i18n::LANG.read().clone();
    let Some((shared, paused, speed, tick)) = ({
        let rt = runtime.borrow();
        rt.shared().map(|shared| {
            (
                shared.clone(),
                shared.paused.load(Ordering::Relaxed),
                shared.speed.load(Ordering::Relaxed),
                shared.stats.lock().unwrap().frontier,
            )
        })
    }) else {
        return rsx! {};
    };
    let readout = crate::session::format_tick(tick);
    let show_inputs = config.read().show_replay_inputs;
    let shared_toggle = shared.clone();
    let shared_swap = shared.clone();
    rsx! {
        div { class: "transport-bar hud-chip",
            button {
                class: if paused { "btn round primary" } else { "btn round" },
                title: t!(&lang, "playback-pause"),
                onclick: move |_| {
                    if shared_toggle.paused.load(Ordering::Relaxed) {
                        shared_toggle.resume();
                    } else {
                        shared_toggle.paused.store(true, Ordering::Release);
                    }
                },
                if paused {
                    icons::Play {}
                } else {
                    icons::Pause {}
                }
            }
            span { class: "readout mono", "{readout}" }
            div { class: "grow" }
            span { class: "sub", {t!(&lang, "playback-speed")} }
            for (label, pct) in [("0.5×", 50u32), ("1×", 100), ("2×", 200), ("4×", 400)] {
                button {
                    class: if speed == pct { "btn chip active" } else { "btn chip" },
                    onclick: {
                        let shared = shared.clone();
                        move |_| shared.speed.store(pct, Ordering::Relaxed)
                    },
                    "{label}"
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
        if show_inputs {
            ReplayInputPads {}
        }
    }
}

/// The recorded joypads (the desktop's `input_display_overlay`): the
/// viewed player bottom-left, the other side bottom-right, each pad
/// lighting the keys held on the current tick.
#[component]
fn ReplayInputPads() -> Element {
    let Ctx { runtime, .. } = use_ctx();
    let Some((inputs, tick, view)) = ({
        let rt = runtime.borrow();
        match (rt.replay_inputs(), rt.shared()) {
            (Some(inputs), Some(shared)) => Some((
                inputs,
                shared.stats.lock().unwrap().frontier,
                shared.view_player.load(Ordering::Relaxed).min(1),
            )),
            _ => None,
        }
    }) else {
        return rsx! {};
    };
    let idx = (tick as usize).saturating_sub(1).min(inputs.len().saturating_sub(1));
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
/// with sparklines and the live frame-delay slider. Gated on the first
/// pong, like the desktop.
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
    // Nothing to show before the first pong.
    let Some(ping) = stats.rtt_ms else {
        return rsx! {};
    };

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


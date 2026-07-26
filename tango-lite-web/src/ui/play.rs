//! The emulator screen: a stage that owns the viewport, with everything
//! else floating over it.
//!
//! The stage is the whole screen and the controls sit on top of it,
//! rather than the screen and the pad splitting the height between them
//! — which is what lets the picture be as large as the device can show
//! it. On a phone held upright the session renders landscape and is
//! rotated onto the tall screen (see `.session` in the stylesheet), so
//! the pad's corners land where fingers actually rest either way up.
//!
//! Nothing here reads the session per frame. [`crate::engine`] paints
//! the canvas directly from its pump; this component supplies the
//! element to paint into and the controls that feed [`crate::input`].
//! The HUD's numbers arrive through a signal the app shell polls at
//! ~10Hz, which is as often as any of them are worth reading.

use dioxus::prelude::*;

use crate::engine::{Kind, Status};
use crate::ui::touch::TouchControls;

#[component]
pub fn Play(status: ReadSignal<Option<Status>>, onexit: EventHandler<()>) -> Element {
    // The canvas element is new every time this screen mounts, so the
    // pump's cached 2D context has to be dropped or the next paint goes
    // into a detached node.
    use_effect(move || crate::engine::invalidate_canvas());

    let status = status();
    let pvp = status.as_ref().filter(|s| s.kind == Kind::Pvp);

    rsx! {
        div { class: "session",
            div { class: "stage",
                canvas { class: "screen", id: "tango-screen" }
            }
            div { class: "session-hud",
                button {
                    class: "chip",
                    // Pointer, not click: the overlay swallows the
                    // events that would otherwise become one.
                    onpointerdown: move |_| onexit.call(()),
                    "Quit"
                }
                if let Some(pvp) = pvp {
                    MatchHud { status: pvp.clone() }
                }
            }
            if let Some((playhead, total)) = status.as_ref().and_then(|s| s.playhead) {
                Transport {
                    playhead,
                    total,
                    prefetched: status.as_ref().map(|s| s.prefetched).unwrap_or(0),
                    paused: status.as_ref().is_some_and(|s| s.paused),
                }
            } else {
                // A replay takes no input, so it gets the transport
                // instead of the pad rather than both.
                TouchControls {}
            }
            if pvp.is_some_and(|s| s.reconnecting) {
                div { class: "overlay",
                    div { class: "box",
                        div { class: "spinner" }
                        div { "Connection lost — reconnecting…" }
                    }
                }
            }
        }
    }
}

/// Playback transport: play/pause and a scrub bar.
///
/// Along the bottom rather than the top, because the touch pad is
/// hidden during a replay — there is nothing to press — so the bottom
/// of the screen is free and is where a thumb already is.
///
/// The track shades how far the keyframe pass has got, the way a video
/// player shades what it has buffered — and it means the same thing
/// here: a seek inside the shading lands on a keyframe immediately,
/// one past it has to re-simulate its way there and takes a moment.
#[component]
fn Transport(playhead: u32, total: u32, prefetched: u32, paused: bool) -> Element {
    let fraction = |ticks: u32| {
        if total == 0 {
            0.0
        } else {
            (ticks as f32 / total as f32).clamp(0.0, 1.0) * 100.0
        }
    };
    let played = fraction(playhead);
    // Never behind the playhead: everything already played has been
    // captured as it went, whatever the background pass has reached.
    let buffered = fraction(prefetched).max(played);
    // And the same point is as far as a seek may go. Past it there is
    // no keyframe to restore from, so the chase would have to
    // re-simulate its way there — seconds of nothing, from a control
    // that looked instant. The thumb sticks at the edge instead,
    // because the re-render keeps putting it back.
    let seekable = prefetched.max(playhead).min(total.saturating_sub(1));

    rsx! {
        div { class: "transport",
            button {
                class: "chip",
                onpointerdown: move |_| crate::engine::set_paused(!paused),
                if paused { "▶" } else { "❚❚" }
            }
            div {
                class: "scrub-wrap",
                style: "--played: {played}%; --buffered: {buffered}%",
                input {
                    r#type: "range",
                    class: "scrub",
                    min: "0",
                    max: "{total.saturating_sub(1)}",
                    value: "{playhead}",
                    // `oninput`, not `onchange`: a seek is a chase the
                    // engine slices across frames, so scrubbing shows
                    // where you are going as you drag.
                    oninput: move |event| {
                        if let Ok(tick) = event.value().parse::<u32>() {
                            crate::engine::seek_to(tick.min(seekable));
                        }
                    },
                }
            }
            span { class: "chip", "{seconds(playhead)} / {seconds(total)}" }
        }
    }
}

/// Ticks as mm:ss. A tick is one GBA frame, and the pair runs at the
/// hardware's real rate, not a round 60.
fn seconds(ticks: u32) -> String {
    let total = (ticks as f32 / tango_session::pvp::EXPECTED_FPS) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

#[component]
fn MatchHud(status: Status) -> Element {
    let mut frame_delay = use_signal(|| status.frame_delay);
    let ping = status
        .latency_ms
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(|| "—".to_string());
    let side = if status.local_player_index == 0 { "P1" } else { "P2" };

    rsx! {
        span { class: "chip", "{side}" }
        span { class: "chip", "{ping}" }
        span { class: "chip", "{status.tps} tps" }
        span { class: "chip delay",
            "delay {frame_delay}"
            input {
                r#type: "range",
                min: "{tango_session::pvp::MIN_FRAME_DELAY}",
                max: "{tango_session::pvp::MAX_FRAME_DELAY}",
                value: "{frame_delay}",
                // Purely local: the peer is neither told nor asked, so
                // this can move mid-match without renegotiating anything.
                oninput: move |event| {
                    if let Ok(value) = event.value().parse::<u32>() {
                        frame_delay.set(value);
                        crate::engine::set_frame_delay(value);
                    }
                },
            }
        }
    }
}

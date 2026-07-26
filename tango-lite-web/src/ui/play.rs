//! The emulator screen: the canvas, the pad, and — in a match — the
//! one live control worth having on a phone (frame delay).
//!
//! Nothing here reads the session per frame. [`crate::engine`] paints
//! the canvas directly from its pump; this component only supplies the
//! element to paint into and the buttons that feed
//! [`crate::input`]. The HUD's numbers arrive through a signal the app
//! shell polls at ~10Hz, which is as often as any of them are worth
//! reading.

use dioxus::prelude::*;

use mgba::input::keys;

use crate::engine::{Kind, Status};
use crate::input;

#[component]
pub fn Play(status: ReadSignal<Option<Status>>, onexit: EventHandler<()>) -> Element {
    // The canvas element is new every time this screen mounts, so the
    // pump's cached 2D context has to be dropped or the next paint goes
    // into a detached node.
    use_effect(move || crate::engine::invalidate_canvas());

    let status = status();
    let pvp = status.as_ref().filter(|s| s.kind == Kind::Pvp);

    rsx! {
        div { class: "play",
            if let Some(pvp) = pvp {
                MatchHud { status: pvp.clone() }
            }
            div { class: "stage",
                canvas { class: "screen", id: "tango-screen" }
            }
            Pad { onexit }
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

#[component]
fn MatchHud(status: Status) -> Element {
    let mut frame_delay = use_signal(|| status.frame_delay);
    let ping = status
        .latency_ms
        .map(|ms| format!("{ms} ms"))
        .unwrap_or_else(|| "—".to_string());
    let side = if status.local_player_index == 0 { "P1" } else { "P2" };

    rsx! {
        div { class: "hud",
            span { "{side}" }
            span { "{ping}" }
            span { "{status.tps} tps" }
            span { class: "grow" }
            span { "delay {frame_delay}" }
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

#[component]
fn Pad(onexit: EventHandler<()>) -> Element {
    rsx! {
        div { class: "pad",
            div { class: "shoulders",
                Key { mask: keys::L, label: "L" }
                button {
                    class: "btn small",
                    // Pointer, not click: the pad swallows the events
                    // that would otherwise become a click.
                    onpointerdown: move |_| onexit.call(()),
                    "Quit"
                }
                Key { mask: keys::R, label: "R" }
            }
            div { class: "dpad",
                for (index , (label , mask)) in input::pad::DPAD.iter().enumerate() {
                    if *mask == 0 {
                        div { key: "{index}", class: "hole" }
                    } else {
                        Key { key: "{index}", mask: *mask, label: label.to_string() }
                    }
                }
            }
            div { class: "face",
                Key { mask: keys::B, label: "B", class: "b" }
                Key { mask: keys::A, label: "A", class: "a" }
            }
            div { class: "menu-keys",
                Key { mask: keys::SELECT, label: "SELECT" }
                Key { mask: keys::START, label: "START" }
            }
        }
    }
}

/// One pad button.
///
/// Press on `pointerdown` and release on up / leave / cancel — no
/// pointer capture, so sliding a thumb off a button releases it rather
/// than holding it down forever. `preventDefault` on the press is what
/// stops the browser turning it into a synthetic click, a text
/// selection, or a double-tap zoom.
#[component]
fn Key(mask: u32, label: String, class: Option<String>) -> Element {
    let mut held = use_signal(|| false);
    let class = class.unwrap_or_default();
    rsx! {
        button {
            class: "{class}",
            "data-held": "{held}",
            onpointerdown: move |event| {
                event.prevent_default();
                held.set(true);
                input::touch_press(mask);
            },
            onpointerup: move |_| {
                held.set(false);
                input::touch_release(mask);
            },
            onpointerleave: move |_| {
                held.set(false);
                input::touch_release(mask);
            },
            oncontextmenu: move |event| event.prevent_default(),
            "{label}"
        }
    }
}

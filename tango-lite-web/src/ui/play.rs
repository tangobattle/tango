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
            TouchControls {}
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

//! The fullscreen session view: the framebuffer canvas, a compact
//! header, the Escape-toggled menu overlay, and the end-of-session
//! overlay (the session itself is already torn down by then; the
//! runtime keeps the end readable until it's dismissed).

use std::sync::atomic::Ordering;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;

use super::{icons, touch, use_ctx, Ctx};
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

    // Reactive inputs: per-frame stats, structural session changes, and
    // the Escape-toggled menu.
    let _ = FRAME_REV.read();
    let _ = SESSION_EPOCH.read();
    let menu_open = *MENU_OPEN.read();

    let (title, running, paused, end) = {
        let rt = runtime.borrow();
        let title = rt
            .descriptor()
            .map(|d| crate::library::display_name(d.game))
            .unwrap_or_else(|| "Session".to_string());
        let end = rt.last_end();
        match rt.shared() {
            Some(shared) => {
                let paused = shared.paused.load(Ordering::Relaxed);
                (title, true, paused, end)
            }
            None => (title, false, false, end),
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
                            title: "Menu (Esc)",
                            onclick: move |_| {
                                let open = *MENU_OPEN.peek();
                                *MENU_OPEN.write() = !open;
                            },
                            icons::Menu {}
                        }
                        button {
                            class: "btn ghost icon-btn quit",
                            title: "Quit game",
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
                    // Coarse-pointer screens get on-screen controls (CSS
                    // decides; it renders inert elsewhere). They stay put
                    // under an open card — the backdrop (z 7) covers them,
                    // so a touch there dismisses the card instead.
                    touch::TouchControls {}
                }
                if paused && !menu_open && end.is_none() {
                    span { class: "badge pause-badge", "Paused" }
                }
            }
            if let Some(end) = end {
                div { class: "overlay",
                    div { class: "overlay-panel",
                        p { class: "end-message", {end_message(&end)} }
                        button {
                            class: "btn primary",
                            onclick: {
                                let runtime = runtime.clone();
                                move |_| runtime.borrow_mut().dismiss_end()
                            },
                            "Back"
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

    let (title, caption, is_pvp) = {
        let rt = runtime.borrow();
        let title = rt
            .descriptor()
            .map(|d| crate::library::display_name(d.game))
            .unwrap_or_else(|| "Session".to_string());
        let is_pvp = rt.descriptor().map(|d| d.kind) == Some(SessionKind::Pvp);
        let caption = if is_pvp { "Netplay" } else { "Playing solo" };
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
                label { "Volume · {volume_pct}%" }
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
                    "Quit game"
                }
            }
            p { class: "hint", "{hints}" }
        }
    }
}

/// The end overlay's one-liner. PvP's variants join it at M4.
fn end_message(end: &SessionEnd) -> String {
    match end {
        SessionEnd::LocalQuit => "Session ended.".to_string(),
        SessionEnd::MatchEnded { wins, losses, draws } => {
            let verdict = if wins > losses {
                "Victory!"
            } else if wins < losses {
                "Defeat."
            } else {
                "Draw."
            };
            let mut line = format!("{verdict}  {wins} – {losses}");
            if *draws > 0 {
                line.push_str(&format!(" ({draws} draw(s))"));
            }
            line.push_str("  ·  Replay saved.");
            line
        }
        SessionEnd::ReplayFinished => "Replay finished.".to_string(),
        SessionEnd::Error(e) => format!("Session error: {e}"),
    }
}

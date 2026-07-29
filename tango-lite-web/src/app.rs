//! The shell: which screen is up, and the one place the non-reactive
//! world is mirrored into signals.
//!
//! The engine, the library and the netplay state machine all live in
//! thread-locals rather than signals — none of them is `Clone`, the
//! engine is touched sixty times a second, and what the UI wants off
//! them is a handful of numbers. So instead of pushing, a single
//! heartbeat polls them ~10 times a second and writes into a signal only
//! when the value has actually changed. Dioxus does the rest: an
//! unchanged signal is not a re-render.

use dioxus::prelude::*;

use crate::engine;
use crate::link::Snapshot;
use crate::loadout::Loadout;

/// How often the shell samples the engine / library / netplay state.
/// Fast enough that a ping readout and a lobby transition feel live,
/// slow enough to be free next to the 60Hz pump.
const HEARTBEAT: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Library,
    Link,
    Replays,
    Play,
}

// Dioxus components are named like types, because in `rsx!` that is what
// they are.
#[allow(non_snake_case)]
pub fn App() -> Element {
    let mut opened = use_signal(|| false);
    let mut screen = use_signal(|| Screen::Library);
    let mut loadout = use_signal(Loadout::default);
    let mut revision = use_signal(|| 0u64);
    let mut link_snapshot = use_signal(Snapshot::default);
    let mut engine_status = use_signal(|| None::<engine::Status>);
    let nickname = use_signal(|| crate::storage::prefs::get("nickname").unwrap_or_default());
    let code = use_signal(|| crate::storage::prefs::get("code").unwrap_or_default());
    // The one place a failure that isn't a session's own gets said out
    // loud: a replay that won't open, usually because a ROM is missing.
    let mut notice = use_signal(|| None::<String>);

    // Startup: read the persisted files, then pull the patch index. The
    // index is best-effort — offline, the cached copy from last time is
    // still browsable, which is exactly what it's stored for.
    use_future(move || async move {
        crate::library::open().await;
        loadout.write().reconcile();
        opened.set(true);
        if let Err(e) = crate::library::fetch_index().await {
            log::warn!("patch index unavailable: {e}");
        }
    });

    // The heartbeat.
    use_future(move || async move {
        loop {
            tango_session::platform::sleep(HEARTBEAT).await;

            let current = crate::library::revision();
            if *revision.peek() != current {
                revision.set(current);
                // A rescan can retire the picked save or patch, and a
                // stale pick is a play button that fails on press.
                let mut next = loadout.peek().clone();
                next.reconcile();
                if *loadout.peek() != next {
                    loadout.set(next);
                }
            }

            let snapshot = crate::link::snapshot();
            if *link_snapshot.peek() != snapshot {
                link_snapshot.set(snapshot);
            }

            let status = engine::status();
            if *engine_status.peek() != status {
                engine_status.set(status);
            }

            // A session that ended by itself — a match finished, the peer
            // left, the link died — puts the user back where they
            // started it from. Collected here rather than watched for on
            // `status`, which is `None` by the time anyone looks.
            if let Some(kind) = engine::take_ended() {
                crate::input::touch_clear();
                crate::input::gamepads_clear();
                engine_status.set(None);
                screen.set(match kind {
                    engine::Kind::Pvp => Screen::Link,
                    engine::Kind::SinglePlayer => Screen::Library,
                    engine::Kind::Replay => Screen::Replays,
                });
            }
        }
    });

    // What we're bringing is also what the lobby advertises and what the
    // handoff builds the match from, so every pick change goes over.
    use_effect(move || crate::link::set_loadout(loadout()));

    // A match that started elsewhere (the peer readied last) takes over
    // the screen when its first frame lands.
    use_effect(move || {
        if engine_status().is_some() && *screen.peek() != Screen::Play {
            screen.set(Screen::Play);
        }
    });

    if !opened() {
        return rsx! {
            div { class: "boot", "Loading Tango Lite…" }
        };
    }

    rsx! {
        div { class: "app",
            match screen() {
                Screen::Play => rsx! {
                    crate::ui::play::Play {
                        status: engine_status(),
                        onexit: move |_| {
                            let kind = engine_status.peek().as_ref().map(|status| status.kind);
                            engine::stop();
                            // Quitting is a navigation of its own; don't
                            // let the latch fire a second one.
                            let _ = engine::take_ended();
                            crate::input::touch_clear();
                            crate::input::gamepads_clear();
                            engine_status.set(None);
                            // Back where it was started from.
                            screen.set(match kind {
                                Some(engine::Kind::Pvp) => Screen::Link,
                                Some(engine::Kind::Replay) => Screen::Replays,
                                _ => Screen::Library,
                            });
                        },
                    }
                },
                Screen::Library => rsx! {
                    Header { title: "Tango Lite", subtitle: subtitle(&loadout()) }
                    crate::ui::library::Library {
                        loadout,
                        revision: revision(),
                        onplay: move |_| start_single_player(loadout(), screen),
                    }
                    Tabs { screen }
                },
                Screen::Link => rsx! {
                    Header { title: "Link battle", subtitle: subtitle(&loadout()) }
                    crate::ui::link::Link {
                        snapshot: link_snapshot(),
                        loadout: loadout(),
                        nickname,
                        code,
                    }
                    Tabs { screen }
                },
                Screen::Replays => rsx! {
                    Header { title: "Replays", subtitle: notice().unwrap_or_default() }
                    crate::ui::replays::Replays {
                        revision: revision(),
                        onerror: move |message| notice.set(Some(message)),
                    }
                    Tabs { screen }
                },
            }
        }
    }
}

fn subtitle(loadout: &Loadout) -> String {
    match loadout.game {
        Some(game) => crate::ui::game_label(game),
        None => "No game picked".to_string(),
    }
}

#[component]
fn Header(title: String, subtitle: String) -> Element {
    rsx! {
        div { class: "topbar",
            h1 { "{title}" }
            span { class: "sub", "{subtitle}" }
        }
    }
}

#[component]
fn Tabs(screen: Signal<Screen>) -> Element {
    rsx! {
        div { class: "tabs",
            button {
                "aria-selected": "{screen() == Screen::Library}",
                onclick: move |_| screen.set(Screen::Library),
                "Library"
            }
            button {
                "aria-selected": "{screen() == Screen::Link}",
                onclick: move |_| screen.set(Screen::Link),
                "Link battle"
            }
            button {
                "aria-selected": "{screen() == Screen::Replays}",
                onclick: move |_| screen.set(Screen::Replays),
                "Replays"
            }
        }
    }
}

/// Boot a standalone session and hand it to the pump.
///
/// Spawned rather than awaited inline because building the audio graph
/// is async (the worklet module has to load) — and this runs from the
/// click that is the user gesture the audio context needs, which is the
/// whole reason the sink is built here rather than at startup.
fn start_single_player(loadout: Loadout, mut screen: Signal<Screen>) {
    spawn(async move {
        let Some(game) = loadout.game else { return };
        let rom = match loadout.rom() {
            Ok(rom) => rom,
            Err(e) => {
                log::error!("{e}");
                return;
            }
        };
        let save = loadout.save_bytes();
        let sink = crate::audio::sink().await;
        let session = tango_session::singleplayer::SinglePlayerSession::new(
            game,
            std::sync::Arc::new(rom),
            save,
            // A browser has no cart clock to read, so the match clock
            // that PvP negotiates has a single-player counterpart: pin
            // it to now, once, at boot.
            Some(now()),
            game.pvp.expected_fps() as f32,
            crate::audio::sample_rate(),
        );
        match session {
            Ok((session, driver, stream)) => {
                crate::engine::start_single_player(session, driver, stream, sink, loadout.save_path.clone());
                screen.set(Screen::Play);
            }
            Err(e) => log::error!("failed to boot {}: {e}", crate::ui::game_label(game)),
        }
    });
}

/// Wall clock as a `SystemTime`. `SystemTime::now()` panics on wasm32,
/// so it comes from the page's own clock instead.
fn now() -> std::time::SystemTime {
    let millis = js_sys::Date::now().max(0.0);
    std::time::UNIX_EPOCH + std::time::Duration::from_millis(millis as u64)
}

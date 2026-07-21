//! The diagnostics pane (Settings → Diagnostics): the wasm half of the
//! cross-target determinism probe, and the permanent desync-diagnosis
//! tool. Runs ONE `tango_pvp::engine::Match` — both cores in-process,
//! zero-latency lockstep schedule, fully fixed inputs — and folds every
//! settled `(tick, digest)` checkpoint into one stream hash. The native
//! half (`gamesupport/tango-gamesupport-bn6/examples/pvp_determinism.rs`)
//! runs the IDENTICAL procedure; equal stream hashes prove the wasm
//! build simulates bit-identically to native — the crossplay gate.
//!
//! Keep [`schedule`], [`Fnv`], and [`probe_config`]'s constants in
//! lockstep with the native example.

use dioxus::prelude::*;

use super::{use_ctx, Ctx};
use crate::t;
use crate::library::{self, GameRef};

/// MUST match the native example's `schedule`.
fn schedule(frame: u32, player: u32) -> u32 {
    if (frame + player * 7) % 5 < 2 {
        1
    } else {
        0
    }
}

/// Dependency-free FNV-1a over the checkpoint stream. MUST match the
/// native example's `Fnv`.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf29ce484222325)
    }

    fn update(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}

/// MUST match the native example's `MatchConfig` (both sides run the
/// same game with the same save on both cores).
fn probe_config<'a>(
    rom: Vec<u8>,
    save: Vec<u8>,
    support: &'a (dyn tango_pvp::GameSupport + Send + Sync),
) -> tango_pvp::engine::MatchConfig<'a> {
    tango_pvp::engine::MatchConfig {
        roms: [rom.clone(), rom],
        saves: [save.clone(), save],
        support: [support, support],
        match_type: (0, 0),
        rng_seed: *b"sio-probe-seed!!",
        rtc: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000),
        local_player: 0,
        present_delay: 2,
        disable_bgm: false,
    }
}

const TICKS: u32 = 3600;
/// Frames simulated per event-loop yield: long enough to make
/// progress, short enough that the tab stays responsive and the
/// status line repaints.
const CHUNK: u32 = 120;

#[derive(Clone, PartialEq)]
enum ProbeState {
    Idle,
    Running { frame: u32 },
    Done { summary: String },
    Failed { error: String },
}

#[component]
pub fn DiagnosticsSection() -> Element {
    let Ctx {
        storage,
        library,
        selected_family,
        selected_save,
        ..
    } = use_ctx();
    let mut state = use_signal(|| ProbeState::Idle);

    // The probe runs on the Play tab's active pick: family + a REAL
    // save file (the native half is fed the same file, so a template
    // fresh-save has no on-disk twin to compare against).
    let family = selected_family.read().clone();
    let pick = selected_save.read().clone();
    let real_save = pick.as_ref().filter(|p| !p.starts_with("//fresh/")).cloned();
    let ready = family.is_some() && real_save.is_some();
    let running = matches!(*state.read(), ProbeState::Running { .. });

    let lang = crate::i18n::LANG.read().clone();
    rsx! {
        section { class: "pane",
            h2 { {t!(&lang, "web-diagnostics")} }
            p { class: "sub", {t!(&lang, "web-diagnostics-description")} }
            div { class: "option-row",
                label {
                    if let Some(save) = real_save.as_deref() {
                        "{save}"
                    } else {
                        {t!(&lang, "web-diagnostics-pick")}
                    }
                }
                button {
                    class: "btn primary",
                    disabled: !ready || running,
                    onclick: move |_| {
                        let storage = storage.read().clone().flatten();
                        let lib = library.read().clone().flatten().unwrap_or_default();
                        let family = selected_family.peek().clone();
                        let save_name = selected_save.peek().clone();
                        async move {
                            let (Some(storage), Some(family), Some(save_name)) =
                                (storage, family, save_name)
                            else {
                                return;
                            };
                            match run_probe(state, &storage, &lib, &family, &save_name).await {
                                Ok(()) => {}
                                Err(e) => state.set(ProbeState::Failed {
                                    error: format!("{e:#}"),
                                }),
                            }
                        }
                    },
                    if running {
                        {t!(&lang, "web-diagnostics-running")}
                    } else {
                        {t!(&lang, "web-diagnostics-run")}
                    }
                }
            }
            match state.read().clone() {
                ProbeState::Idle => rsx! {},
                ProbeState::Running { frame } => rsx! {
                    p { class: "sub", "simulating… {frame}/{TICKS} ticks" }
                },
                ProbeState::Done { summary } => rsx! {
                    p { class: "sub flash ok", "{summary}" }
                },
                ProbeState::Failed { error } => rsx! {
                    p { class: "sub flash bad", "{error}" }
                },
            }
        }
    }
}

/// Resolve the save to its game, read both images, run the probe.
async fn run_probe(
    mut state: Signal<ProbeState>,
    storage: &crate::storage::Storage,
    lib: &library::Library,
    family: &str,
    save_name: &str,
) -> anyhow::Result<()> {
    state.set(ProbeState::Running { frame: 0 });

    let save = crate::storage::read(storage.saves(), save_name)
        .await
        .map_err(|e| anyhow::anyhow!("couldn't read save: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("save disappeared"))?;
    let games: Vec<GameRef> = library::games_in_family(family).collect();
    let game = games
        .iter()
        .copied()
        .find(|g| g.parse_save(&save).is_ok())
        .ok_or_else(|| anyhow::anyhow!("save no longer parses as {family}"))?;
    let entry = lib
        .by_game(game)
        .ok_or_else(|| anyhow::anyhow!("that game's ROM isn't imported"))?;
    let rom = crate::storage::read(storage.roms(), &entry.file)
        .await
        .map_err(|e| anyhow::anyhow!("couldn't read ROM: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("ROM disappeared"))?;

    log::info!(
        "dprobe: {} + {save_name}, {TICKS} ticks",
        library::game_slug(game)
    );

    // Priming alone is a few thousand core-frames; yield first so the
    // "Running…" state paints before the long haul starts.
    gloo_timers::future::TimeoutFuture::new(0).await;
    let mut m = tango_pvp::engine::Match::new(probe_config(rom, save, game.pvp))?;

    let mut stream = Fnv::new();
    let mut checkpoints = 0u32;
    let mut last: Option<(u32, u32)> = None;
    for frame in 0..TICKS {
        m.add_remote_input(schedule(frame, 1), 0);
        let _ = m.advance(schedule(frame, 0))?;
        if let Some((tick, digest)) = m.checkpoint() {
            if last.map(|(t, _)| t) != Some(tick) {
                stream.update(&tick.to_le_bytes());
                stream.update(&digest.to_le_bytes());
                checkpoints += 1;
                last = Some((tick, digest));
                if tick % 600 == 0 {
                    log::info!("dprobe: tick {tick} digest {digest:08x}");
                }
            }
        }
        if frame % CHUNK == 0 {
            state.set(ProbeState::Running { frame });
            gloo_timers::future::TimeoutFuture::new(0).await;
        }
    }

    let (final_tick, final_digest) =
        last.ok_or_else(|| anyhow::anyhow!("no checkpoints settled"))?;
    let summary = format!(
        "dprobe: ticks={TICKS} checkpoints={checkpoints} stream={:016x} final={final_tick}:{final_digest:08x}",
        stream.0
    );
    log::info!("{summary}");
    state.set(ProbeState::Done { summary });
    Ok(())
}

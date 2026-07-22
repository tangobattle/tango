//! Match-stats analysis, browser flavor: the desktop's replay re-sim
//! fold (`tango_pvp::analysis::analyze`) rebuilt as a cooperative
//! main-thread job — the same prime + tick + telemetry-fold loop,
//! yielding to the event loop every batch so the UI stays live — with
//! the results cached as the desktop's own `.stats` sidecar format in
//! OPFS (`stats/<replay stem>.stats`). Version drift self-heals:
//! `MatchStats::read` rejects other format versions and the caller
//! recomputes.

use dioxus::prelude::*;
use tango_pvp::analysis::{MatchStats, StatsBuilder};

/// The in-flight analysis, for status lines: `(replay file, done,
/// total)`. One at a time — a second request while one runs just
/// waits its turn behind the guard.
pub static ANALYSIS_PROGRESS: GlobalSignal<Option<(String, u32, u32)>> = Signal::global(|| None);

/// Ticks simulated per cooperative slice. Headless + frameskipped, a
/// batch runs well under a frame's budget.
const BATCH: u32 = 240;

fn stats_name(replay_file: &str) -> String {
    let stem = replay_file.strip_suffix(".tangoreplay").unwrap_or(replay_file);
    format!("{stem}.stats")
}

/// Load `file`'s cached stats without ever computing — the running
/// session's analysis strip uses this: a re-sim on the emulator's own
/// thread would starve the pump (the desktop hands this to a worker
/// thread; the web has none).
pub async fn cached_stats(storage: &crate::storage::Storage, file: &str) -> Option<MatchStats> {
    let bytes = crate::storage::read(storage.stats(), &stats_name(file)).await.ok()??;
    MatchStats::read(&bytes[..]).ok()
}

/// Load `file`'s cached stats, or re-simulate + cache them. `storage`
/// and `lib` are the same handles the replays tab's Watch uses.
pub async fn stats_for(
    storage: Option<crate::storage::Storage>,
    lib: Option<crate::library::Library>,
    file: &str,
) -> anyhow::Result<MatchStats> {
    let name = stats_name(file);
    loop {
        if let Some(s) = &storage {
            if let Ok(Some(bytes)) = crate::storage::read(s.stats(), &name).await {
                if let Ok(stats) = MatchStats::read(&bytes[..]) {
                    return Ok(stats);
                }
                // Stale format (or corrupt) — recompute over it.
            }
        }
        // One analysis at a time; wait our turn (the running one may
        // even be for this very file — the cache re-check above picks
        // that up).
        if ANALYSIS_PROGRESS.peek().is_none() {
            break;
        }
        gloo_timers::future::TimeoutFuture::new(250).await;
    }
    *ANALYSIS_PROGRESS.write() = Some((file.to_string(), 0, 0));
    let result = compute(storage.clone(), lib, file).await;
    *ANALYSIS_PROGRESS.write() = None;

    let stats = result?;
    if let Some(s) = &storage {
        let mut bytes = Vec::new();
        if stats.write(&mut bytes).is_ok() {
            if let Err(e) = crate::storage::write(s.stats(), &name, &bytes).await {
                log::warn!("stats cache write failed: {e}");
            }
        }
    }
    Ok(stats)
}

async fn compute(
    storage: Option<crate::storage::Storage>,
    lib: Option<crate::library::Library>,
    file: &str,
) -> anyhow::Result<MatchStats> {
    let (replay, local_rom, remote_rom) = crate::ui::replays::load_pair(storage, lib, file).await?;
    let (local_game, remote_game) = crate::session::replay::resolve_games(&replay)?;
    let local_player = replay.local_player_index as usize;
    let chip_semantics = local_game.pvp.chip_semantics(&local_rom);
    let counts_buster = local_game.pvp.counts_buster(&local_rom);

    // Everything below is in ABSOLUTE player order; the replay stores
    // its streams local-first (the same orientation the viewer boots).
    let (roms, saves, games) = if local_player == 0 {
        (
            [local_rom, remote_rom],
            [replay.local_sram.clone(), replay.remote_sram.clone()],
            [local_game, remote_game],
        )
    } else {
        (
            [remote_rom, local_rom],
            [replay.remote_sram.clone(), replay.local_sram.clone()],
            [remote_game, local_game],
        )
    };
    let inputs: Vec<[u32; 2]> = replay
        .inputs
        .iter()
        .map(|(local, remote)| {
            let (p0, p1) = if local_player == 0 {
                (local.joyflags, remote.joyflags)
            } else {
                (remote.joyflags, local.joyflags)
            };
            [p0 as u32, p1 as u32]
        })
        .collect();

    // ---- boot + prime, the analyze() body with yields ----
    let [rom0, rom1] = roms;
    let [save0, save1] = saves;
    let mut pair = mgba_siolink::Link::with_options(mgba_siolink::LinkOptions {
        sides: vec![
            mgba_siolink::SideOptions {
                rom: rom0,
                save: Some(save0),
            },
            mgba_siolink::SideOptions {
                rom: rom1,
                save: Some(save1),
            },
        ],
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_millis(replay.metadata.ts)),
        peripheral: mgba_siolink::Peripheral::Cable,
    })?;
    // Nothing reads the pixels.
    pair.set_frameskip(0, i32::MAX);
    pair.set_frameskip(1, i32::MAX);

    let prime_config = tango_pvp::PrimeConfig {
        match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
        rng_seed: replay.rng_seed,
        disable_bgm: false,
    };
    let lifecycle = tango_pvp::telemetry::LifecycleSink::new();
    let primed = [tango_pvp::PrimedLatch::new(), tango_pvp::PrimedLatch::new()];
    pair.set_traps(0, games[0].pvp.primer_traps(&prime_config, 0, &lifecycle, &primed[0]));
    pair.set_traps(1, games[1].pvp.primer_traps(&prime_config, 1, &lifecycle, &primed[1]));

    const MAX_PRIME_TICKS: u32 = 3600;
    let mut prime_ticks = 0;
    while !(primed[0].is_set() && primed[1].is_set()) {
        if prime_ticks >= MAX_PRIME_TICKS {
            anyhow::bail!("analysis: priming did not reach a link battle within {MAX_PRIME_TICKS} ticks");
        }
        pair.tick(&[0, 0]);
        prime_ticks += 1;
        if prime_ticks % BATCH == 0 {
            gloo_timers::future::TimeoutFuture::new(0).await;
        }
    }

    // ---- the fold ----
    use mgba_siolink::session::TickObserver;
    let (mut observer, store) = tango_pvp::telemetry::Telemetry::new(
        [games[0].pvp.core_poller(0), games[1].pvp.core_poller(1)],
        lifecycle,
    );
    let mut builder = StatsBuilder::new(chip_semantics, counts_buster);
    let total = inputs.len() as u32;
    for (i, &keys) in inputs.iter().enumerate() {
        let tick = i as u32 + 1;
        pair.tick(&keys);
        // Everything is final on a linear re-sim — fold as we go.
        observer.on_tick(&mut pair, tick);
        let (samples, events) = store.lock().unwrap().drain_confirmed(tick);
        tango_pvp::analysis::fold_confirmed(&mut builder, local_player, samples, events, &mut |t| {
            (t == tick).then_some(keys)
        });
        if tick % BATCH == 0 {
            if let Some(p) = ANALYSIS_PROGRESS.write().as_mut() {
                p.1 = tick;
                p.2 = total;
            }
            gloo_timers::future::TimeoutFuture::new(0).await;
        }
    }

    Ok(builder.finish())
}

//! Replay playback, browser flavor: `tango_pvp::playback::Playback`
//! (the same boot-and-prime linear re-sim the desktop viewer and
//! exporter run) ticked by the runtime pump. Linear playback with the
//! hold-to-fast-forward knob; the desktop's seek/scrub machinery rides
//! a later milestone.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use crate::library::{self, GameRef};
use crate::session::{
    LinkAccess, SessionDescriptor, SessionEnd, SessionKind, SharedSession, EXPECTED_FPS,
};

pub struct ReplaySession {
    pub driver: ReplayDriver,
    pub shared: Arc<SharedSession>,
    pub link: LinkAccess,
    pub descriptor: SessionDescriptor,
}

/// Resolve a decoded replay's two games against the registry.
pub fn resolve_games(
    replay: &tango_pvp::replay::Replay,
) -> anyhow::Result<(GameRef, GameRef)> {
    let side_game = |side: Option<&tango_pvp::replay::metadata::Side>| -> anyhow::Result<GameRef> {
        let info = side
            .and_then(|s| s.game_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("replay has no game info"))?;
        library::find_by_family_and_variant(&info.rom_family, info.rom_variant as u8)
            .ok_or_else(|| anyhow::anyhow!("{} isn't supported by this build", info.rom_family))
    };
    Ok((
        side_game(replay.metadata.local_side.as_ref())?,
        side_game(replay.metadata.remote_side.as_ref())?,
    ))
}

/// Boot + prime a playback pair for `replay` (synchronously — seconds,
/// behind the caller's status line). Shared by live playback and the
/// video exporter; returns the playback plus the local player index.
pub fn boot(
    replay: &tango_pvp::replay::Replay,
    local_rom: Vec<u8>,
    remote_rom: Vec<u8>,
    disable_bgm: bool,
) -> anyhow::Result<(tango_pvp::playback::Playback, usize)> {
    let (local_game, remote_game) = resolve_games(replay)?;
    let local_player = replay.local_player_index as usize;

    // Everything the boot takes is in ABSOLUTE player order; the
    // replay stores its streams local-first.
    let (roms, saves, games): ([Vec<u8>; 2], [Vec<u8>; 2], [GameRef; 2]) = if local_player == 0 {
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

    let config = tango_pvp::playback::BootConfig {
        roms,
        saves,
        support: [games[0].pvp, games[1].pvp],
        match_type: (
            replay.metadata.match_type as u8,
            replay.metadata.match_subtype as u8,
        ),
        rng_seed: replay.rng_seed,
        rtc: std::time::UNIX_EPOCH + std::time::Duration::from_millis(replay.metadata.ts),
        disable_bgm,
    };
    let lifecycle = tango_pvp::telemetry::LifecycleSink::new();
    let playback = tango_pvp::playback::Playback::new(&config, Arc::new(inputs), &lifecycle)?;
    Ok((playback, local_player))
}

/// Boot + prime the playback pair for the live viewer.
pub fn start(
    replay: tango_pvp::replay::Replay,
    local_rom: Vec<u8>,
    remote_rom: Vec<u8>,
) -> anyhow::Result<ReplaySession> {
    let (local_game, _) = resolve_games(&replay)?;
    let (playback, local_player) = boot(&replay, local_rom, remote_rom, false)?;

    let shared = SharedSession::new(0);
    shared.view_player.store(local_player, Ordering::Relaxed);

    let descriptor = SessionDescriptor {
        kind: SessionKind::Replay,
        local_player,
        game: local_game,
    };

    let playback = Arc::new(Mutex::new(playback));
    Ok(ReplaySession {
        driver: ReplayDriver {
            shared: shared.clone(),
            playback: playback.clone(),
        },
        shared,
        link: LinkAccess::Playback(playback),
        descriptor,
    })
}

/// The playback session's per-tick body: feed the next recorded pair.
pub struct ReplayDriver {
    shared: Arc<SharedSession>,
    playback: Arc<Mutex<tango_pvp::playback::Playback>>,
}

impl ReplayDriver {
    pub fn tick(&mut self) -> bool {
        if self.shared.quit.load(Ordering::Relaxed) {
            self.shared.finish(SessionEnd::LocalQuit);
            return false;
        }

        let view = self.shared.view_player.load(Ordering::Relaxed);
        let stepped = {
            let mut playback = self.playback.lock().unwrap();
            let stepped = playback.step();
            if stepped {
                if let Some(buf) = playback.pair_mut().video_buffer(view) {
                    self.shared.publish_video(buf);
                }
            }
            stepped
        };
        if !stepped {
            self.shared.finish(SessionEnd::ReplayFinished);
            return false;
        }

        // Hold-to-fast-forward, same knob as solo play.
        let speed = self.shared.speed.load(Ordering::Relaxed).max(25) as f32 / 100.0;
        let fps_target = EXPECTED_FPS * speed;
        self.shared.set_fps_target(fps_target);
        {
            let mut stats = self.shared.stats.lock().unwrap();
            stats.fps_target = fps_target;
            stats.frontier += 1;
        }
        true
    }
}

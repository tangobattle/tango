//! Replay playback, browser flavor: `tango_pvp::playback::Playback`
//! (the same boot-and-prime linear re-sim the desktop viewer and
//! exporter run) ticked by the runtime pump.
//!
//! Seeking is the desktop's snapshot-and-chase, single-threaded: there
//! is no seek worker or prefetch pair here, so keyframes are captured
//! opportunistically as playback runs, and a seek is chased
//! cooperatively — a time-budgeted burst per pump — from the best
//! snapshot at or before the target. The keyframe store adaptively
//! thins itself instead of leaning on a prefetcher: wasm linear memory
//! never shrinks, so an unbounded desktop-style store would eat the
//! tab.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_pvp::playback::Snapshot;

use crate::library::{self, GameRef};
use crate::session::{
    LinkAccess, SessionDescriptor, SessionEnd, SessionKind, SharedSession, EXPECTED_FPS,
};

/// Capture cadence during normal playback — the desktop's interval.
const KEYFRAME_GAP: u32 = tango_pvp::playback::KEYFRAME_INTERVAL;

/// Ceiling on stored keyframes (~1MB apiece: both cores' savestate +
/// framebuffers). Crossing it doubles the effective gap and thins.
const KEYFRAME_CAP: usize = 96;

/// Per-pump budget for a seek chase. Half a frame: the chase shares
/// the main thread with the UI, so it advances in bursts instead of
/// stalling the tab until it lands.
const CHASE_BUDGET_MS: f64 = 8.0;

/// The keyframe store: the desktop `SnapshotStore` with a memory cap.
/// Thinning doubles the gap and drops the entries between, so coverage
/// stays whole-replay at coarser granularity rather than truncating.
pub struct Keyframes {
    map: BTreeMap<u32, Arc<Snapshot>>,
    gap: u32,
}

impl Keyframes {
    fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            gap: KEYFRAME_GAP,
        }
    }

    /// True if no keyframe exists within the current gap at or before
    /// `tick` — capturing here fills a hole.
    fn needed(&self, tick: u32) -> bool {
        let lo = tick.saturating_sub(self.gap);
        self.map
            .range((std::ops::Bound::Excluded(lo), std::ops::Bound::Included(tick)))
            .next()
            .is_none()
    }

    fn insert(&mut self, snap: Arc<Snapshot>) {
        self.map.insert(snap.tick, snap);
        if self.map.len() > KEYFRAME_CAP {
            self.gap = self.gap.saturating_mul(2);
            let mut last: Option<u32> = None;
            let gap = self.gap;
            self.map.retain(|&t, _| {
                if last.is_none_or(|l| t >= l + gap) {
                    last = Some(t);
                    true
                } else {
                    false
                }
            });
        }
    }

    fn best_at_or_before(&self, target: u32) -> Option<Arc<Snapshot>> {
        self.map.range(..=target).next_back().map(|(_, s)| s.clone())
    }

    fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<Arc<Snapshot>> {
        self.map
            .range((
                std::ops::Bound::Excluded(lo_exclusive),
                std::ops::Bound::Included(hi_inclusive),
            ))
            .next_back()
            .map(|(_, s)| s.clone())
    }

    /// Keyframe closest to `target` on either side — the scrub
    /// preview's lookup.
    pub fn nearest(&self, target: u32) -> Option<Arc<Snapshot>> {
        let below = self.map.range(..=target).next_back();
        let above = self
            .map
            .range((std::ops::Bound::Excluded(target), std::ops::Bound::Unbounded))
            .next();
        [below, above]
            .into_iter()
            .flatten()
            .min_by_key(|(k, _)| k.abs_diff(target))
            .map(|(_, s)| s.clone())
    }

    /// Keyframe exactly at `target`, if one exists — the press-preview
    /// lookup (a nearest frame there would flash the wrong scene).
    pub fn exact(&self, target: u32) -> Option<Arc<Snapshot>> {
        self.map.get(&target).cloned()
    }
}

/// UI → driver seek request. One slot, newest target wins — a fresh
/// request supersedes an in-flight chase at its next burst.
pub struct SeekState {
    /// The requested tick; `u32::MAX` = no seek pending.
    target: AtomicU32,
    /// Unpause on landing (the drag started while playing).
    resume_after: AtomicBool,
}

impl SeekState {
    fn new() -> Self {
        Self {
            target: AtomicU32::new(u32::MAX),
            resume_after: AtomicBool::new(false),
        }
    }

    pub fn request(&self, target: u32, resume_after: bool) {
        self.resume_after.store(resume_after, Ordering::Relaxed);
        self.target.store(target, Ordering::Relaxed);
    }

    /// The in-flight chase's target — readouts hold here instead of
    /// snapping back to the cursor while the chase catches up.
    pub fn pending(&self) -> Option<u32> {
        match self.target.load(Ordering::Relaxed) {
            u32::MAX => None,
            t => Some(t),
        }
    }

    /// A landed chase will unpause — the transport keeps showing
    /// "playing" through the chase instead of a stuck pause glyph.
    pub fn will_resume(&self) -> bool {
        self.pending().is_some() && self.resume_after.load(Ordering::Relaxed)
    }
}

pub struct ReplaySession {
    pub driver: ReplayDriver,
    pub shared: Arc<SharedSession>,
    pub link: LinkAccess,
    pub descriptor: SessionDescriptor,
    /// The recorded joyflag stream in absolute player order, one entry
    /// per tick — the transport bar's input display reads the current
    /// tick's pair out of it.
    pub inputs: Arc<Vec<[u32; 2]>>,
    /// Keyframes for seek/scrub, shared with the driver.
    pub keyframes: Arc<Mutex<Keyframes>>,
    pub seek: Arc<SeekState>,
    /// Inter-round scrubber marks: the recorder's round-start markers,
    /// minus the first round's tick 0. Recordings that predate the
    /// markers get an unsegmented bar (no prefetch pair here to
    /// re-derive them).
    pub round_boundaries: Vec<u32>,
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
) -> anyhow::Result<(tango_pvp::playback::Playback, usize, Arc<Vec<[u32; 2]>>)> {
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
    let inputs = Arc::new(inputs);
    let playback = tango_pvp::playback::Playback::new(&config, inputs.clone(), &lifecycle)?;
    Ok((playback, local_player, inputs))
}

/// Boot + prime the playback pair for the live viewer.
pub fn start(
    replay: tango_pvp::replay::Replay,
    local_rom: Vec<u8>,
    remote_rom: Vec<u8>,
) -> anyhow::Result<ReplaySession> {
    let (local_game, _) = resolve_games(&replay)?;
    let round_boundaries: Vec<u32> = replay.round_starts.iter().skip(1).map(|&i| i as u32).collect();
    let (mut playback, local_player, inputs) = boot(&replay, local_rom, remote_rom, false)?;

    let shared = SharedSession::new(0);
    shared.view_player.store(local_player, Ordering::Relaxed);

    let descriptor = SessionDescriptor {
        kind: SessionKind::Replay,
        local_player,
        game: local_game,
    };

    // The tick-0 keyframe: the floor every backward seek can land on.
    let mut keyframes = Keyframes::new();
    match playback.capture() {
        Ok(snap) => keyframes.insert(snap),
        Err(e) => log::warn!("replay: tick-0 capture failed: {e:?}"),
    }
    let keyframes = Arc::new(Mutex::new(keyframes));
    let seek = Arc::new(SeekState::new());

    let playback = Arc::new(Mutex::new(playback));
    Ok(ReplaySession {
        driver: ReplayDriver {
            shared: shared.clone(),
            playback: playback.clone(),
            keyframes: keyframes.clone(),
            seek: seek.clone(),
            chasing: false,
        },
        shared,
        link: LinkAccess::Playback(playback),
        descriptor,
        inputs,
        keyframes,
        seek,
        round_boundaries,
    })
}

/// The playback session's per-tick body: feed the next recorded pair.
pub struct ReplayDriver {
    shared: Arc<SharedSession>,
    playback: Arc<Mutex<tango_pvp::playback::Playback>>,
    keyframes: Arc<Mutex<Keyframes>>,
    seek: Arc<SeekState>,
    /// A chase burst ran and left frameskip engaged — the landing
    /// burst has to restore it even if the target moved meanwhile.
    chasing: bool,
}

impl ReplayDriver {
    pub fn tick(&mut self) -> bool {
        if self.shared.quit.load(Ordering::Relaxed) {
            self.shared.finish(SessionEnd::LocalQuit);
            return false;
        }
        // A pending seek owns the pair — the chase (pump-driven) does
        // the stepping. Scrub drags pause playback so this is mostly
        // belt-and-suspenders against a seek fired while playing.
        if self.seek.pending().is_some() {
            return true;
        }

        let view = self.shared.view_player.load(Ordering::Relaxed).min(1);
        let stepped = {
            let mut playback = self.playback.lock().unwrap();
            let stepped = playback.step();
            if stepped {
                if let Some(buf) = playback.pair_mut().video_buffer(view) {
                    self.shared.publish_video(buf);
                }
                // The other side feeds the picture-in-picture.
                if let Some(buf) = playback.pair_mut().video_buffer(1 - view) {
                    self.shared.publish_video2(buf);
                }
                // Opportunistic keyframes — rendered playback is where
                // capture is free-est (the framebuffers are current, so
                // they double as scrub previews).
                let mut keyframes = self.keyframes.lock().unwrap();
                if keyframes.needed(playback.cursor()) {
                    match playback.capture() {
                        Ok(snap) => keyframes.insert(snap),
                        Err(e) => log::warn!("replay: keyframe capture failed: {e:?}"),
                    }
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
            // The cursor, not a running count — seeks move it both ways.
            stats.frontier = self.playback.lock().unwrap().cursor();
        }
        true
    }

    /// One cooperative chase burst toward the pending seek target, if
    /// any. Called once per rAF pump regardless of pause state (drags
    /// pause playback; the chase must still run under them). Returns
    /// true when it made progress the UI should repaint for.
    pub fn chase_seek(&mut self, now_ms_fn: impl Fn() -> f64) -> bool {
        let Some(raw_target) = self.seek.pending() else {
            return false;
        };
        let mut playback = self.playback.lock().unwrap();
        let target = raw_target.min(playback.total());
        let cur = playback.cursor();

        // Plan: land on a snapshot when the target is behind the
        // cursor, or when one sits between us and a forward target.
        if target < cur {
            let snap = self.keyframes.lock().unwrap().best_at_or_before(target);
            let Some(snap) = snap else {
                // No floor to land on (tick-0 capture failed) — drop
                // the request, like the desktop's pre-round window.
                Self::finish_chase(&self.shared, &self.seek, &mut self.chasing, &mut playback, false);
                return true;
            };
            if let Err(e) = playback.load(&snap) {
                log::error!("replay seek: snapshot load failed: {e:?}");
                Self::finish_chase(&self.shared, &self.seek, &mut self.chasing, &mut playback, false);
                return true;
            }
        } else if let Some(snap) = self.keyframes.lock().unwrap().best_in_range(cur, target) {
            if let Err(e) = playback.load(&snap) {
                log::error!("replay seek: snapshot load failed: {e:?}");
                Self::finish_chase(&self.shared, &self.seek, &mut self.chasing, &mut playback, false);
                return true;
            }
        }

        // Chase forward under frameskip, re-rendering only the last
        // couple of ticks so the landing frame is real.
        let deadline = now_ms_fn() + CHASE_BUDGET_MS;
        while playback.cursor() < target {
            let remaining = target - playback.cursor();
            let skip = if remaining > 2 { i32::MAX } else { 0 };
            if !self.chasing || remaining <= 3 {
                playback.pair_mut().set_frameskip(0, skip);
                playback.pair_mut().set_frameskip(1, skip);
                self.chasing = true;
            }
            if !playback.step() {
                break;
            }
            let mut keyframes = self.keyframes.lock().unwrap();
            if keyframes.needed(playback.cursor()) {
                // Chase-captured framebuffers can be stale under
                // frameskip; seeks don't care (they only load state),
                // and previews prefer rendered neighbors anyway.
                match playback.capture() {
                    Ok(snap) => keyframes.insert(snap),
                    Err(e) => log::warn!("replay seek: capture failed: {e:?}"),
                }
            }
            drop(keyframes);
            if now_ms_fn() >= deadline {
                // Budget spent — publish progress and continue next
                // pump. Purge the burst's fast-forward audio: unlike
                // the desktop's seek worker, the pair mutex is free
                // between bursts, so the audio pull would otherwise
                // play this garble.
                for i in 0..2 {
                    playback.pair_mut().core_mut(i).audio_buffer().clear();
                }
                self.shared.stats.lock().unwrap().frontier = playback.cursor();
                return true;
            }
        }
        Self::finish_chase(&self.shared, &self.seek, &mut self.chasing, &mut playback, true);
        true
    }

    /// Land (or abandon) the chase: restore rendering, purge the
    /// fast-forward audio, publish the landing frames, and resume if
    /// the request asked to. Associated (not `&mut self`) — the caller
    /// holds the playback guard, which borrows a field of `self`.
    fn finish_chase(
        shared: &SharedSession,
        seek: &SeekState,
        chasing: &mut bool,
        playback: &mut tango_pvp::playback::Playback,
        publish: bool,
    ) {
        if *chasing {
            playback.pair_mut().set_frameskip(0, 0);
            playback.pair_mut().set_frameskip(1, 0);
            *chasing = false;
        }
        for i in 0..2 {
            playback.pair_mut().core_mut(i).audio_buffer().clear();
        }
        shared.stats.lock().unwrap().frontier = playback.cursor();
        if publish {
            let view = shared.view_player.load(Ordering::Relaxed).min(1);
            if let Some(buf) = playback.pair_mut().video_buffer(view) {
                shared.publish_video(buf);
            }
            if let Some(buf) = playback.pair_mut().video_buffer(1 - view) {
                shared.publish_video2(buf);
            }
        }
        seek.target.store(u32::MAX, Ordering::Relaxed);
        if seek.resume_after.swap(false, Ordering::Relaxed) {
            shared.resume();
        }
    }
}

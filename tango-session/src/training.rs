//! Training-mode emulator session: a real link battle you fight
//! locally, against a **dummy** on the opponent core.
//!
//! Mechanically this is a netplay match with the network cut out: the
//! game's own registration starts it, and both seats' input is supplied
//! locally before the tick advances. Both cores run the player's own ROM + save
//! (a mirror match), primed all the way into their link battle exactly
//! as a netplay match would be — so training *starts in a battle*, not
//! at the title screen. The player drives one core; the dummy on the
//! other presses nothing, so the opponent just stands there.
//!
//! The battle runs entirely off in-memory SRAM, so nothing a training
//! session does is written back to the player's `.sav` on disk. There is
//! no netcode, no throttling and no rollback churn: the dummy's input for
//! each tick is supplied locally before that tick advances, so the pair
//! runs in perfect lockstep.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::local::{Pacing, Surfaces};

use tango_match::telemetry::Event;

/// Single battle. Training always fights one round against the dummy;
/// there's no lobby to pick a mode, and the default do-nothing opponent
/// makes best-of-N pointless.
const TRAINING_MATCH_TYPE: (u8, u8) = (0, 0);

pub struct TrainingSession {
    game: &'static tango_gamesupport::Game,
    /// Which core the human currently drives (0 or 1). The player starts
    /// on core 0 with the dummy on core 1; [`swap`](Self::toggle_swap)
    /// flips it so the human takes the other side. Read every tick by the
    /// drive loop (to route input) and by the audio pull (to follow the
    /// controlled core).
    controlled: Arc<AtomicUsize>,
    joyflags: Arc<AtomicU32>,
    pacing: Pacing,
    /// The controlled core's screen, and the non-controlled core's as
    /// the picture-in-picture while that is on.
    surfaces: Surfaces,
    /// The console's screens, as the game's engine presents them —
    /// what the session's surfaces are sized for.
    layout: tango_match::ScreenLayout,
    /// Latched once the battle's own match-end path fires — flips
    /// [`is_ended`](crate::Session::is_ended) so the host tears the
    /// session down.
    ended: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl TrainingSession {
    /// Boot a training battle against the do-nothing dummy. Both cores
    /// run `rom` + `save_sram` (a mirror match); the
    /// SRAM is in-memory, so nothing persists back to disk.
    ///
    /// Primes both games into their link battle before returning — a
    /// short burst of headless emulation — so a live session is already
    /// mid-battle. Also returns the session's audio stream (the human
    /// core's samples resampled to `sample_rate`) for the host to route
    /// to its output; dropping it just costs sound.
    pub fn new(
        game: &'static tango_gamesupport::Game,
        rom: Arc<Vec<u8>>,
        save_sram: Vec<u8>,
        rtc: std::time::SystemTime,
        rng_seed: [u8; 16],
        sample_rate: u32,
    ) -> Result<(Self, Driver, crate::audio::Stream), crate::Error> {
        // The engine's local core is core 0; `advance` always feeds core
        // 0 and `add_remote_input` core 1. The player starts on core 0
        // (the dummy on core 1); a swap only re-routes which input source
        // feeds each core, so the engine's local_player stays 0. Both
        // cores run the same game (a mirror match) — training is local,
        // so there's no opponent selection.
        //
        // Present delay 0: the match is local and lockstep, so there's no
        // latency to hide and no speculation to roll back.
        // The pair pushes the seat the player is driving into the ring
        // on its way out of every tick; the stream plays the other end
        // without ever reaching for a console.
        let (audio_in, audio_out) = crate::audio::ring();
        let mut match_ = game.pvp.start(tango_match::StartConfig {
            roms: [rom.as_ref(), rom.as_ref()],
            saves: [Some(&save_sram), Some(&save_sram)],
            match_type: TRAINING_MATCH_TYPE,
            rng_seed,
            rtc,
            // A mirror match, so the peer's cartridge is this one.
            peer_rom: tango_match::PeerRom {
                code: *game.rom_code,
                revision: game.revision,
            },
            local_player: 0,
            present_delay: 0,
            disable_bgm: false,
            audio: Some(audio_in),
            // Training builds its session around an already-primed
            // pair, so the walk runs before there is anything to
            // cancel from.
            cancel: None,
        })?;

        // A netplay match renders only the local side. Training shows
        // both — the PiP and the side-swap — so ask for the whole pair.
        match_.render_seats();

        let controlled = Arc::new(AtomicUsize::new(0));
        let joyflags = Arc::new(AtomicU32::new(0));
        let pacing = Pacing::new(game);
        // A primed pair, same as netplay — the dummy seat is the pair's
        // other console, not a solo boot.
        let layout = game.pvp.screen_layout(tango_match::SessionMode::PvP {
            match_type: TRAINING_MATCH_TYPE,
        });
        let surfaces = Surfaces::new(&layout, false);
        let ended = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));

        // Audio comes off whichever core the player is driving (same
        // path as PvP), rate control following the pacing target. A swap
        // tells the match to listen to the other seat, so the sound
        // follows the player without anything here being rebuilt.
        let audio = pacing.audio_stream(audio_out, sample_rate);

        let driver = Driver {
            match_,
            controlled: controlled.clone(),
            joyflags: joyflags.clone(),
            pacing: pacing.clone(),
            surfaces: surfaces.clone(),
            ended: ended.clone(),
            stop: stop.clone(),
            confirmed_through: 0,
        };

        Ok((
            Self {
                game,
                controlled,
                joyflags,
                pacing,
                surfaces,
                ended,
                stop,
                layout,
            },
            driver,
            audio,
        ))
    }

    /// Whether the player has swapped to the non-default side (control of
    /// core 1). `false` on the side the session booted on.
    pub fn is_swapped(&self) -> bool {
        self.controlled.load(Ordering::Relaxed) != 0
    }

    /// Swap which side the player controls: the human and the dummy trade
    /// cores. Takes effect on the next tick the drive loop routes input,
    /// and the audio + main screen follow the newly-controlled core.
    pub fn toggle_swap(&self) {
        self.controlled.fetch_xor(1, Ordering::Relaxed);
    }

    /// Turn the auxiliary opponent surface on or off. The host presents
    /// that surface as either picture-in-picture or an equal second pane.
    /// Takes effect on the next published frame.
    pub fn set_opponent_visible(&self, visible: bool) {
        self.surfaces.set_pip_visible(visible);
    }
}

impl crate::Session for TrainingSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.game
    }

    fn frame(&self) -> Vec<u8> {
        self.surfaces.screen.read()
    }

    fn screen_layout(&self) -> tango_match::ScreenLayout {
        self.layout.clone()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.surfaces.wake.clone()
    }

    /// The non-controlled core's screen — `None` while the PiP is off or
    /// before its first captured frame.
    fn pip_frame(&self) -> Option<Vec<u8>> {
        self.surfaces.pip_frame()
    }

    fn set_input(&self, input: crate::HostInput) {
        // A training pair is GBA-only, so the stylus has nowhere to go.
        self.joyflags.store(input.keys, Ordering::Relaxed);
    }

    fn set_speed(&self, factor: f32) {
        self.pacing.set_speed(factor);
    }

    /// True once the battle's own match-end path fired, so the host
    /// tears the session down instead of leaving the player on a hung
    /// post-match link screen.
    fn is_ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }
}

impl Drop for TrainingSession {
    /// Tell whoever is driving to stop; the next `tick` returns false.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Everything the driver owns for the session's life.
pub struct Driver {
    match_: tango_match::Match,
    controlled: Arc<AtomicUsize>,
    joyflags: Arc<AtomicU32>,
    pacing: Pacing,
    surfaces: Surfaces,
    ended: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    /// Number of authoritative rows returned by the match so far, used as the
    /// confirmed telemetry boundary. Training discards the rows themselves.
    confirmed_through: u32,
}

impl crate::Drive for Driver {
    fn tick(&mut self) -> bool {
        Driver::tick(self)
    }

    fn fps_target(&self) -> f32 {
        self.pacing.fps_target()
    }
}

impl Driver {
    /// Advance the battle one tick: route both inputs, step the pair,
    /// publish the screens. `false` once the session has
    /// ended — the battle's own match-end path, a failed advance, or the
    /// session being dropped.
    pub fn tick(&mut self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return false;
        }
        {
            // Which core the player drives this tick; the dummy takes the
            // other. A swap flips this between ticks.
            let controlled = self.controlled.load(Ordering::Relaxed);
            let dummy_player = 1 - controlled;
            // The sound follows the player across a swap; the pair drops
            // whatever the seat being left had queued, so the old side's
            // tail never plays under the new one.
            self.match_.listen_to(controlled);

            // The dummy presses nothing.
            let dummy = 0;

            // Route each input to its core, then feed the engine: core 0
            // via `advance`, core 1 via `add_remote_input` (the engine's
            // fixed local/remote slots). Whichever core the player drives
            // gets the pad; the other gets the dummy. Both inputs for the
            // tick are present before it advances, so the pair confirms it
            // immediately — lockstep, no rollback.
            let player = self.joyflags.load(Ordering::Relaxed);
            let core0 = if controlled == 0 { player } else { dummy };
            let core1 = if controlled == 0 { dummy } else { player };
            self.match_.add_remote_input(tango_match::HostInput::keys(core1), 0);
            let advanced = match self.match_.advance(tango_match::HostInput::keys(core0)) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("training: advance failed: {e}");
                    self.ended.store(true, Ordering::Release);
                    self.surfaces.wake.notify_one();
                    return false;
                }
            };

            // Watch the confirmed telemetry for the games' own match-end
            // path so the session can tear down cleanly (with a
            // do-nothing dummy the player wins and the battle ends). We
            // don't fold stats — training records nothing.
            // Training has no replay sink, but the returned batch length is the
            // telemetry boundary for this advance. The rows then drop here.
            self.confirmed_through += advanced.confirmed_inputs.len() as u32;
            let (_samples, events) = match self.match_.telemetry() {
                Some(store) => store.lock().unwrap().take_through(self.confirmed_through),
                None => (Vec::new(), Vec::new()),
            };
            if events.iter().any(|(_, e)| matches!(e, Event::MatchEnded)) {
                self.ended.store(true, Ordering::Release);
                self.surfaces.wake.notify_one();
                return false;
            }

            // Publish the controlled core to the main screen; the other
            // core feeds the PiP while it's on.
            let main = self.match_.seat_frame(controlled);
            let other = if self.surfaces.pip_visible() {
                self.match_.seat_frame(dummy_player)
            } else {
                None
            };
            self.surfaces.publish(main.as_deref(), other.as_deref());
        }
        true
    }
}

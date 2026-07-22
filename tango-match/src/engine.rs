//! The connection-level SIO match: builds the two-player [`Link`], primes
//! both games to their link battle, then runs the rollback
//! [`Session`](mgba_siolink::session::Session) with per-tick RAM-poll
//! telemetry over it.
//!
//! This is the trap engine's `Match` analogue for the SIO engine, but
//! much smaller: there is no shadow, no per-round `Round` object, and no
//! trap-driven netcode. The host drives it directly — [`advance`] once
//! per frame with the local joypad — and reads video, telemetry, and
//! clock-sync signals back. Round boundaries and outcomes come from the
//! telemetry [`Store`](crate::telemetry::Store), not from traps.
//!
//! (mgba-siolink links go up to four players, but every game tango
//! supports is a two-player link battle, so this engine is two-player
//! throughout: the pair of cores IS the link.)
//!
//! [`Link`]: mgba_siolink::Link
//! [`advance`]: Match::advance

use mgba_siolink::{Link, LinkOptions, Peripheral, SideOptions};

use crate::telemetry::{Telemetry, TelemetryHandle};
use crate::{GameSupport, PrimeConfig};

pub use mgba_siolink::session::{Outgoing, Report};

/// Cap on priming ticks before we give up bringing the games to their
/// link battle. Real bring-up is ~470 ticks (BN6); this is generous
/// headroom for slower families without hanging forever on a wedge.
const MAX_PRIME_TICKS: u32 = 3600;

/// Everything [`Match::new`] needs. Both peers pass identical values
/// except [`local_player`](Self::local_player): the pair is symmetric, so
/// core 0 always runs `player0`'s game and core 1 `player1`'s, on both
/// peers.
pub struct MatchConfig<'a> {
    /// Per-core ROM images (already patched). `roms[i]` runs on core `i`.
    pub roms: [Vec<u8>; 2],
    /// Per-core SRAM images.
    pub saves: [Vec<u8>; 2],
    /// Per-core game support (priming + telemetry). `support[i]` drives core `i`.
    pub support: [&'a dyn GameSupport; 2],
    /// Link-battle mode selection, passed to both games' primers.
    pub match_type: (u8, u8),
    /// The negotiated match seed, for the primers' per-core RNG reseed
    /// (see [`PrimeConfig::rng_seed`](crate::PrimeConfig)).
    pub rng_seed: [u8; 16],
    /// The negotiated match clock, pinned into both carts' RTC.
    pub rtc: std::time::SystemTime,
    /// Which core this peer controls (0 or 1).
    pub local_player: usize,
    /// How many ticks behind the local frontier to present. Purely local.
    pub present_delay: u32,
    /// Silence the battle BGM (see [`PrimeConfig::disable_bgm`]). Purely
    /// local.
    pub disable_bgm: bool,
}

/// A booted, primed, running SIO match.
pub struct Match {
    session: mgba_siolink::session::Session,
    telemetry: TelemetryHandle,
    local_player: usize,
}

impl Match {
    /// Boot the pair, prime both games to their link battle, and start the
    /// rollback session. Priming runs identically on both peers (it is a
    /// pure function of ROM/save/rtc), so both reach the same state before
    /// the session — and therefore the same session initial state.
    pub fn new(config: MatchConfig) -> anyhow::Result<Self> {
        let MatchConfig {
            roms,
            saves,
            support,
            match_type,
            rng_seed,
            rtc,
            local_player,
            present_delay,
            disable_bgm,
        } = config;
        assert!(local_player < 2);
        let [rom0, rom1] = roms;
        let [save0, save1] = saves;

        let mut pair = Link::with_options(LinkOptions {
            sides: vec![
                SideOptions {
                    rom: rom0,
                    save: Some(save0),
                },
                SideOptions {
                    rom: rom1,
                    save: Some(save1),
                },
            ],
            rtc: Some(rtc),
            peripheral: Peripheral::Cable,
        })?;

        let prime_config = PrimeConfig {
            match_type,
            rng_seed,
            disable_bgm,
        };
        let lifecycle = crate::telemetry::LifecycleSink::new();
        let primed = [crate::PrimedLatch::new(), crate::PrimedLatch::new()];
        // The cores own their primer traps (see [`Link::set_traps`]): the
        // pair outlives this Match whenever a host still holds a
        // [`LinkHandle`] (e.g. the audio pull), and core teardown walks the
        // trap component, so the traps must live exactly as long as their
        // cores. They stay installed for the pair's life — inert once
        // primed, since their boot/menu addresses never execute in battle.
        pair.set_traps(0, support[0].primer_traps(&prime_config, 0, &lifecycle, &primed[0]));
        pair.set_traps(1, support[1].primer_traps(&prime_config, 1, &lifecycle, &primed[1]));

        // Prime both cores to their link battle. The traps do all the
        // driving (each core's walk its own menu state machine); the
        // pads stay idle throughout. Priming is done when both games'
        // own battle-start code has fired.
        let mut prime_ticks = 0;
        while !(primed[0].is_set() && primed[1].is_set()) {
            if prime_ticks >= MAX_PRIME_TICKS {
                anyhow::bail!("pvp: priming did not reach a link battle within {MAX_PRIME_TICKS} ticks");
            }
            pair.tick(&[0, 0]);
            prime_ticks += 1;
        }
        log::info!("pvp: primed to link battle in {prime_ticks} ticks");

        // Audio bring-up, host-side only (sample buffers aren't in
        // savestates, so the simulation is unaffected — but do it
        // identically for both cores anyway, keeping the pairs
        // configured bit-identically across the peers):
        //   * deepen the buffers from mgba's 2048-sample default — the
        //     host's queue-level rate control wants ~50 ms queued plus
        //     headroom for rollback re-simulation bursts, and 2048
        //     doesn't even hold the 50 ms at the 65536 Hz rate BN4+
        //     run at;
        //   * drop what priming piled up (it ran far faster than real
        //     time with nothing draining), so the session doesn't open
        //     on a stale burst of boot/menu sound.
        for i in 0..2 {
            let core = pair.core_mut(i);
            core.set_audio_buffer_size(16384);
            core.audio_buffer().clear();
        }

        let (telemetry, telemetry_handle) =
            Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], lifecycle);
        let mut session = mgba_siolink::session::Session::new(pair, local_player, present_delay)?;
        session.set_observer(Some(Box::new(telemetry)));

        Ok(Match {
            session,
            telemetry: telemetry_handle,
            local_player,
        })
    }

    pub fn local_player(&self) -> usize {
        self.local_player
    }

    /// Advance one frame: sample the local joypad, settle newly-confirmed
    /// inputs (rolling back on misprediction), speculate to the present
    /// target. Returns the packet to forward to the peer plus a report.
    /// Telemetry for the ticks this advanced lands in the shared store.
    pub fn advance(&mut self, local_keys: u32) -> anyhow::Result<(Outgoing, Report)> {
        Ok(self.session.advance(local_keys)?)
    }

    /// Feed one remote input packet, in tick order (see
    /// [`Session::add_remote_input`](mgba_siolink::session::Session::add_remote_input)).
    pub fn add_remote_input(&mut self, keys: u32, tick_advantage: i16) {
        self.session
            .add_remote_input(1 - self.local_player, keys, tick_advantage);
    }

    /// The shared telemetry store: round events, HP/chip/custom samples,
    /// standing outcome. Ticks above [`confirmed`](Self::confirmed) are
    /// still speculative.
    pub fn telemetry(&self) -> &TelemetryHandle {
        &self.telemetry
    }

    /// Clock-sync skew for the throttler; read before [`advance`].
    pub fn skew(&self) -> i32 {
        self.session.skew()
    }

    pub fn speculation_balance(&self) -> i32 {
        self.session.speculation_balance()
    }

    pub fn local_queue_length(&self) -> usize {
        self.session.local_queue_length()
    }

    /// Rows the next [`advance`](Match::advance) could confirm from buffered
    /// remote input alone — nonzero means advancing *drains* the local queue
    /// instead of only growing it. The drive loop's stall guard consults this
    /// so a full queue that the peer is still feeding (e.g. its resends after a
    /// reconnect) keeps settling rather than deadlocking: `advance` is the only
    /// thing that drains, so a guard that unconditionally skips it while the
    /// queue is full would leave those inputs forever unconsumed.
    pub fn matchable(&self) -> usize {
        self.session.matchable()
    }

    /// Ticks [0, confirmed) can never be rolled back again.
    pub fn confirmed(&self) -> u32 {
        self.session.confirmed()
    }

    pub fn present_delay(&self) -> u32 {
        self.session.present_delay()
    }

    pub fn set_present_delay(&mut self, present_delay: u32) {
        self.session.set_present_delay(present_delay);
    }

    /// Newest settled `(tick, digest)` for cross-peer desync detection.
    pub fn checkpoint(&self) -> Option<(u32, u32)> {
        self.session.checkpoint()
    }

    /// This session's settled digest at `tick`, if that boundary was
    /// observed (`None` = can't check, not mismatch).
    pub fn digest_at(&self, tick: u32) -> Option<u32> {
        self.session.digest_at(tick)
    }

    /// Confirmed `(tick, [p0 keys, p1 keys])` pairs in order, for the
    /// replay sink.
    pub fn drain_confirmed(&mut self) -> Vec<(u32, [u32; 2])> {
        self.session
            .drain_confirmed()
            .into_iter()
            .map(|(tick, keys)| (tick, [keys[0], keys[1]]))
            .collect()
    }

    /// Run `f` against the live pair — for video/audio readout. The pair
    /// is parked at the newest simulated tick.
    pub fn with_pair<R>(&self, f: impl FnOnce(&mut Link) -> R) -> R {
        self.session.with_link(f)
    }

    /// A cloneable, lockable handle to the live pair for readout from
    /// other threads (e.g. the host's audio callback pulling the local
    /// core's samples). Same contract as [`with_pair`](Self::with_pair).
    pub fn pair_handle(&self) -> crate::LinkHandle {
        self.session.link_handle()
    }

    /// The local player's rendered frame (native BGR555), for the
    /// frontend. `None` if that core has no buffer yet.
    pub fn local_video_buffer(&self) -> Option<Vec<u8>> {
        let player = self.local_player;
        self.session
            .with_link(|pair| pair.video_buffer(player).map(|b| b.to_vec()))
    }
}

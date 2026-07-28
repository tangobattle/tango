//! Live PvP emulator session on the SIO-lockstep engine — peer-paired
//! netplay sibling of
//! [`crate::singleplayer::SinglePlayerSession`].
//!
//! Both games run locally as an [`mgba_rollback::Link`] pair linked through
//! mgba's lockstep SIO driver: the games speak
//! their *real* link protocol over the emulated cable, and the pair is
//! the rollback unit. There is no mgba thread, no traps, and no shadow —
//! a dedicated drive thread paces the [`Match`] at the GBA frame
//! rate, feeding the local joypad in and shipping each tick's input to
//! the peer. HP/custom/chip telemetry is RAM-polled out of the
//! simulation by the engine's per-tick observer; round starts and the
//! match end are trap-driven off the games' own code paths.
//!
//! Construction is async because it has to wait for the lobby
//! background loop to release the data-channel `Receiver` (it holds it
//! through the cancel-exit path), and then for the drive thread to boot
//! and prime the pair to a live link battle. Once up, this is the same
//! kind of session the UI tick loop already knows how to draw.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_match::engine::{Match, MatchConfig};
use tango_match::telemetry;

/// GBA video framerate in frames per second.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

/// Inclusive bounds for a side's `frame_delay`, which is realized purely as
/// local frame delay (how far the display trails the netcode frontier).
/// Each side picks its own; there's no negotiation. The lobby slider and config
/// clamp to this range. 0 presents the frontier itself — pure rollback, every
/// misprediction visible immediately; the default (`default_frame_delay`, 2)
/// stays above it, and the ping-based suggestion never lands below 1.
pub const MIN_FRAME_DELAY: u32 = 0;
pub const MAX_FRAME_DELAY: u32 = 10;

pub fn suggest_frame_delay(rtt: std::time::Duration) -> u32 {
    let one_way_frames = (rtt.as_millis() * 60 / 2 / std::time::Duration::from_secs(1).as_millis()) as i32;
    (one_way_frames + 1).clamp(MIN_FRAME_DELAY as i32, MAX_FRAME_DELAY as i32) as u32
}

// Both peers must draw the player index from the same shared RNG state
// at the same point in the protocol; the construction lives in the
// shared protocol crate.
use tango_net_protocol::derive::pick_local_player_index;

/// Upper bound on how long `is_ended` waits for the peer's
/// `EndOfMatch` packet after local completion. Wide enough to
/// cover slow networks + the typical match-end animation, tight
/// enough that a crashed peer doesn't pin the UI for long.
const PEER_END_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Retransmit-heartbeat cadence for the in-match channel — one emulator frame
/// at [`EXPECTED_FPS`]. Keeps the unacked redundancy window flowing while the
/// local sim is throttled or stalled, so loss recovery isn't coupled to the
/// frame rate (see [`crate::net::InMatchTx`]).
const IN_MATCH_HEARTBEAT: std::time::Duration =
    std::time::Duration::from_nanos((1_000_000_000.0 / EXPECTED_FPS as f64) as u64);

/// Session-redraw cadence while reconnecting (~30 fps), so the give-up progress
/// bar drains smoothly even though the paused drive loop emits no frames. Purely
/// cosmetic.
const RECONNECT_UI_TICK: std::time::Duration = std::time::Duration::from_millis(33);

/// Grace window after a successful reconnect during which the stall watchdog
/// is suppressed. On reconnect the local input queue is still pegged at
/// [`crate::net::data::RECONNECT_QUEUE_LENGTH`] — that's *why* we reconnected
/// — and the resumed drive loop only drains it back down as the peer's resent
/// window arrives. Without this grace the still-high `queue_len` would re-trip
/// the stall the instant the supervisor loops back, re-pausing the drive loop
/// before it could ingest a single resend: the transport renegotiates forever
/// while the sim never resumes. The grace ends early the moment the queue
/// recovers (drops below the threshold); the deadline only bounds the wait so
/// a reconnect that genuinely fails to recover still re-trips.
const RECONNECT_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// The latching end-of-match signals, grouped so the teardown policy
/// lives in one place instead of four loose atomics on the session.
///
/// Each field starts cleared and flips exactly once as the match winds
/// down; [`PvpSession::is_ended`] combines them with the completion +
/// cancellation tokens. Every field is an `Arc`, so [`Clone`] hands out a
/// shared handle: the net receive task, the supervisor, and the drive
/// thread each keep one and raise their own signal.
#[derive(Clone, Default)]
struct EndState {
    /// Remote's in-game match-end handshake (the in-band `data::wire`
    /// `EndOfMatch` marker) arrived — raised by the net receive task
    /// ([`crate::net::PvpReceiver`]).
    /// `is_ended` honors it so the lagging side gets time to write its
    /// replay tail before we drop the data channel.
    remote_ended: Arc<AtomicBool>,
    /// Remote's channel closed (clean RTC `on_closed` or receiver `Err`)
    /// or it announced a deliberate quit (the control channel's `Goodbye`)
    /// — raised by the supervisor. No more packets are coming, so
    /// `is_ended` skips straight past the grace window.
    remote_disconnected: Arc<AtomicBool>,
    /// Wall-clock instant we first observed local completion, or `None`
    /// until then. Pulls double duty: the drive thread fires our
    /// `EndOfMatch` exactly once on the `None → Some` edge, and `is_ended`
    /// reads the stamp as the fallback grace deadline so a silent peer
    /// can't pin us forever.
    local_ended_at: Arc<Mutex<Option<web_time::Instant>>>,
}

/// Live per-frame readouts the drive thread publishes for the UI —
/// the instrument panel and sparklines read these between frames.
#[derive(Default)]
struct Metrics {
    /// Clock-sync skew the throttler reacts to (positive = we lead).
    skew: std::sync::atomic::AtomicI32,
    /// Local inputs not yet matched by a remote input.
    queue_len: AtomicU32,
    /// Speculative ticks the last advance rolled back.
    depth: AtomicU32,
    /// What the pacing loop currently targets, f32 bits (base rate minus
    /// the throttler's shave).
    fps_target: AtomicU32,
}

pub struct PvpSession {
    local_game: &'static tango_gamesupport::Game,
    /// This side's player index (P1 = 0, P2 = 1), picked once at match start and
    /// stable for the whole match. The pair is symmetric — core 0 always runs
    /// player 0's game on both peers — so this is also which core is "ours".
    local_player_index: u8,
    joyflags: Arc<AtomicU32>,
    /// Flipped once the games' own match-end path is confirmed — the
    /// direct successor of the trap engine's per-game completion hook.
    completed: Arc<AtomicBool>,
    /// Latching end-of-match signals (remote-ended / remote-disconnected /
    /// local-ended). Grouped in [`EndState`]; `is_ended` reads them
    /// alongside `completed` and `cancellation_token`.
    end: EndState,
    /// Sliding-window timestamp counter marked once per drive-loop frame —
    /// yields the true simulation TPS regardless of how often the UI polls.
    tps_counter: Arc<Mutex<TpsCounter>>,
    /// Drops fire-cancellation through the drive thread and the network
    /// tasks. On Close we cancel + drop the session, which tears the
    /// network loop down cleanly.
    cancellation_token: tokio_util::sync::CancellationToken,
    /// The peer link: owns the peer connection, both channels' halves, the
    /// latency readout, and the transparent mid-match reconnect (see
    /// [`crate::net::link`]). The supervisor task holds its own `Arc`; the
    /// session's keeps the transport alive for the match's lifetime, and its
    /// eventual drop closes the connection gracefully (DTLS close_notify → the
    /// peer's prompt EOF).
    link: Arc<crate::net::link::Link>,
    /// Live UI readouts published by the drive thread.
    metrics: Arc<Metrics>,
    pub link_code: String,
    pub remote_nickname: String,
    /// Live local frame delay — realized as the engine's present delay
    /// (how far the displayed tick trails the local input frontier). The
    /// footer slider writes it; the drive loop applies changes each frame.
    /// Purely local — never negotiated or sent to the peer.
    frame_delay: Arc<AtomicU32>,
    /// Incremental local-perspective match stats, fed by the drive thread
    /// from confirmed telemetry as rounds close. Our own `Arc` (the drive
    /// thread holds a clone), so the post-match results snapshot and the
    /// sidecar write can read it during teardown regardless of how far the
    /// background tasks have already wound down.
    stats: Arc<Mutex<tango_match::analysis::StatsBuilder>>,
    /// Where this match's replay is being recorded, or `None` if the writer
    /// failed to open. The post-match results screen offers to play it back.
    pub replay_path: Option<std::path::PathBuf>,
    /// This session's display, written by the drive thread once per
    /// simulated frame.
    screen: Arc<crate::Framebuffer>,
    /// Repaint/re-check wake — fired per frame by the drive thread and
    /// by the end-detection wires that produce no frame at all (the
    /// receive pump, the reconnect supervisor, the peer-end grace).
    wake: Arc<tokio::sync::Notify>,
    /// When the session was built, for the results screen's match duration.
    started_at: web_time::Instant,
}

/// Everything the app needs to build a PvpSession: the negotiated match
/// terms plus the live transport bundle. Drained out of the matchmaking
/// lobby (or the direct-connect path) after both sides exchanged
/// StartMatch.
pub struct PreMatchData {
    /// The transport bundle — every live handle the peer link owns for the
    /// match's lifetime (channels, peer connection, reconnect recipe).
    /// `Link::bring_up` assembles it.
    pub link_parts: crate::net::link::LinkParts,
    pub is_offerer: bool,
    pub rng_seed: [u8; 16],
    /// The match clock, milliseconds since the unix epoch: the offerer's
    /// commit-time wall clock, identical on both peers. Every core (primary,
    /// shadow, re-sim stepper) pins its cart RTC here so RTC-reading games
    /// (exe45) stay deterministic, and the replay metadata records it as `ts`
    /// so playback pins to the same value.
    pub match_ts: u64,
    pub local_save_data: Vec<u8>,
    pub remote_save_data: Vec<u8>,
    pub local_settings: tango_net_protocol::control::Settings,
    pub remote_settings: tango_net_protocol::control::Settings,
    pub link_code: String,
    pub match_type: (u8, u8),
}

// The channel/peer-conn handles aren't `Debug`; a placeholder keeps any
// enclosing message (the app carries this in a `Slot<PreMatchData>`)
// derivable, same as `Channels`.
impl std::fmt::Debug for PreMatchData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreMatchData { .. }")
    }
}

/// Where a match's recording goes.
///
/// The session composes the name — it encodes the timestamp, the
/// matchup and which seat we were, and both peers derive their own —
/// and hands over the bytes. What a name *refers to* is the host's:
/// a file under the replays directory on a desktop, a row in an object
/// store in a browser. This is the one place the live match assumed a
/// filesystem, and a browser is the host that doesn't have one.
pub trait ReplayStore: crate::platform::WasmNotSend + crate::platform::WasmNotSync {
    /// Open a recording. `name` carries no extension and no directory.
    fn create(&self, name: &str) -> std::io::Result<Recording>;

    /// The directory recordings land in, if they land in one. `None`
    /// for a host with no filesystem — which is also the host with no
    /// match-stats sidecar to key against it.
    fn root(&self) -> Option<&Path> {
        None
    }
}

/// An open recording: where the bytes go, and how to find it again.
pub struct Recording {
    /// `Send` because [`tango_replay::Writer`] holds it as
    /// `Box<dyn Write + Send>`, which is what lets a desktop host move
    /// a match onto a thread.
    pub sink: Box<dyn std::io::Write + Send>,
    /// How this recording is identified afterwards — the file's path
    /// where there are files, and otherwise whatever key the host
    /// filed it under. Surfaced as
    /// [`PvpSession::replay_path`](PvpSession::replay_path).
    pub key: std::path::PathBuf,
}

/// [`ReplayStore`] over a directory: the desktop's, and the behaviour
/// the live match had built in before the trait existed.
#[cfg(not(target_arch = "wasm32"))]
pub struct DirReplayStore(pub std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl ReplayStore for DirReplayStore {
    fn create(&self, name: &str) -> std::io::Result<Recording> {
        std::fs::create_dir_all(&self.0)?;
        let key = self.0.join(format!("{name}.{}", tango_replay::EXTENSION));
        log::info!("pvp: opening replay file {}", key.display());
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&key)?;
        Ok(Recording {
            sink: Box::new(file),
            key,
        })
    }

    fn root(&self) -> Option<&Path> {
        Some(&self.0)
    }
}

/// Everything [`PvpSession::new`] needs, as named fields. Assembled by
/// the app's `spawn_pvp` glue.
pub struct PvpSessionArgs<'a> {
    /// Local/remote game impls; the roms must already have any patch applied.
    pub local_game: &'static tango_gamesupport::Game,
    pub local_rom: Arc<Vec<u8>>,
    pub remote_game: &'static tango_gamesupport::Game,
    pub remote_rom: Arc<Vec<u8>>,
    /// The netplay handoff: negotiated terms + the transport bundle.
    pub pre_match: crate::pvp::PreMatchData,
    /// This side's frame delay — realized purely as local display lag (the
    /// engine's present delay). Comes straight from local config; never
    /// negotiated with or sent to the peer.
    pub frame_delay: u32,
    /// Silence the battle BGM (the primers skip the games' battle-start
    /// music call). Comes straight from local config; never negotiated
    /// with or sent to the peer — sound-driver state never feeds battle
    /// logic.
    pub disable_bgm: bool,
    /// Where the match is recorded, or `None` not to record it.
    pub replays: Option<&'a dyn ReplayStore>,
    /// Where the match-stats sidecar goes. Native-only: the cache is a
    /// file keyed against the store's directory, and a host whose
    /// [`ReplayStore`] has no [`root`](ReplayStore::root) has nowhere
    /// to put one and nothing to key it by.
    #[cfg(not(target_arch = "wasm32"))]
    pub cache_path: &'a Path,
    /// The host output rate the session's audio stream resamples to.
    pub sample_rate: u32,
}

impl PvpSession {
    /// Build the live match from [`PvpSessionArgs`].
    ///
    /// Async because the lobby loop holds the data-channel `Receiver`
    /// until it observes its cancellation and exits (`Link::bring_up`
    /// awaits its handback), and because the drive thread then boots and
    /// primes the pair — a couple of seconds of emulation — before the
    /// session is live. Also returns the session's audio stream (the
    /// local core's samples at the args' `sample_rate`, rate control
    /// following the drive loop's published fps target) for the host to
    /// route to its output.
    pub async fn new(args: PvpSessionArgs<'_>) -> Result<(Self, PvpBoot), crate::Error> {
        let PvpSessionArgs {
            local_game,
            local_rom,
            remote_game,
            remote_rom,
            pre_match,
            frame_delay,
            disable_bgm,
            replays,
            #[cfg(not(target_arch = "wasm32"))]
            cache_path,
            sample_rate,
        } = args;
        let cancellation_token = tokio_util::sync::CancellationToken::new();

        // Parse both sides' committed SRAM dumps. PvP runs entirely off
        // these in-memory images — writes don't persist back to anyone's
        // .sav file.
        let remote_save = remote_game
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| crate::Error::ParseSave {
                side: "remote",
                source: e,
            })?;
        let local_save = local_game
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| crate::Error::ParseSave {
                side: "local",
                source: e,
            })?;

        let local_sio = local_game.pvp;
        let remote_sio = remote_game.pvp;

        // Player index off the shared RNG seed, same negotiation as ever:
        // both peers derive the same assignment, mirrored.
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(pre_match.rng_seed);
        let local_player_index = pick_local_player_index(&mut rng, pre_match.is_offerer);

        // The match clock, pinned into both carts' RTC and recorded in the
        // replay metadata so playback re-primes to the identical state.
        let rtc_time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(pre_match.match_ts);

        // Replay writer. Failing to open it shouldn't kill the
        // match — log and continue without recording.
        let (replay_writer, replay_path) = match replays {
            None => (None, None),
            Some(store) => match build_replay_writer(
                store,
                &pre_match,
                local_player_index,
                local_save.as_ref(),
                remote_save.as_ref(),
            ) {
                Ok((writer, path)) => (Some(writer), Some(path)),
                Err(e) => {
                    log::warn!("pvp: replay writer open failed: {e}");
                    (None, None)
                }
            },
        };

        // Assemble the peer link from the lobby handoff — this awaits the
        // lobby loop releasing the reliable receiver (typically a few ms after
        // take_pre_match flipped the cancel) and starts the in-match
        // retransmit heartbeat.
        let link = Arc::new(
            crate::net::link::Link::bring_up(pre_match.link_parts, IN_MATCH_HEARTBEAT, cancellation_token.clone())
                .await?,
        );
        let in_match = link.in_match().clone();

        let end = EndState::default();
        let joyflags = Arc::new(AtomicU32::new(0));
        let completed = Arc::new(AtomicBool::new(false));
        let frame_delay = Arc::new(AtomicU32::new(frame_delay));
        let metrics = Arc::new(Metrics::default());
        let drive_paused = Arc::new(crate::PauseGate::new(false));
        // ~1 s window at 60 Hz, matching the legacy emu_tps_counter.
        let tps_counter = Arc::new(Mutex::new(TpsCounter::new(60)));
        let screen = crate::Framebuffer::new();
        let wake = Arc::new(tokio::sync::Notify::new());
        // The two-sided ready gate. Priming takes as long as the machine
        // running it takes, so each peer announces when its own pair
        // reaches the link battle and neither ticks until both have.
        let local_primed = Arc::new(AtomicBool::new(false));
        let peer_primed = link.peer_primed();
        let announce_primed = Arc::new(tokio::sync::Notify::new());

        // Usage semantics can depend on the applied patch (exe45's PvP
        // patch), so they're probed off the patched ROM.
        let stats = Arc::new(Mutex::new(tango_match::analysis::StatsBuilder::new(
            local_game.pvp.chip_semantics(local_rom.as_ref()),
            local_game.pvp.counts_buster(local_rom.as_ref()),
        )));

        // Remote input events flow receive-task → drive thread over this
        // queue; the rennet reassembly in PvpReceiver already ordered and
        // deduplicated them (one Input per remote tick, in tick order).
        let (event_tx, event_rx) = std::sync::mpsc::channel::<crate::net::data::Input>();

        // The sender pump: the drive thread pushes one Input per advance;
        // the pump ships each as a rennet frame over the unreliable channel.
        let sender = crate::net::PvpSender::new(in_match.clone());

        // Pair-order arrays: core 0 always runs player 0's game, on both
        // peers, so priming and simulation are bit-identical across the pair.
        let (roms, saves, supports) = if local_player_index == 0 {
            (
                [local_rom.as_ref().clone(), remote_rom.as_ref().clone()],
                [local_save.to_sram_dump(), remote_save.to_sram_dump()],
                [local_sio, remote_sio],
            )
        } else {
            (
                [remote_rom.as_ref().clone(), local_rom.as_ref().clone()],
                [remote_save.to_sram_dump(), local_save.to_sram_dump()],
                [remote_sio, local_sio],
            )
        };

        // Everything the match needs to boot, handed back for the host
        // to run: the pair is single-threaded by design and priming it
        // is seconds of emulation, so *where* that happens is the host's
        // call — a blocking thread on a desktop, the event loop in a
        // browser.
        let boot = PvpBoot {
            pieces: BootPieces {
                roms,
                saves,
                supports,
                match_type: pre_match.match_type,
                rng_seed: pre_match.rng_seed,
                rtc: rtc_time,
                local_player: local_player_index as usize,
                present_delay: frame_delay.load(Ordering::Relaxed),
                disable_bgm,
            },
            drive: DriveContext {
                joyflags: joyflags.clone(),
                frame_delay: frame_delay.clone(),
                metrics: metrics.clone(),
                drive_paused: drive_paused.clone(),
                cancel: cancellation_token.clone(),
                completed: completed.clone(),
                end: end.clone(),
                event_rx,
                sender,
                in_match: in_match.clone(),
                replay_writer,
                stats: stats.clone(),
                // Keyed against the store's own directory, so a store
                // that isn't one (a browser's) simply has no sidecar —
                // which is also the only kind of host that has nowhere
                // to write it.
                #[cfg(not(target_arch = "wasm32"))]
                stats_path: replay_path
                    .as_ref()
                    .zip(replays.and_then(|store| store.root()))
                    .map(|(path, root)| crate::stats::stats_path(cache_path, root, path)),
                tps_counter: tps_counter.clone(),
                screen: screen.clone(),
                wake: wake.clone(),
                local_primed: local_primed.clone(),
                peer_primed: peer_primed.clone(),
                announce_primed: announce_primed.clone(),
                local_player: local_player_index as usize,
            },
            sample_rate,
            local_player: local_player_index as usize,
            metrics: metrics.clone(),
        };

        // Announce our own prime as soon as the boot finishes. It rides
        // the reliable control channel, so one accepted send is
        // delivered — but the boot can outlast a transport, so retry
        // until one is accepted (and the supervisor re-announces after a
        // reconnect, whose rebuild drops anything unacked).
        spawn_primed_announcer(
            link.clone(),
            local_primed.clone(),
            announce_primed,
            cancellation_token.clone(),
        );

        // Receive pump + link supervisor: reads peer frames into the event
        // queue, watches for stalls, and runs the transparent reconnect.
        spawn_supervisor(SupervisorContext {
            link: link.clone(),
            in_match,
            event_tx,
            end: end.clone(),
            completed: completed.clone(),
            cancel: cancellation_token.clone(),
            metrics: metrics.clone(),
            drive_paused: drive_paused.clone(),
            wake: wake.clone(),
            local_primed,
        });

        let session = Self {
            local_game,
            local_player_index,
            joyflags,
            completed,
            end,
            tps_counter,
            cancellation_token,
            link,
            metrics,
            link_code: pre_match.link_code,
            remote_nickname: pre_match.remote_settings.nickname,
            frame_delay,
            stats,
            replay_path,
            screen,
            wake,
            started_at: web_time::Instant::now(),
        };
        Ok((session, boot))
    }

    /// This side's player index (P1 = 0, P2 = 1) for the match. Stable across
    /// rounds, so the instrument panel's P1/P2 tag reads it directly rather than
    /// pulling it from the per-round [`RoundStats`].
    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    /// Current local frame delay — drives the footer slider's
    /// displayed value.
    pub fn frame_delay(&self) -> u32 {
        self.frame_delay.load(Ordering::Relaxed)
    }

    /// Live-set the local frame delay. Purely local: the drive loop applies
    /// it as the engine's present delay on the next frame, no peer
    /// coordination. Clamped to the supported range as a guard against an
    /// out-of-range caller.
    pub fn set_frame_delay(&self, frame_delay: u32) {
        self.frame_delay
            .store(frame_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY), Ordering::Relaxed);
    }

    /// `true` while the link has dropped and the session is transparently
    /// rebuilding it (direct or matchmaking) — the drive loop is paused and the
    /// PvP view shows a "Reconnecting…" overlay.
    pub fn is_reconnecting(&self) -> bool {
        matches!(self.link.health(), crate::net::link::LinkHealth::Reconnecting { .. })
    }

    /// Fraction of the reconnect give-up window still remaining — `1.0` when a
    /// reconnect just started, falling to `0.0` at the give-up deadline, or
    /// `None` when not reconnecting. Drives the overlay's depleting progress bar.
    pub fn reconnect_progress(&self) -> Option<f32> {
        match self.link.health() {
            crate::net::link::LinkHealth::Reconnecting { started, give_up_at } => {
                let total = give_up_at
                    .saturating_duration_since(started)
                    .as_secs_f32()
                    .max(f32::EPSILON);
                let remaining = give_up_at
                    .saturating_duration_since(web_time::Instant::now())
                    .as_secs_f32();
                Some((remaining / total).clamp(0.0, 1.0))
            }
            _ => None,
        }
    }

    /// Whether the match ran to its natural end (a final round ended and
    /// the runout elapsed) — as opposed to ending by disconnect or quit.
    /// Gates the post-match results screen.
    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::Acquire)
    }

    /// Whether the match ended because the remote vanished mid-match —
    /// the peer announced a quit, its channel EOF'd (crash, its own
    /// give-up) or the reconnect window expired (see the link
    /// supervisor). Never set by our own quit paths. Gates the
    /// disconnect dress of the results screen.
    pub fn remote_disconnected(&self) -> bool {
        self.end.remote_disconnected.load(Ordering::Acquire)
    }

    /// Median ping over the last few seconds — drives the frame-delay
    /// suggestion, where smoothing out a transient spike is what we want.
    /// `Some(ZERO)` until the first sample arrives, then `Some(median)`
    /// while the link is up; `None` once the remote drops (the supervisor
    /// retires the link's counter). The UI keys the instrument panel off this:
    /// `None` means "no live link".
    pub fn latency(&self) -> Option<std::time::Duration> {
        self.link.latency()
    }

    /// Raw latest ping — the most recent single measurement, unsmoothed.
    /// Drives the live telemetry plate + sparkline, where the median's lag
    /// would mask a real spike. Same `Some`/`None` link-up semantics as
    /// [`latency`](Self::latency) (both read the same counter), so it gates
    /// the instrument panel identically; only the reported value differs.
    pub fn latency_raw(&self) -> Option<std::time::Duration> {
        self.link.latency_raw()
    }

    /// Smoothed simulation ticks-per-second from the drive loop's
    /// per-frame marks. Independent of UI refresh rate. ZERO until the
    /// second sample lands.
    pub fn tps(&self) -> f32 {
        let mean = self.tps_counter.lock().unwrap().mean_duration();
        if mean.is_zero() {
            0.0
        } else {
            1.0 / mean.as_secs_f32()
        }
    }

    /// What the pacing loop is currently targeting. Pairs with `tps()` —
    /// gap between the two tells you whether the throttler is the cause of
    /// a slow tps or just observing one.
    pub fn fps_target(&self) -> f32 {
        f32::from_bits(self.metrics.fps_target.load(Ordering::Relaxed))
    }

    /// The match stats aggregated so far, in play order. Read at teardown
    /// for the post-match results screen; a round the match never
    /// finished (mid-round disconnect) is in it with its telemetry
    /// intact and no outcome — only the verdict is missing.
    pub fn stats_snapshot(&self) -> tango_match::analysis::MatchStats {
        self.stats.lock().unwrap().snapshot()
    }

    /// How long the match ran, start of session to local completion — or to
    /// now, if completion hasn't been observed yet (it is stamped a frame
    /// after the completion flag flips). For the results screen.
    pub fn match_duration(&self) -> std::time::Duration {
        match *self.end.local_ended_at.lock().unwrap() {
            Some(ended_at) => ended_at.duration_since(self.started_at),
            None => self.started_at.elapsed(),
        }
    }

    /// Snapshot of the live netcode metrics for the status bar
    /// (skew, lead, rollback depth). Always available while the
    /// session runs — the SIO engine's simulation never stops
    /// between rounds.
    pub fn round_stats(&self) -> Option<RoundStats> {
        Some(RoundStats {
            skew: self.metrics.skew.load(Ordering::Relaxed),
            lead: self.metrics.queue_len.load(Ordering::Relaxed) as i32,
            depth: self.metrics.depth.load(Ordering::Relaxed),
        })
    }
}

impl crate::Session for PvpSession {
    fn local_game(&self) -> &'static tango_gamesupport::Game {
        self.local_game
    }

    fn frame(&self) -> Vec<u8> {
        self.screen.read()
    }

    fn wake(&self) -> Arc<tokio::sync::Notify> {
        self.wake.clone()
    }

    fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, Ordering::Relaxed);
    }

    fn request_close(&self) {
        // Cancelling the token is the whole close signal: it stops the
        // drive thread and the supervisor and flips `is_ended`'s
        // cancellation check. The supervisor announces the quit to the
        // peer (best-effort `Goodbye`) on its way out, so the peer ends
        // at once instead of trying to reconnect to us.
        self.cancellation_token.cancel();
    }

    /// True once it's safe to tear the session down. Requires
    /// local completion (the deciding round's end confirmed + runout)
    /// PLUS one of:
    ///   * the peer also sent us `EndOfMatch`, or
    ///   * `PEER_END_GRACE` has elapsed since local completion
    ///     (peer crashed / disconnected — give up waiting).
    ///
    /// The handshake keeps the data channel alive long enough
    /// for the lagging side to also confirm its end and write
    /// its replay tail before we drop the connection. Without it,
    /// whichever side finishes first kills the connection out
    /// from under the other and the other side's replay ends up
    /// truncated.
    fn is_ended(&self) -> bool {
        // The dead-link checks come before the completion gate: a match
        // that ends by disconnect (the peer quit, the reconnect window
        // expired, our own netcode tore down) is over whether or not it
        // ever completed — leaving these behind `completed` stranded
        // mid-match disconnects on a frozen session forever.
        //
        // Remote's data channel closed (RTCPeerConnection drop or
        // SCTP-level disconnect): no EndOfMatch is ever coming, so skip
        // straight to teardown without burning the grace window.
        if self.end.remote_disconnected.load(Ordering::Acquire) {
            return true;
        }
        // We tore our own netcode down. Same rationale, from our side.
        if self.cancellation_token.is_cancelled() {
            return true;
        }
        if !self.completed.load(Ordering::Acquire) {
            return false;
        }
        if self.end.remote_ended.load(Ordering::Acquire) {
            return true;
        }
        match *self.end.local_ended_at.lock().unwrap() {
            Some(t) => t.elapsed() >= PEER_END_GRACE,
            // The completion flag can flip before the drive loop
            // observes it and stamps the deadline. Hold off
            // teardown for one extra tick rather than firing the
            // grace timer from t=0.
            None => false,
        }
    }
}

/// Subset of the engine's per-frame metrics surfaced in the status bar.
#[derive(Clone, Copy, Debug)]
pub struct RoundStats {
    /// Real-time clock skew the throttler reacts to (see
    /// [`tango_match::Throttler`]). The symmetric network term cancels
    /// in the difference, so this reads ~0 at clock sync, positive when
    /// we're leading (and being slowed), and negative when the peer is
    /// leading.
    pub skew: i32,
    /// Local tick lead: how many local inputs are still unmatched by a
    /// confirmed remote input. Steady around the wire latency at clock
    /// sync; ramps up when the remote falls behind or a delivery stall
    /// holds its confirmed frontier still.
    pub lead: i32,
    /// Misprediction depth: how many speculative frames the last advance
    /// discarded and re-simulated because a confirmed remote input
    /// contradicted the prediction. 0 on a clean frame; spikes mark the
    /// size of each rollback.
    pub depth: u32,
}

impl std::fmt::Debug for PvpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PvpSession")
            .field("link_code", &self.link_code)
            .field("remote_nickname", &self.remote_nickname)
            .finish_non_exhaustive()
    }
}

impl Drop for PvpSession {
    fn drop(&mut self) {
        // Belt-and-suspenders: even if request_close wasn't
        // called, cancelling the token signals the drive thread and
        // network tasks to wind down.
        self.cancellation_token.cancel();
    }
}

// ---------------------------------------------------------------------------
// The drive thread: boots + primes the pair, then paces the session.

/// What the drive thread needs to boot the [`Match`].
struct BootPieces {
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    supports: [&'static (dyn tango_match::GameSupport + Send + Sync); 2],
    match_type: (u8, u8),
    rng_seed: [u8; 16],
    rtc: std::time::SystemTime,
    local_player: usize,
    present_delay: u32,
    disable_bgm: bool,
}

struct DriveContext {
    joyflags: Arc<AtomicU32>,
    frame_delay: Arc<AtomicU32>,
    metrics: Arc<Metrics>,
    drive_paused: Arc<crate::PauseGate>,
    cancel: tokio_util::sync::CancellationToken,
    completed: Arc<AtomicBool>,
    end: EndState,
    event_rx: std::sync::mpsc::Receiver<crate::net::data::Input>,
    sender: crate::net::PvpSender,
    in_match: crate::net::InMatchTx,
    replay_writer: Option<tango_replay::Writer>,
    stats: Arc<Mutex<tango_match::analysis::StatsBuilder>>,
    /// Where this match's stats sidecar goes. Native-only: the cache is
    /// a file next to the replay, and a browser has no such place.
    #[cfg(not(target_arch = "wasm32"))]
    stats_path: Option<std::path::PathBuf>,
    tps_counter: Arc<Mutex<TpsCounter>>,
    screen: Arc<crate::Framebuffer>,
    wake: Arc<tokio::sync::Notify>,
    /// The ready gate. `local_primed` is set (and `announce_primed`
    /// notified) the moment our pair reaches its link battle;
    /// `peer_primed` is latched by the link's control watch when the
    /// peer says the same. [`PvpDriver::tick`] advances nothing until
    /// both are set.
    local_primed: Arc<AtomicBool>,
    peer_primed: Arc<AtomicBool>,
    announce_primed: Arc<tokio::sync::Notify>,
    local_player: usize,
}

impl DriveContext {
    /// Boot the match, then hand back the driver that runs it and a
    /// readout handle to its pair.
    fn boot(mut self, pieces: BootPieces) -> Result<(PvpDriver, tango_match::LinkHandle), tango_match::Error> {
        let match_ = Match::new(MatchConfig {
            roms: pieces.roms,
            saves: pieces.saves,
            support: [pieces.supports[0], pieces.supports[1]],
            match_type: pieces.match_type,
            rng_seed: pieces.rng_seed,
            rtc: pieces.rtc,
            local_player: pieces.local_player,
            present_delay: pieces.present_delay.clamp(MIN_FRAME_DELAY, MAX_FRAME_DELAY),
            disable_bgm: pieces.disable_bgm,
        })?;
        let pair_handle = match_.pair_handle();

        // Our half of the ready gate: the pair is at its link battle.
        // Release the announcer so the peer learns it — priming ran at
        // whatever speed this machine manages, and until both sides are
        // here neither may advance a tick.
        self.local_primed.store(true, Ordering::Release);
        self.announce_primed.notify_one();

        if let Some(w) = self.replay_writer.as_mut() {
            // The SIO stream is one continuous run of pair ticks; the
            // container wants at least one open round.
            let _ = w.start_round();
        }

        Ok((
            PvpDriver {
                ctx: self,
                match_,
                throttler: tango_match::Throttler::new(),
                pending_buttons: std::collections::VecDeque::new(),
                first_round_started: false,
                pending_round_marks: std::collections::VecDeque::new(),
                fired_end_of_match: false,
            },
            pair_handle,
        ))
    }

    /// Fold a batch of confirmed telemetry into the stats builder (the
    /// shared [`tango_match::analysis::fold_confirmed`], so live stats
    /// and offline re-analysis stay byte-equivalent) and drive the round
    /// lifecycle off the events.
    fn fold_confirmed_telemetry(
        &mut self,
        samples: Vec<(u32, telemetry::BattleObs)>,
        events: Vec<(u32, telemetry::RoundEvent)>,
        pending_buttons: &mut std::collections::VecDeque<(u32, [u32; 2])>,
    ) {
        let mut stats = self.stats.lock().unwrap();
        tango_match::analysis::fold_confirmed(&mut stats, self.local_player, samples, events, &mut |tick| {
            // Discard input pairs older than this sample; the front pair
            // at the sample's tick carries its buttons. Samples arrive
            // tick-ascending, so this never skips a later sample's pair.
            while pending_buttons.front().is_some_and(|(t, _)| *t < tick) {
                pending_buttons.pop_front();
            }
            match pending_buttons.front() {
                Some(&(t, keys)) if t == tick => Some(keys),
                _ => None,
            }
        });
    }
}

// ---------------------------------------------------------------------------
// The ready gate's announcer.

/// How long to wait before retrying a rejected `Primed` announcement.
/// The peer's gate stays shut until one lands, so this is a retry
/// cadence rather than a give-up timer — only cancellation stops it.
const PRIMED_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Tell the peer our pair is primed, once the boot says so.
///
/// A task of its own because the boot is synchronous and the host
/// chooses where it runs — a blocking thread on the desktop, the event
/// loop in a browser — so it can't await a send itself.
fn spawn_primed_announcer(
    link: Arc<crate::net::link::Link>,
    local_primed: Arc<AtomicBool>,
    announce: Arc<tokio::sync::Notify>,
    cancel: tokio_util::sync::CancellationToken,
) {
    crate::platform::spawn(async move {
        while !local_primed.load(Ordering::Acquire) {
            tokio::select! {
                _ = cancel.cancelled() => return,
                // `notify_one` before the first `notified()` still wakes
                // it: Notify holds the permit. Re-checking the flag on
                // each pass is what makes the ordering irrelevant.
                _ = announce.notified() => {}
            }
        }
        loop {
            match link.send_primed().await {
                Ok(()) => {
                    log::info!("pvp: announced primed");
                    return;
                }
                Err(e) => log::debug!("pvp: primed announce failed, retrying: {e}"),
            }
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = crate::platform::sleep(PRIMED_RETRY_INTERVAL) => {}
            }
        }
    });
}

// ---------------------------------------------------------------------------
// The receive pump + link supervisor.

struct SupervisorContext {
    link: Arc<crate::net::link::Link>,
    in_match: crate::net::InMatchTx,
    event_tx: std::sync::mpsc::Sender<crate::net::data::Input>,
    end: EndState,
    completed: Arc<AtomicBool>,
    cancel: tokio_util::sync::CancellationToken,
    metrics: Arc<Metrics>,
    drive_paused: Arc<crate::PauseGate>,
    wake: Arc<tokio::sync::Notify>,
    /// Whether our own pair is primed, so a reconnect can re-announce it
    /// (the rebuild drops anything the old transport hadn't delivered).
    local_primed: Arc<AtomicBool>,
}

/// Pump one receiver until error/EOF, forwarding events to the drive
/// thread. Returns when the channel dies (the reconnect decision is the
/// supervisor's).
async fn run_receive_pump(
    mut receiver: crate::net::PvpReceiver,
    event_tx: std::sync::mpsc::Sender<crate::net::data::Input>,
    wake: Arc<tokio::sync::Notify>,
) -> std::io::Error {
    loop {
        match receiver.receive().await {
            Ok(input) => {
                if event_tx.send(input).is_err() {
                    return std::io::Error::new(std::io::ErrorKind::BrokenPipe, "drive thread gone");
                }
                // Remote inputs settle ticks; make sure a paused/idle UI
                // still observes progress.
                wake.notify_one();
            }
            Err(e) => return e,
        }
    }
}

/// Receive loop + link supervisor. Reads peer frames into the drive
/// thread's queue until the match ends (completion / cancel) or, when the
/// link drops, until reconnection gives up. Policy lives here — deciding
/// when a trip is worth reconnecting and freezing/unfreezing the drive
/// loop around the attempt. The transport surgery (silent teardown,
/// rebuild, hot-swap under the persistent rennet streams) is
/// [`crate::net::link::Link::reconnect`]'s; the lockstep sim treats the
/// whole gap as a pause, so no state resync is needed.
fn spawn_supervisor(ctx: SupervisorContext) {
    let SupervisorContext {
        link,
        in_match,
        event_tx,
        end,
        completed,
        cancel,
        metrics,
        drive_paused,
        wake,
        local_primed,
    } = ctx;

    let make_receiver = {
        let link = link.clone();
        let in_match = in_match.clone();
        let end = end.clone();
        let wake = wake.clone();
        move || -> Option<crate::net::PvpReceiver> {
            Some(crate::net::PvpReceiver::new(
                link.take_match_receiver()?,
                in_match.clone(),
                link.latency_handle(),
                end.remote_ended.clone(),
                wake.clone(),
            ))
        }
    };

    crate::platform::spawn(async move {
        // Why the receive loop ended this iteration.
        enum Trip {
            /// Clean local teardown (user closed / cancelled). Announces the
            /// quit to the peer (best-effort `Goodbye`), never reconnects.
            Cancelled,
            /// The peer announced a deliberate quit (the control channel's
            /// `Goodbye`): it is leaving and will never be at a rendezvous,
            /// so the match ends at once — no reconnect window at all.
            PeerQuit,
            /// A channel hit EOF without a goodbye — the peer's reconnect
            /// dropping its old transport (libdatachannel closes gracefully;
            /// there is no silent teardown), its transport declaring the
            /// link dead, or a quit whose goodbye was lost. We can't tell
            /// those apart, so this reconnects on a *short* window: a real
            /// drop's peer is already waiting at the rendezvous and rejoins
            /// in a second or two, while a lost-goodbye quit finds no one
            /// there and ends quickly.
            Closed,
            /// The local input queue climbed to `RECONNECT_QUEUE_LENGTH`:
            /// the peer stopped matching our inputs, i.e. a quiet/dead link.
            /// Reconnects on the full per-transport window.
            Stalled,
        }

        let mut receiver = make_receiver().expect("bring_up parks the in-match receiver");
        // Set after each successful reconnect to `now + RECONNECT_DRAIN_GRACE`:
        // the stall watch stays quiet until the queue recovers or this passes,
        // so the just-reconnected (still-full) queue can't instantly re-trip.
        let mut drain_until: Option<web_time::Instant> = None;
        loop {
            // The stall watch: poll the drive thread's published queue
            // length. Coarse (10 Hz) is fine — the queue takes seconds to
            // climb to the trip point. A queue below the threshold clears any
            // post-reconnect drain grace (the link recovered); a queue at or
            // above it trips a stall — unless we're still inside the grace,
            // where a high queue is the expected pre-drain state.
            let stall_watch = async {
                loop {
                    let queue_len = metrics.queue_len.load(Ordering::Relaxed) as usize;
                    if queue_len < crate::net::data::RECONNECT_QUEUE_LENGTH {
                        drain_until = None;
                    } else if drain_until.is_none_or(|t| web_time::Instant::now() >= t) {
                        return;
                    }
                    crate::platform::sleep(std::time::Duration::from_millis(100)).await;
                }
            };
            let trip = tokio::select! {
                biased;
                _ = cancel.cancelled() => Trip::Cancelled,
                end = link.watch_control() => match end {
                    crate::net::link::ControlEnd::Goodbye => {
                        log::info!("pvp: peer announced a quit");
                        Trip::PeerQuit
                    }
                    crate::net::link::ControlEnd::Eof => Trip::Closed,
                },
                e = run_receive_pump(receiver, event_tx.clone(), wake.clone()) => {
                    log::info!("pvp in-match channel closed: {e:?}");
                    Trip::Closed
                }
                _ = stall_watch => Trip::Stalled,
            };

            // Our own deliberate close: announce it, then stop. The
            // session's teardown drops the peer connection gracefully, and
            // its DTLS close_notify hands the peer a prompt EOF — but that
            // EOF alone is ambiguous over there (our reconnect's transport
            // drop looks identical), so send the goodbye first to let the
            // peer end at once instead of burning its clean-close reconnect
            // window on us. Best-effort: if it's lost, that window is the
            // fallback.
            if matches!(trip, Trip::Cancelled) {
                link.send_goodbye().await;
                break;
            }

            // Reconnect on any mid-match link loss — a stalled input queue
            // *or* a bare channel close — as long as the transport can
            // rebuild and the match isn't ending (our completion or the
            // peer's EndOfMatch). A close uses the short give-up window, so
            // a real drop reconnects fast while a lost-goodbye quit still
            // ends quickly. An announced quit (`PeerQuit`) never reconnects
            // — the peer told us it isn't coming back.
            let reconnectable = matches!(trip, Trip::Stalled | Trip::Closed)
                && link.can_reconnect()
                && !completed.load(Ordering::Acquire)
                && !end.remote_ended.load(Ordering::Acquire);
            if !reconnectable {
                end.remote_disconnected.store(true, Ordering::Release);
                cancel.cancel();
                break;
            }

            // Freeze the drive loop so its speculative lead can't run past
            // the rollback horizon while the link is down. Both peers
            // converge on the rebuild: whoever trips first goes silent,
            // which stall-trips the other within RECONNECT_QUEUE_LENGTH
            // frames.
            drive_paused.set(true);
            wake.notify_one();
            log::info!("pvp link dropped — pausing to reconnect");

            // Rebuild + hot-swap (the link's job), ticking the UI at
            // ~30 fps so the give-up bar drains smoothly while the paused
            // drive loop produces no frames.
            let restored = {
                let ui_tick = async {
                    let mut iv = crate::platform::Ticker::immediate(RECONNECT_UI_TICK);
                    loop {
                        iv.tick().await;
                        wake.notify_one();
                    }
                };
                let cause = if matches!(trip, Trip::Closed) {
                    crate::net::link::ReconnectCause::CleanClose
                } else {
                    crate::net::link::ReconnectCause::Stall
                };
                tokio::select! {
                    restored = link.reconnect(cause) => restored,
                    _ = ui_tick => unreachable!(),
                }
            };

            if !restored {
                // Timed out or cancelled — give up and end the match.
                end.remote_disconnected.store(true, Ordering::Release);
                cancel.cancel();
                drive_paused.set(false);
                break;
            }

            // Fresh receiver over the swapped channel, same `in_match` —
            // the rennet in-stream (seq/ack) carries across the swap, so
            // the peer's resent window fills our gap contiguously. The
            // drive loop resumes; its stall guard holds it below the
            // horizon until the resends drain the queue.
            receiver = make_receiver().expect("reconnect parks a fresh receiver");
            // The swap rebuilds the control channel too, so an earlier
            // `Primed` may have died with the old transport. Say it again:
            // a peer still waiting on the ready gate has nothing else to
            // wait for, and one it has already latched is a no-op.
            if local_primed.load(Ordering::Acquire) {
                if let Err(e) = link.send_primed().await {
                    log::debug!("pvp: primed re-announce after reconnect failed: {e}");
                }
            }
            // Hold the stall watch off until the resumed drive loop drains the
            // still-full queue back below the threshold (or the grace lapses),
            // so the stale-high `queue_len` can't instantly re-trip the stall
            // and bounce us straight back into another reconnect.
            drain_until = Some(web_time::Instant::now() + RECONNECT_DRAIN_GRACE);
            drive_paused.set(false);
            wake.notify_one();
            log::info!("pvp transparently reconnected the link");
        }

        // Teardown: retire latency so `latency()` reads `None` and the
        // telemetry panel retires, and wake the session to re-check
        // `is_ended` (the drive loop may already be gone, so no frame is
        // coming).
        link.retire_latency();
        drive_paused.set(false);
        wake.notify_one();
    });
}

/// Open the replay file + write its metadata frame, returning the writer
/// along with the path it records to (surfaced on the session so the
/// post-match results screen can offer playback). Everything the metadata
/// needs lives on `pre_match` (settings, seed, match clock, link code).
/// Name format mirrors the legacy app:
/// `YYYYMMDDhhmmss-<link_code>-<compat>-vs-<opponent>-p<idx>`.
fn build_replay_writer(
    store: &dyn ReplayStore,
    pre_match: &crate::pvp::PreMatchData,
    local_player_index: u8,
    local_save: &dyn tango_gamesupport::SaveData,
    remote_save: &dyn tango_gamesupport::SaveData,
) -> Result<(tango_replay::Writer, std::path::PathBuf), crate::Error> {
    let link_code = &pre_match.link_code;
    let local_settings = &pre_match.local_settings;
    let remote_settings = &pre_match.remote_settings;
    let local_gi = local_settings
        .game_info
        .as_ref()
        .ok_or(crate::Error::MissingGameInfo { side: "local" })?;
    let remote_gi = remote_settings
        .game_info
        .as_ref()
        .ok_or(crate::Error::MissingGameInfo { side: "remote" })?;
    let netplay_compat = local_gi
        .patch
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| local_gi.family_and_variant.0.clone());
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S");
    // Direct sessions have no link code in their metadata —
    // substitute a stable placeholder here so the filename
    // doesn't end up with a double-dash where the slot would be.
    let filename_link_code = if link_code.is_empty() { "direct" } else { link_code };
    let raw_name = format!(
        "{ts}-{filename_link_code}-{netplay_compat}-vs-{}-p{}",
        remote_settings.nickname,
        local_player_index + 1
    );
    let safe_name: String = raw_name.chars().filter(|c| !"/\\?%*:|\"<>. ".contains(*c)).collect();
    let Recording { sink, key } = store.create(&safe_name)?;
    let local_sram = local_save.to_sram_dump();
    let remote_sram = remote_save.to_sram_dump();
    let local_side = Some(tango_replay::metadata::Side {
        nickname: local_settings.nickname.clone(),
        game_info: Some(tango_replay::metadata::GameInfo {
            rom_family: local_gi.family_and_variant.0.clone(),
            rom_variant: local_gi.family_and_variant.1 as u32,
            patch: local_gi
                .patch
                .as_ref()
                .map(|p| tango_replay::metadata::game_info::Patch {
                    name: p.name.clone(),
                    version: p.version.to_string(),
                }),
        }),
        // The replay metadata proto (replay11) predates the
        // blind-setup inversion and still stores the
        // positive "reveal" sense.
        reveal_setup: !local_settings.blind_setup,
    });
    let remote_side = Some(tango_replay::metadata::Side {
        nickname: remote_settings.nickname.clone(),
        game_info: Some(tango_replay::metadata::GameInfo {
            rom_family: remote_gi.family_and_variant.0.clone(),
            rom_variant: remote_gi.family_and_variant.1 as u32,
            patch: remote_gi
                .patch
                .as_ref()
                .map(|p| tango_replay::metadata::game_info::Patch {
                    name: p.name.clone(),
                    version: p.version.to_string(),
                }),
        }),
        reveal_setup: !remote_settings.blind_setup,
    });
    // The recorder is where perspective enters the format: everything in
    // the file is absolute player order, so seat the two sides (and the
    // two saves) by our negotiated player index here and nowhere else.
    let (p1_side, p2_side, srams) = if local_player_index == 0 {
        (local_side, remote_side, [&local_sram, &remote_sram])
    } else {
        (remote_side, local_side, [&remote_sram, &local_sram])
    };
    let writer = tango_replay::Writer::new(
        // Buffered: write_input runs on the drive thread once per
        // confirmed tick, and unbuffered it costs a few small write
        // syscalls each time. The format already recovers truncated tails,
        // so a hard crash losing the buffered tail of an (already
        // incomplete) replay changes nothing; finish() flushes.
        std::io::BufWriter::new(sink),
        // SIO-engine stream: one continuous run of pair ticks.
        tango_replay::VERSION,
        local_player_index,
        tango_replay::Metadata {
            // The negotiated match clock, not the local wall clock: both
            // cores' cart RTC is pinned to this instant, and playback
            // re-primes pinned to `metadata.ts`, so recording the same
            // value is what makes playback reproduce the live match. Both
            // peers' replays of one match carry the identical ts.
            ts: pre_match.match_ts,
            link_code: link_code.clone(),
            p1_side,
            p2_side,
            match_type: pre_match.match_type.0 as u32,
            match_subtype: pre_match.match_type.1 as u32,
        },
        pre_match.rng_seed,
        [srams[0].as_slice(), srams[1].as_slice()],
    )?;
    Ok((writer, key))
}

/// A match waiting to be booted.
///
/// Handed back by [`PvpSession::new`] instead of being run inside it:
/// priming the pair is seconds of blocking emulation, and every other
/// session kind here leaves that decision — a thread, a blocking pool,
/// an event loop — to the host.
pub struct PvpBoot {
    pieces: BootPieces,
    drive: DriveContext,
    sample_rate: u32,
    local_player: usize,
    metrics: Arc<Metrics>,
}

impl PvpBoot {
    /// Boot and prime the pair. Blocks for seconds; run it somewhere
    /// that can afford to.
    ///
    /// Hands back the driver to tick and the session's audio stream —
    /// the local core resampled to the rate asked for at construction,
    /// following the loop's published fps target (see
    /// [`crate::audio::CoreStream`]).
    pub fn boot(self) -> Result<(PvpDriver, crate::audio::CoreStream), crate::Error> {
        let local_player = self.local_player;
        let metrics = self.metrics;
        let sample_rate = self.sample_rate;
        let (driver, pair) = self.drive.boot(self.pieces)?;
        let audio = crate::audio::CoreStream::new(
            crate::audio::PairCorePull {
                pair,
                player: Box::new(move || local_player),
            },
            move || f32::from_bits(metrics.fps_target.load(Ordering::Relaxed)),
            sample_rate,
        );
        Ok((driver, audio))
    }
}

/// A live match, one tick at a time.
///
/// The state below used to be locals of a `loop` on a thread of its
/// own; naming it is what lets a host drive the match instead — a
/// desktop thread paced by the wall clock today, an event loop once the
/// browser's signaling exists.
pub struct PvpDriver {
    ctx: DriveContext,
    match_: Match,
    throttler: tango_match::Throttler,
    /// (tick, [p0, p1]) confirmed input pairs not yet folded into
    /// stats (the telemetry for those ticks may confirm later).
    pending_buttons: std::collections::VecDeque<(u32, [u32; 2])>,
    /// Whether the first round's Started has been seen. Later Started
    /// events stamp round markers into the replay stream.
    first_round_started: bool,
    /// Confirmed ticks whose replay input record must carry a
    /// round-start marker.
    pending_round_marks: std::collections::VecDeque<u32>,
    fired_end_of_match: bool,
}

impl PvpDriver {
    /// Advance the match one tick. `false` once it's over — cancelled,
    /// a dead link, or a failed advance — after which the host calls
    /// [`PvpDriver::finish`].
    pub fn tick(&mut self) -> bool {
        if self.ctx.cancel.is_cancelled() {
            return false;
        }
        if self.ctx.drive_paused.paused() {
            // The reconnect supervisor releases the gate on every
            // exit path. Nothing to advance meanwhile, so the tick
            // goes back to the host — which is also what lets a
            // browser pause without a thread to park.
            return true;
        }

        // The ready gate. Reaching `tick` means our own pair is primed;
        // hold here until the peer says the same, so both sides start
        // within a round trip of each other.
        //
        // Advancing alone is not merely early — `advance` buffers a
        // local input per tick that the absent peer cannot match, and at
        // `RECONNECT_QUEUE_LENGTH` (~3 s) the supervisor's stall watch
        // reads that as a dead link and tears the transport down for a
        // reconnect the peer never needed. Idling here keeps the
        // published `queue_len` at zero, so the watch stays quiet however
        // long the slower machine takes; the in-match heartbeat runs off
        // its own task and keeps the link alive meanwhile.
        //
        // The wait is bounded without a timer: a peer that never primes
        // fails its own boot (`MAX_PRIME_TICKS`) and tears down, which
        // reaches us as the control channel's `Goodbye` or EOF.
        if !self.ctx.peer_primed.load(Ordering::Acquire) {
            return true;
        }

        // Live present-delay adjustment from the footer slider.
        let pd = self.ctx.frame_delay.load(Ordering::Relaxed);
        if pd != self.match_.present_delay() {
            self.match_.set_present_delay(pd);
        }

        // Drain the network before advancing: every confirmed tick we
        // ingest now is a rollback we don't take deeper.
        for input in self.ctx.event_rx.try_iter() {
            self.match_
                .add_remote_input(input.joyflags as u32, input.tick_advantage);
        }

        // Stall guard: the peer is too far behind (or gone) — advancing
        // further would run the input stream past the rennet horizon.
        // The in-match heartbeat keeps the redundancy window + acks
        // flowing while we wait; the supervisor watches queue_len and
        // decides whether this is a reconnect.
        //
        // But hold ONLY when advancing can't make progress. `advance` is
        // the sole thing that drains the local queue (it matches buffered
        // remote inputs against local ones and confirms the pairs); merely
        // ingesting remote inputs above just buffers them. So if the peer
        // is still feeding us matchable inputs — exactly the case during a
        // post-reconnect resend burst — we must keep advancing to settle
        // them, even at a full queue: each such advance nets the queue
        // *down* (drains ≥1, adds 1 local). Skipping advance whenever the
        // queue is full instead would leave those resends forever
        // unconsumed — the queue never drains, the stall never clears, and
        // the link "reconnects but never resumes". Only a genuinely dead
        // link (nothing matchable) parks here.
        let queue_len = self.match_.local_queue_length() as u32;
        self.ctx.metrics.queue_len.store(queue_len, Ordering::Relaxed);
        if queue_len as usize >= crate::net::data::RECONNECT_QUEUE_LENGTH && self.match_.matchable() == 0 {
            // Block on the event queue rather than poll it: the only
            // thing that clears a stall is the peer's next input, and
            // it arrives exactly here. Ingest it and loop — the drain
            // + stall re-check above decide whether we're unstuck.
            // The only thing that clears a stall is the peer's
            // next input. Take whatever has landed and give the
            // tick back; the drain + stall re-check above decide
            // next time whether we're unstuck. A dead channel
            // yields too — the supervisor owns what happens next,
            // and spinning on it would burn the host's loop.
            if let Ok(input) = self.ctx.event_rx.try_recv() {
                self.match_
                    .add_remote_input(input.joyflags as u32, input.tick_advantage);
            }
            return true;
        }

        // Sample the skew before `advance` enqueues this tick's local
        // input, so our half matches the advantage we ship the peer.
        let skew = self.match_.skew();
        self.ctx.metrics.skew.store(skew, Ordering::Relaxed);

        let keys = self.ctx.joyflags.load(Ordering::Relaxed) & tango_match::input::JOYFLAGS_MASK as u32;
        let (outgoing, report) = match self.match_.advance(keys) {
            Ok(r) => r,
            Err(e) => {
                log::error!("pvp: sio advance failed: {e}");
                self.ctx.cancel.cancel();
                return false;
            }
        };
        self.ctx.metrics.depth.store(report.rolled_back, Ordering::Relaxed);

        // Ship this tick's local input. Push-before-send semantics live
        // in the pump; a transport error is non-terminal (the heartbeat
        // retransmits once the reconnect swaps a live channel back in).
        if self
            .ctx
            .sender
            .send(&crate::net::data::Input {
                joyflags: outgoing.keys as u16,
                tick_advantage: outgoing.tick_advantage,
            })
            .is_err()
        {
            log::warn!("pvp: send pump terminated; ending match");
            self.ctx.end.remote_disconnected.store(true, Ordering::Release);
            self.ctx.cancel.cancel();
            return false;
        }

        // Confirmed telemetry, drained before the replay write so this
        // batch's round-start events can stamp markers onto this
        // batch's input records. Everything at or below the confirmed
        // boundary is final — no revocation bookkeeping needed on this
        // side of the engine.
        let (samples, events) = self
            .match_
            .telemetry()
            .lock()
            .unwrap()
            .drain_confirmed(report.confirmed);

        // Round lifecycle, trap-driven off the games' own code paths:
        // a round start (after the first) stamps a marker into the
        // replay; the match-end anchor firing means the players left
        // the battle loop for good — the direct successor of the trap
        // engine's match-end hook.
        let mut match_ended = false;
        for (tick, event) in &events {
            match event {
                telemetry::RoundEvent::Started => {
                    if self.first_round_started {
                        self.pending_round_marks.push_back(*tick);
                    }
                    self.first_round_started = true;
                }
                telemetry::RoundEvent::Ended { .. } => {}
                telemetry::RoundEvent::MatchEnded => {
                    match_ended = true;
                }
            }
        }

        // Confirmed inputs: replay sink + the buttons half of the
        // stats merge below.
        let confirmed_inputs = self.match_.drain_confirmed();
        if let Some(w) = self.ctx.replay_writer.as_mut() {
            for (tick, keys) in &confirmed_inputs {
                if self.pending_round_marks.front().is_some_and(|t| t <= tick) {
                    self.pending_round_marks.pop_front();
                    let _ = w.start_round();
                }
                if let Err(e) = w.write_input([keys[0] as u16, keys[1] as u16]) {
                    log::warn!("pvp: replay write failed (recording stops): {e}");
                    self.ctx.replay_writer = None;
                    break;
                }
            }
        }
        self.pending_buttons.extend(confirmed_inputs);

        if !samples.is_empty() || !events.is_empty() {
            self.ctx
                .fold_confirmed_telemetry(samples, events, &mut self.pending_buttons);
        }

        // Completion: the games' own match-end path ran (confirmed —
        // both peers see it at the same pair tick). No runout needed:
        // the anchor fires after the result screens have played.
        if match_ended {
            self.ctx.completed.store(true, Ordering::Release);
        }
        if self.ctx.completed.load(Ordering::Acquire) && !self.fired_end_of_match {
            self.fired_end_of_match = true;
            let first_completion = {
                let mut completed_at = self.ctx.end.local_ended_at.lock().unwrap();
                if completed_at.is_some() {
                    false
                } else {
                    *completed_at = Some(web_time::Instant::now());
                    true
                }
            };
            if first_completion {
                // In-band EndOfMatch: rides the same ordered seq stream
                // as inputs, so the peer sees it exactly once and only
                // after every preceding input.
                let in_match = self.ctx.in_match.clone();
                crate::platform::spawn(async move {
                    if let Err(e) = in_match.send_end_of_match().await {
                        log::warn!("pvp: send EndOfMatch failed: {e}");
                    }
                });
                // Wall-clock fallback wake so `is_ended` is rechecked
                // even if the peer never sends EndOfMatch.
                let wake = self.ctx.wake.clone();
                crate::platform::spawn(async move {
                    crate::platform::sleep(PEER_END_GRACE).await;
                    wake.notify_one();
                });
            }
        }

        // Present the local screen to the UI. (Audio needs no push —
        // the output stream pulls it straight off the pair.)
        if let Some(buf) = self.match_.local_video_buffer() {
            self.ctx.screen.write(&buf);
        }
        self.ctx.tps_counter.lock().unwrap().mark();
        self.ctx.wake.notify_one();

        // Clock sync: only the leading peer shaves tick rate, and only
        // once the presented frame actually speculates past the present
        // delay.
        let slowdown = self.throttler.step(skew, self.match_.speculation_balance());
        let target = EXPECTED_FPS - slowdown;
        self.ctx.metrics.fps_target.store(target.to_bits(), Ordering::Relaxed);

        true
    }

    /// Flush the replay tail and cache the match's stats. Called once,
    /// after [`PvpDriver::tick`] reports the match over — reached through
    /// [`crate::Drive::finish`], which is why a host must wind a driver
    /// down rather than drop it.
    pub fn finish(mut self) {
        // Teardown: flush the replay tail. Finalize (write the EOR
        // sentinel) only if the match completed — same policy as the trap
        // engine, so an aborted match leaves a truncated-but-parseable
        // recording.
        if let Some(mut w) = self.ctx.replay_writer.take() {
            for (_, keys) in self.match_.drain_confirmed() {
                let _ = w.write_input([keys[0] as u16, keys[1] as u16]);
            }
            if self.ctx.completed.load(Ordering::Acquire) {
                if let Err(e) = w.finish() {
                    log::error!("finish replay failed: {e}");
                }
                // Cache the finished match's stats — each round already
                // folded as it ended, so the Replays tab never has to
                // re-simulate this one.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(stats_path) = self.ctx.stats_path.as_ref() {
                    let snapshot = self.ctx.stats.lock().unwrap().snapshot();
                    if let Err(e) = crate::stats::write_match_stats(stats_path, &snapshot) {
                        log::warn!("failed to write replay stats cache entry: {e}");
                    }
                }
            }
        }
    }
}

impl crate::Drive for PvpDriver {
    fn tick(&mut self) -> bool {
        PvpDriver::tick(self)
    }

    fn finish(self) {
        PvpDriver::finish(self)
    }

    /// The throttler's target: the base rate minus whatever it shaved
    /// to bring the peers' clocks together.
    fn fps_target(&self) -> f32 {
        f32::from_bits(self.ctx.metrics.fps_target.load(Ordering::Relaxed))
    }
}

/// Rolling-window tick counter behind the status bar's TPS readout.
/// Marked once per emulated frame, so the reading follows the match
/// rather than the host's refresh rate.
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

pub struct TpsCounter {
    marks: VecDeque<Instant>,
    window_size: usize,
}

impl TpsCounter {
    pub fn new(window_size: usize) -> Self {
        Self {
            marks: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    pub fn mark(&mut self) {
        while self.marks.len() >= self.window_size {
            self.marks.pop_front();
        }
        self.marks.push_back(Instant::now());
    }

    /// Average interval between consecutive marks. ZERO if the
    /// counter has fewer than two marks.
    pub fn mean_duration(&self) -> Duration {
        if self.marks.len() < 2 {
            return Duration::ZERO;
        }
        let mut total = Duration::ZERO;
        let mut count = 0u32;
        for (a, b) in self.marks.iter().zip(self.marks.iter().skip(1)) {
            total += *b - *a;
            count += 1;
        }
        total / count
    }
}

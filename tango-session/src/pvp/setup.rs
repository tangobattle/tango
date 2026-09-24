//! Assemble a session's shared state, transport, drivers, and recording.

use super::driver::{BootPieces, DriveContext};
use super::recording::build_replay_writer;
use super::supervisor::{spawn_primed_announcer, spawn_supervisor, SupervisorContext};
use super::{EndState, Metrics, PvpBoot, PvpSession, PvpSessionArgs, Seat, TpsCounter};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tango_net_protocol::derive::pick_local_player_index;

/// Retransmit-heartbeat cadence for the in-match channel — one nominal
/// 60 Hz frame. A wire cadence, not a sim rate — the engines' real
/// rates differ from it by well under a percent, which loss recovery
/// doesn't care about. Keeps the unacked redundancy window flowing
/// while the local sim is throttled or stalled, so recovery isn't
/// coupled to the frame rate (see [`tango_net::InMatchTx`]).
const IN_MATCH_HEARTBEAT: std::time::Duration = std::time::Duration::from_nanos((1_000_000_000.0 / 60.0) as u64);

impl PvpSession {
    /// Build the live match from [`PvpSessionArgs`].
    ///
    /// Async because the lobby loop holds the data-channel `Receiver`
    /// until it observes its cancellation and exits (`Link::bring_up`
    /// awaits its handback). Everything slow happens after this
    /// returns: the session is live as soon as the exchange is, and the
    /// pair is booted and primed on the drive loop underneath it (see
    /// [`PvpBoot`]).
    ///
    /// Hands back the session, the thing the host drives, and the
    /// session's audio stream — the local core's samples at the args'
    /// `sample_rate`, rate control following the drive loop's published
    /// fps target, silent until the boot lands.
    pub async fn new(args: PvpSessionArgs<'_>) -> Result<(Self, PvpBoot, crate::audio::Stream), crate::Error> {
        let PvpSessionArgs {
            local,
            remote,
            pre_match,
            frame_delay,
            disable_bgm,
            replays,
            stats_sink,
            sample_rate,
        } = args;
        let Seat {
            game: local_game,
            rom: local_rom,
            sram: local_sram,
        } = local;
        let Seat {
            game: remote_game,
            rom: remote_rom,
            sram: remote_sram,
        } = remote;
        let expected_fps = crate::local::native_fps(local_game);
        let cancellation_token = tokio_util::sync::CancellationToken::new();

        // The engine gets a head start on the pair the boot will run —
        // the awaits below (handoff, transport bring-up) are where a
        // browser's worker threads get the event-loop turns their
        // startup needs.
        local_game.pvp.prepare(2);

        // Player index off the shared RNG seed, same negotiation as ever:
        // both peers derive the same assignment, mirrored.
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(pre_match.terms.rng_seed);
        let local_player_index = pick_local_player_index(&mut rng, pre_match.terms.is_offerer);

        // The match clock, pinned into both carts' RTC and recorded in the
        // replay metadata so playback re-primes to the identical state.
        let rtc_time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(pre_match.terms.match_ts);

        // Replay writer. Failing to open it shouldn't kill the
        // match — log and continue without recording.
        let (replay_writer, replay_path) = match replays {
            None => (None, None),
            Some(store) => {
                match build_replay_writer(
                    store,
                    &pre_match,
                    local_game,
                    remote_game,
                    local_player_index,
                    &local_sram,
                    &remote_sram,
                ) {
                    Ok((writer, path)) => (Some(writer), Some(path)),
                    Err(e) => {
                        log::warn!("pvp: replay writer open failed: {e}");
                        (None, None)
                    }
                }
            }
        };

        // Assemble the peer link from the lobby handoff — this awaits the
        // lobby loop releasing the reliable receiver (typically a few ms after
        // take_pre_match flipped the cancel) and starts the in-match
        // retransmit heartbeat.
        let link = Arc::new(
            tango_net::link::Link::bring_up(pre_match.link_parts, IN_MATCH_HEARTBEAT, cancellation_token.clone())
                .await?,
        );
        let in_match = link.in_match().clone();

        let end = EndState::default();
        let local_input = crate::InputCell::new();
        let completed = Arc::new(AtomicBool::new(false));
        // All screens until the host says which it is arranging to show.
        let displayed_screens = Arc::new(std::sync::atomic::AtomicU8::new(u8::MAX));
        let frame_delay = Arc::new(AtomicU32::new(frame_delay));
        let metrics = Arc::new(Metrics::default());
        // Seeded rather than left at zero: the host paces the boot
        // itself off this, and the audio stream's rate control reads it
        // from the moment the host binds the stream — both of which
        // start before the drive loop first publishes a target.
        metrics.fps_target.store(expected_fps.to_bits(), Ordering::Relaxed);
        let drive_paused = Arc::new(crate::PauseGate::new(false));
        // ~1 s window at 60 Hz.
        let tps_counter = Arc::new(Mutex::new(TpsCounter::new(60)));
        let layout = local_game.pvp.screen_layout(tango_match::SessionMode::PvP {
            match_type: pre_match.terms.match_type,
        });
        let screen = crate::Framebuffer::new(&layout);
        let wake = Arc::new(tokio::sync::Notify::new());
        // The two-sided ready gate. Priming takes as long as the machine
        // running it takes, so each peer announces when its own pair
        // reaches the link battle and neither ticks until both have.
        let local_primed = Arc::new(AtomicBool::new(false));
        let boot_cancel = Arc::new(AtomicBool::new(false));
        let peer_primed = link.peer_primed();
        let announce_primed = Arc::new(tokio::sync::Notify::new());

        // A game whose engine reports no chip events folds the rest of
        // the stats without them — the aggregator is the same for all.
        let stats = Arc::new(Mutex::new(tango_match::analysis::StatsBuilder::new()));

        // Remote input events flow receive-task → drive thread over this
        // queue; the rennet reassembly in PvpReceiver already ordered and
        // deduplicated them (one Input per remote tick, in tick order).
        let (event_tx, event_rx) = std::sync::mpsc::channel::<tango_net::data::Input>();

        // The sender pump: the drive thread pushes one Input per advance;
        // the pump ships each as a rennet frame over the unreliable channel.
        let sender = tango_net::PvpSender::new(in_match.clone());

        // Pair-order arrays: core 0 always runs player 0's game, on both
        // peers, so priming and simulation are bit-identical across the pair.
        let (roms, saves, supports) = if local_player_index == 0 {
            (
                [local_rom.as_ref().clone(), remote_rom.as_ref().clone()],
                [local_sram, remote_sram],
                [local_game.pvp, remote_game.pvp],
            )
        } else {
            (
                [remote_rom.as_ref().clone(), local_rom.as_ref().clone()],
                [remote_sram, local_sram],
                [remote_game.pvp, local_game.pvp],
            )
        };

        // Everything the match needs to boot, handed back for the host
        // to drive: the pair is single-threaded by design and priming it
        // is seconds of emulation, so *where* that happens is the host's
        // call — a drive thread on a desktop, the event loop in a
        // browser. Either way the session is already on screen for it.
        let pieces = BootPieces {
            roms,
            saves,
            pvp: supports,
            peer_rom: tango_match::PeerRom {
                code: *remote_game.rom_code,
                revision: remote_game.revision,
            },
            match_type: pre_match.terms.match_type,
            rng_seed: pre_match.terms.rng_seed,
            rtc: rtc_time,
            local_player: local_player_index as usize,
            present_delay: frame_delay.load(Ordering::Relaxed),
            disable_bgm,
        };
        let drive = DriveContext {
            local_input: local_input.clone(),
            frame_delay: frame_delay.clone(),
            metrics: metrics.clone(),
            drive_paused: drive_paused.clone(),
            cancel: cancellation_token.clone(),
            completed: completed.clone(),
            displayed_screens: displayed_screens.clone(),
            end: end.clone(),
            event_rx,
            sender,
            in_match: in_match.clone(),
            replay_writer,
            stats: stats.clone(),
            stats_key: replay_path.clone(),
            stats_sink,
            tps_counter: tps_counter.clone(),
            screen: screen.clone(),
            wake: wake.clone(),
            local_primed: local_primed.clone(),
            peer_primed: peer_primed.clone(),
            announce_primed: announce_primed.clone(),
            local_player: local_player_index as usize,
            boot_cancel: boot_cancel.clone(),
        };

        // The session's audio ring, made before the pair that feeds it
        // exists: the host binds this stream at construction, and the
        // ring simply reads empty — so the stream primes — through the
        // seconds of priming walk the boot below runs.
        let (audio_in, audio_out) = crate::audio::ring();
        let audio = crate::audio::Stream::new(
            audio_out,
            expected_fps,
            {
                let metrics = metrics.clone();
                move || f32::from_bits(metrics.fps_target.load(Ordering::Relaxed))
            },
            sample_rate,
        );
        let prime_error = Arc::new(Mutex::new(None));
        let boot = PvpBoot {
            pending: Some((pieces, drive)),
            driver: None,
            audio: Some(audio_in),
            prime_error: prime_error.clone(),
            expected_fps,
            metrics: metrics.clone(),
            wake: wake.clone(),
            backend: local_game.pvp,
        };

        // Announce our own prime as soon as the boot finishes. It rides
        // the reliable control channel, so an accepted send is a
        // delivered one; a rejected send means that channel is gone and
        // ends the match. (The supervisor re-announces after a
        // reconnect, whose rebuild drops anything undelivered.)
        spawn_primed_announcer(
            link.clone(),
            local_primed.clone(),
            announce_primed,
            end.clone(),
            cancellation_token.clone(),
        );

        // Receive pump + link supervisor: reads peer frames into the event
        // queue, watches for stalls, and runs the transparent reconnect.
        let supervisor_done = tokio_util::sync::CancellationToken::new();
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
            local_primed: local_primed.clone(),
            done: supervisor_done.clone(),
        });

        let session = Self {
            local_game,
            local_player_index,
            local_input,
            completed,
            displayed_screens: displayed_screens.clone(),
            end,
            tps_counter,
            cancellation_token,
            supervisor_done,
            link,
            local_primed,
            peer_primed,
            prime_error,
            boot_cancel,
            metrics,
            link_code: pre_match.terms.link_code,
            remote_nickname: pre_match.terms.remote_settings.nickname,
            frame_delay,
            stats,
            replay_path,
            layout,
            screen,
            wake,
            started_at: web_time::Instant::now(),
        };
        Ok((session, boot, audio))
    }
}

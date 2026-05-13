use crate::{audio, config, game, net, rom, stats, video};
use parking_lot::Mutex;
use rand::SeedableRng;
use std::sync::Arc;

pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

pub struct GameInfo {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub patch: Option<(String, semver::Version)>,
}

pub struct Setup {
    pub game_lang: unic_langid::LanguageIdentifier,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    pub assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
}

pub struct Session {
    start_time: std::time::SystemTime,
    game_info: GameInfo,
    vbuf: std::sync::Arc<Mutex<Vec<u8>>>,
    _audio_binding: audio::Binding,
    thread: mgba::thread::Thread,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    mode: Mode,
    completion_token: tango_pvp::hooks::CompletionToken,
    pause_on_next_frame: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Set by [`Session::request_close`]. The GUI's session-management loop
    /// drops the session when this becomes true. Replay sessions ignore the
    /// completion_token for auto-close (so the user can scrub past the end)
    /// and rely on this flag instead.
    close_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    opponent_setup: Option<Setup>,
    own_setup: Option<Setup>,
}

pub struct PvP {
    pub match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<tango_pvp::battle::Match>>>>,
    cancellation_token: tokio_util::sync::CancellationToken,
    latency_counter: std::sync::Arc<tokio::sync::Mutex<crate::stats::LatencyCounter>>,
    _peer_conn: datachannel_wrapper::PeerConnection,
}

impl PvP {
    pub async fn latency(&self) -> std::time::Duration {
        self.latency_counter.lock().await.median()
    }
}

pub struct SinglePlayer {}

/// mgba snapshot captured during replay playback, used by backward seek to
/// skip the boot/menu sequence (and any rounds + frames before this point).
/// Captured both at round-start (`has_committed_this_round = false`,
/// `current_tick_in_round = 0`) and periodically mid-round.
#[derive(Clone)]
struct ReplaySnapshot {
    round_index: u32,
    /// Position used to pick the best snapshot for a seek target.
    absolute_tick: u32,
    current_tick_in_round: u32,
    has_committed_this_round: bool,
    rng_state: rand_pcg::Mcg128Xsl64,
    /// Captured local_packet (target_tick, packet bytes) at snapshot time;
    /// fed back into the stepper on restore so games whose frame layout
    /// shifts current_tick relative to send_and_receive (e.g. BN3) get the
    /// exact same packet they had in normal playback.
    local_packet: Option<(u32, Vec<u8>)>,
    /// Inputs consumed from the current round at snapshot time. Restore
    /// drops this many entries from the front of the round so the input
    /// queue picks up where it left off (current_tick_in_round isn't a
    /// reliable proxy when sends per tick != 1).
    inputs_consumed: u32,
    mgba_state: Arc<mgba::state::State>,
}

/// Take a fresh snapshot every this many absolute_ticks within an active
/// round, in addition to the round-start snapshot. Coarser snapshots leave
/// long fast-forwards on backward scrub; finer snapshots cost RAM and a
/// `core.save_state()` per capture (~256KB each). 240 ticks ≈ 4 seconds of
/// GBA time.
const MID_ROUND_SNAPSHOT_INTERVAL: u32 = 240;

pub struct Replayer {
    /// Held so a seek can rebuild the stepper from
    /// [`replay::Replay::rounds`] starting at any round index.
    replay: Arc<tango_pvp::replay::Replay>,
    /// Stepper state shared with the per-game replay traps. Used by the
    /// Session API to read out absolute_tick / total_replay_ticks, and
    /// swapped wholesale by [`Session::replay_seek_to`] on snapshot load.
    stepper_state: tango_pvp::stepper::State,
    /// mgba state + stepper checkpoints captured during playback (round
    /// starts and periodic mid-round) and during sync seeks. Both seek
    /// directions pick the snapshot closest to target to minimize how
    /// many frames we have to re-run.
    snapshots: Arc<parking_lot::Mutex<Vec<ReplaySnapshot>>>,
    /// Furthest absolute_tick the background prefetch worker has reached.
    /// The seek-bar UI clamps user drags to this value so the user can
    /// only seek into prefetched (= snapshotted) territory.
    prefetch_progress: Arc<std::sync::atomic::AtomicU32>,
    /// Holds the prefetch worker thread alive. Drop signals cancel + joins.
    _prefetcher: Prefetcher,
}

impl Replayer {
    /// Returns the snapshot whose `absolute_tick` is the largest value still
    /// `<= target`, if any.
    fn best_snapshot_for(&self, target: u32) -> Option<ReplaySnapshot> {
        self.snapshots
            .lock()
            .iter()
            .filter(|s| s.absolute_tick <= target)
            .max_by_key(|s| s.absolute_tick)
            .cloned()
    }
}

/// Background worker that runs a second mgba core forward as fast as the
/// host CPU allows, capturing snapshots into the shared snapshots Vec. The
/// playback thread reuses these snapshots when the user seeks, so forward
/// seeks within the prefetched range land instantly.
struct Prefetcher {
    cancel: Arc<std::sync::atomic::AtomicBool>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl Prefetcher {
    fn spawn(
        rom: Arc<Vec<u8>>,
        replay: Arc<tango_pvp::replay::Replay>,
        game: &'static (dyn game::Game + Send + Sync),
        snapshots: Arc<parking_lot::Mutex<Vec<ReplaySnapshot>>>,
        progress: Arc<std::sync::atomic::AtomicU32>,
    ) -> Self {
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_for_thread = cancel.clone();
        let join_handle = std::thread::spawn(move || {
            if let Err(e) = run_prefetch(rom, replay, game, snapshots, cancel_for_thread, progress) {
                log::error!("replay prefetch worker exited with error: {:?}", e);
            }
        });
        Prefetcher {
            cancel,
            join_handle: Some(join_handle),
        }
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

fn run_prefetch(
    rom: Arc<Vec<u8>>,
    replay: Arc<tango_pvp::replay::Replay>,
    game: &'static (dyn game::Game + Send + Sync),
    snapshots: Arc<parking_lot::Mutex<Vec<ReplaySnapshot>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    progress: Arc<std::sync::atomic::AtomicU32>,
) -> anyhow::Result<()> {
    let mut core = mgba::core::Core::new_gba("tango-prefetch")?;
    core.enable_video_buffer();
    core.as_mut()
        .load_rom(mgba::vfile::VFile::from_vec(rom.as_ref().clone()))?;
    core.as_mut()
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram.clone()))?;
    // mgba::thread::Thread::start does this implicitly for the playback
    // core; a raw Core driven by run_frame needs it explicitly.
    core.as_mut().reset();

    let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(game.gamedb_entry()).unwrap();
    hooks.patch(core.as_mut());

    let total_replay_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();
    let stepper_state = tango_pvp::stepper::State::new(
        (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
        replay.local_player_index,
        replay.rounds.clone(),
        0,
        replay.rng_seed,
        replay.is_offerer,
        total_replay_ticks,
        Box::new(|| {}),
    );
    let mut traps = hooks.common_traps();
    traps.extend(hooks.stepper_traps(stepper_state.clone()));
    core.set_traps(traps);

    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }

        let (total_left, absolute_tick) = {
            let inner = stepper_state.lock_inner();
            (inner.total_input_pairs_left(), inner.absolute_tick())
        };
        if total_left == 0 && absolute_tick > 0 {
            // Prefetcher reached end-of-replay. Mark the bar fully buffered
            // and exit — the playback thread will pick up from existing
            // snapshots from here on out.
            progress.store(total_replay_ticks, std::sync::atomic::Ordering::Relaxed);
            return Ok(());
        }

        if let Some(cp) = stepper_state.capture_replay_checkpoint() {
            let mut snaps = snapshots.lock();
            let want_round_start = !cp.has_committed_this_round
                && !snaps
                    .iter()
                    .any(|s| s.round_index == cp.current_round_index && !s.has_committed_this_round);
            let lo = cp.absolute_tick.saturating_sub(MID_ROUND_SNAPSHOT_INTERVAL);
            let want_mid_round = cp.has_committed_this_round
                && !snaps
                    .iter()
                    .any(|s| s.absolute_tick > lo && s.absolute_tick <= cp.absolute_tick);
            if want_round_start || want_mid_round {
                if let Ok(state) = core.as_mut().save_state() {
                    snaps.push(ReplaySnapshot {
                        round_index: cp.current_round_index,
                        absolute_tick: cp.absolute_tick,
                        current_tick_in_round: cp.current_tick_in_round,
                        has_committed_this_round: cp.has_committed_this_round,
                        rng_state: cp.rng_state,
                        local_packet: cp.local_packet,
                        inputs_consumed: cp.inputs_consumed,
                        mgba_state: Arc::new(*state),
                    });
                }
            }
        }

        progress.store(absolute_tick, std::sync::atomic::Ordering::Relaxed);
        core.as_mut().run_frame();
    }
}

pub enum Mode {
    SinglePlayer(SinglePlayer),
    PvP(PvP),
    Replayer(Replayer),
}

struct BuildReplayerArgs {
    audio_binder: audio::LateBinder,
    game: &'static (dyn game::Game + Send + Sync),
    patch: Option<(String, semver::Version)>,
    rom: Arc<Vec<u8>>,
    emu_tps_counter: Arc<Mutex<stats::Counter>>,
    replay: Arc<tango_pvp::replay::Replay>,
}

impl Session {
    pub fn new_pvp(
        config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
        audio_binder: audio::LateBinder,
        link_code: String,
        netplay_compatibility: String,
        local_settings: net::protocol::Settings,
        local_game: &'static (dyn game::Game + Send + Sync),
        local_patch: Option<(String, semver::Version)>,
        local_patch_overrides: &rom::Overrides,
        local_rom: &[u8],
        local_save: Box<dyn tango_dataview::save::Save + Send + Sync + 'static>,
        remote_settings: net::protocol::Settings,
        remote_game: &'static (dyn game::Game + Send + Sync),
        remote_patch_overrides: &rom::Overrides,
        remote_rom: &[u8],
        remote_save: Box<dyn tango_dataview::save::Save + Send + Sync + 'static>,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        sender: net::Sender,
        receiver: net::Receiver,
        peer_conn: datachannel_wrapper::PeerConnection,
        is_offerer: bool,
        replays_path: std::path::PathBuf,
        match_type: (u8, u8),
        rng_seed: [u8; 16],
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(local_rom.to_vec()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(local_save.as_sram_dump()))?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let local_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(local_game.gamedb_entry()).unwrap();
        local_hooks.patch(core.as_mut());

        let match_ = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let _ = std::fs::create_dir_all(replays_path.parent().unwrap());
        let mut traps = local_hooks.common_traps();

        let completion_token = tango_pvp::hooks::CompletionToken::new();

        traps.extend(local_hooks.primary_traps(joyflags.clone(), match_.clone(), completion_token.clone()));
        core.set_traps(
            traps
                .into_iter()
                .map(|(addr, f)| {
                    let handle = tokio::runtime::Handle::current();
                    (
                        addr,
                        Box::new(move |core: mgba::core::CoreMutRef<'_>| {
                            let _guard = handle.enter();
                            f(core)
                        }) as Box<dyn Fn(mgba::core::CoreMutRef<'_>)>,
                    )
                })
                .collect(),
        );

        let reveal_setup = remote_settings.reveal_setup;

        let thread = mgba::thread::Thread::new(core);

        let sender = std::sync::Arc::new(tokio::sync::Mutex::new(sender));
        let latency_counter = std::sync::Arc::new(tokio::sync::Mutex::new(crate::stats::LatencyCounter::new(5)));

        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let match_ = match_.clone();
        *match_.try_lock().unwrap() = Some({
            let config = config.read();
            let replays_path = config.replays_path();
            let link_code = link_code.clone();
            let netplay_compatibility = netplay_compatibility.clone();
            let local_settings = local_settings.clone();
            let remote_settings = remote_settings.clone();

            let mut rng = rand_pcg::Mcg128Xsl64::from_seed(rng_seed);
            let local_player_index = tango_pvp::battle::Match::pick_local_player_index(&mut rng, is_offerer);

            let local_sram = local_save.as_sram_dump();
            let remote_sram = remote_save.as_sram_dump();

            const TIME_DESCRIPTION: &[time::format_description::FormatItem<'_>] = time::macros::format_description!(
                "[year padding:zero][month padding:zero repr:numerical][day padding:zero][hour padding:zero][minute padding:zero][second padding:zero]"
            );
            let replay_filename = replays_path.join(format!(
                "{}.tangoreplay",
                format!(
                    "{}-{}-{}-vs-{}-p{}",
                    time::OffsetDateTime::from(std::time::SystemTime::now())
                        .format(TIME_DESCRIPTION)
                        .expect("format time"),
                    link_code,
                    netplay_compatibility,
                    remote_settings.nickname,
                    local_player_index + 1
                )
                .chars()
                .filter(|c| "/\\?%*:|\"<>. ".chars().all(|c2| c2 != *c))
                .collect::<String>()
            ));

            let replay_writer = {
                log::info!("open replay: {}", replay_filename.display());

                let local_game_settings = local_settings.game_info.as_ref().unwrap();
                let remote_game_settings = remote_settings.game_info.as_ref().unwrap();

                let replay_file = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&replay_filename)?;
                Some(tango_pvp::replay::Writer::new(
                    replay_file,
                    tango_pvp::replay::Metadata {
                        ts: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64,
                        link_code: link_code.clone(),
                        local_side: Some(tango_pvp::replay::metadata::Side {
                            nickname: local_settings.nickname.clone(),
                            game_info: Some(tango_pvp::replay::metadata::GameInfo {
                                rom_family: local_game_settings.family_and_variant.0.to_string(),
                                rom_variant: local_game_settings.family_and_variant.1 as u32,
                                patch: local_game_settings.patch.as_ref().map(|patch| {
                                    tango_pvp::replay::metadata::game_info::Patch {
                                        name: patch.name.clone(),
                                        version: patch.version.to_string(),
                                    }
                                }),
                            }),
                            reveal_setup: local_settings.reveal_setup,
                        }),
                        remote_side: Some(tango_pvp::replay::metadata::Side {
                            nickname: remote_settings.nickname.clone(),
                            game_info: Some(tango_pvp::replay::metadata::GameInfo {
                                rom_family: remote_game_settings.family_and_variant.0.to_string(),
                                rom_variant: remote_game_settings.family_and_variant.1 as u32,
                                patch: remote_game_settings.patch.as_ref().map(|patch| {
                                    tango_pvp::replay::metadata::game_info::Patch {
                                        name: patch.name.clone(),
                                        version: patch.version.to_string(),
                                    }
                                }),
                            }),
                            reveal_setup: remote_settings.reveal_setup,
                        }),
                        round: 0,
                        match_type: match_type.0 as u32,
                        match_subtype: match_type.1 as u32,
                    },
                    is_offerer,
                    local_player_index,
                    local_hooks.packet_size() as u8,
                    rng_seed,
                    &local_sram,
                    &remote_sram,
                )?)
            };

            let remote_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(remote_game.gamedb_entry()).unwrap();
            let identity = tango_pvp::battle::MatchIdentity {
                match_type,
                is_offerer,
                local_player_index,
                input_delay: config.input_delay,
            };
            let shadow = tango_pvp::shadow::Shadow::new(
                remote_rom,
                remote_save.as_ref(),
                remote_hooks,
                match_type,
                is_offerer,
                local_player_index,
                rng.clone(),
            )?;
            let inner_match = tango_pvp::battle::Match::new(
                local_rom.to_vec(),
                local_hooks,
                thread.handle(),
                Box::new(crate::net::PvpSender::new(sender.clone())),
                cancellation_token.clone(),
                rng,
                shadow,
                identity,
                tango_pvp::battle::ReplayConfig { writer: replay_writer },
            );

            {
                let match_ = match_.clone();
                let inner_match = inner_match.clone();
                let receiver = Box::new(crate::net::PvpReceiver::new(
                    receiver,
                    sender.clone(),
                    latency_counter.clone(),
                ));
                tokio::task::spawn(async move {
                    tokio::select! {
                        r = inner_match.run(receiver) => {
                            log::info!("match thread ending: {:?}", r);
                        }
                        _ = inner_match.cancelled() => {
                        }
                    }
                    log::info!("match thread ended");
                    if let Err(e) = inner_match.finish_replay() {
                        log::error!("finish replay failed: {}", e);
                    }
                    *match_.lock().await = None;
                });
            }

            inner_match
        });

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        ))))?;

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        thread.set_frame_callback({
            let completion_token = completion_token.clone();
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut vbuf);
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();

                if completion_token.is_complete() {
                    thread_handle.pause();
                }
            }
        });

        Ok(Session {
            start_time: std::time::SystemTime::now(),
            game_info: GameInfo {
                game: local_game,
                patch: local_patch,
            },
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::PvP(PvP {
                match_,
                cancellation_token,
                _peer_conn: peer_conn,
                latency_counter,
            }),
            completion_token,
            pause_on_next_frame: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            close_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            own_setup: {
                let assets = local_game.load_rom_assets(local_rom, &local_save.as_raw_wram(), local_patch_overrides)?;
                Some(Setup {
                    game_lang: local_patch_overrides
                        .language
                        .clone()
                        .unwrap_or_else(|| crate::game::region_to_language(local_game.gamedb_entry().region)),
                    save: local_save,
                    assets,
                })
            },
            opponent_setup: if reveal_setup {
                let assets =
                    remote_game.load_rom_assets(remote_rom, &remote_save.as_raw_wram(), remote_patch_overrides)?;
                Some(Setup {
                    game_lang: remote_patch_overrides
                        .language
                        .clone()
                        .unwrap_or_else(|| crate::game::region_to_language(remote_game.gamedb_entry().region)),
                    save: remote_save,
                    assets,
                })
            } else {
                None
            },
        })
    }

    pub fn new_singleplayer(
        audio_binder: audio::LateBinder,
        game: &'static (dyn game::Game + Send + Sync),
        patch: Option<(String, semver::Version)>,
        rom: &[u8],
        save_file: std::fs::File,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
    ) -> Result<Self, anyhow::Error> {
        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;

        let save_vf = mgba::vfile::VFile::from_file(save_file);

        core.as_mut().load_save(save_vf)?;

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(game.gamedb_entry()).unwrap();
        hooks.patch(core.as_mut());

        let thread = mgba::thread::Thread::new(core);

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        ))))?;

        let pause_on_next_frame = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        thread.set_frame_callback({
            let joyflags = joyflags.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let pause_on_next_frame = pause_on_next_frame.clone();
            move |mut core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut vbuf);
                core.set_keys(joyflags.load(std::sync::atomic::Ordering::Relaxed));
                emu_tps_counter.lock().mark();

                if pause_on_next_frame.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    thread_handle.pause();
                }
            }
        });
        Ok(Session {
            start_time: std::time::SystemTime::now(),
            game_info: GameInfo { game, patch },
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags,
            mode: Mode::SinglePlayer(SinglePlayer {}),
            pause_on_next_frame,
            completion_token: tango_pvp::hooks::CompletionToken::new(),
            close_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            own_setup: None,
            opponent_setup: None,
        })
    }

    pub fn new_replayer(
        audio_binder: audio::LateBinder,
        game: &'static (dyn game::Game + Send + Sync),
        patch: Option<(String, semver::Version)>,
        rom: Arc<Vec<u8>>,
        emu_tps_counter: Arc<Mutex<stats::Counter>>,
        replay: Arc<tango_pvp::replay::Replay>,
    ) -> Result<Self, anyhow::Error> {
        Self::build_replayer_from(BuildReplayerArgs {
            audio_binder,
            game,
            patch,
            rom,
            emu_tps_counter,
            replay,
        })
    }

    fn build_replayer_from(args: BuildReplayerArgs) -> Result<Self, anyhow::Error> {
        let BuildReplayerArgs {
            audio_binder,
            game,
            patch,
            rom,
            emu_tps_counter,
            replay,
        } = args;

        let mut core = mgba::core::Core::new_gba("tango")?;
        core.enable_video_buffer();

        core.as_mut()
            .load_rom(mgba::vfile::VFile::from_vec(rom.as_ref().clone()))?;
        core.as_mut()
            .load_save(mgba::vfile::VFile::from_vec(replay.local_sram.clone()))?;

        let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(game.gamedb_entry()).unwrap();
        hooks.patch(core.as_mut());

        let completion_token = tango_pvp::hooks::CompletionToken::new();

        let replay_is_complete = replay.is_complete;
        if replay.rounds.is_empty() {
            return Err(anyhow::anyhow!("replay has no rounds"));
        }

        let total_replay_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();

        let stepper_state = tango_pvp::stepper::State::new(
            (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            replay.local_player_index,
            replay.rounds.clone(),
            0,
            replay.rng_seed,
            replay.is_offerer,
            total_replay_ticks,
            Box::new({
                let completion_token = completion_token.clone();
                move || {
                    completion_token.complete();
                }
            }),
        );
        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        core.set_traps(traps);

        let thread = mgba::thread::Thread::new(core);

        let snapshots: Arc<parking_lot::Mutex<Vec<ReplaySnapshot>>> = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let prefetch_progress = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let prefetcher = Prefetcher::spawn(
            rom.clone(),
            replay.clone(),
            game,
            snapshots.clone(),
            prefetch_progress.clone(),
        );

        thread.start()?;
        thread.handle().lock_audio().sync_mut().set_fps_target(EXPECTED_FPS);

        let audio_binding = audio_binder.bind(Some(Box::new(audio::MGBAStream::new(
            thread.handle(),
            audio_binder.sample_rate(),
        ))))?;

        let pause_on_next_frame = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));
        thread.set_frame_callback({
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let completion_token = completion_token.clone();
            let stepper_state = stepper_state.clone();
            let pause_on_next_frame = pause_on_next_frame.clone();
            let snapshots = snapshots.clone();
            move |core, video_buffer, mut thread_handle| {
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(video_buffer);
                video::fix_vbuf_alpha(&mut vbuf);
                emu_tps_counter.lock().mark();

                // Surface stepper errors so they don't sit silently in the
                // InnerState. Without this, set_anyhow_error from per-game
                // traps (packet-tick mismatch, etc.) is invisible — the trap
                // just returns early and the game silently hangs.
                if let Some(err) = stepper_state.lock_inner().take_error() {
                    log::error!("replay stepper error: {:?}", err);
                }

                // Capture round-start + periodic mid-round snapshots so
                // seeking can jump rather than re-run frames. Snapshots are
                // replay-deterministic; revisiting the same tick later
                // reuses the existing one.
                let checkpoint = stepper_state.capture_replay_checkpoint();
                let total_left = stepper_state.lock_inner().total_input_pairs_left();
                if let Some(cp) = checkpoint {
                    let mut snaps = snapshots.lock();
                    let want_round_start = !cp.has_committed_this_round
                        && !snaps
                            .iter()
                            .any(|s| s.round_index == cp.current_round_index && !s.has_committed_this_round);
                    let lo = cp.absolute_tick.saturating_sub(MID_ROUND_SNAPSHOT_INTERVAL);
                    let want_mid_round = cp.has_committed_this_round
                        && !snaps
                            .iter()
                            .any(|s| s.absolute_tick > lo && s.absolute_tick <= cp.absolute_tick);
                    if want_round_start || want_mid_round {
                        if let Ok(state) = core.save_state() {
                            snaps.push(ReplaySnapshot {
                                round_index: cp.current_round_index,
                                absolute_tick: cp.absolute_tick,
                                current_tick_in_round: cp.current_tick_in_round,
                                has_committed_this_round: cp.has_committed_this_round,
                                rng_state: cp.rng_state,
                                local_packet: cp.local_packet,
                                inputs_consumed: cp.inputs_consumed,
                                mgba_state: Arc::new(*state),
                            });
                        }
                    }
                }

                // Fire completion regardless of replay_is_complete: for clean
                // replays the stepper's on_round_ended callback handles the
                // first completion, but it's a one-shot, so this path covers
                // re-completion after the user scrubs back and replays past
                // the end again. CompletionToken::complete is idempotent.
                let _ = replay_is_complete;
                if total_left == 0 {
                    completion_token.complete();
                }

                if pause_on_next_frame.swap(false, std::sync::atomic::Ordering::SeqCst)
                    || completion_token.is_complete()
                {
                    thread_handle.pause();
                }
            }
        });

        Ok(Session {
            start_time: std::time::SystemTime::now(),
            game_info: GameInfo { game, patch },
            vbuf,
            _audio_binding: audio_binding,
            thread,
            joyflags: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            mode: Mode::Replayer(Replayer {
                replay,
                stepper_state,
                snapshots,
                prefetch_progress,
                _prefetcher: prefetcher,
            }),
            completion_token,
            pause_on_next_frame,
            close_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            own_setup: None,
            opponent_setup: None,
        })
    }

    pub fn completed(&self) -> bool {
        self.completion_token.is_complete()
    }

    pub fn request_close(&self) {
        self.close_requested.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn close_requested(&self) -> bool {
        self.close_requested.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Total ticks across all rounds of the replay this Session is playing.
    /// Returns None for non-replay sessions.
    pub fn replay_total_ticks(&self) -> Option<u32> {
        let Mode::Replayer(r) = &self.mode else { return None };
        Some(r.stepper_state.lock_inner().total_replay_ticks())
    }

    /// Current playback position (monotonic across rounds). None for
    /// non-replay sessions.
    pub fn replay_current_tick(&self) -> Option<u32> {
        let Mode::Replayer(r) = &self.mode else { return None };
        Some(r.stepper_state.lock_inner().absolute_tick())
    }

    /// Furthest absolute_tick the background prefetch worker has reached.
    /// The seek-bar UI uses this to clamp user drags and render a buffered
    /// fill. None for non-replay sessions.
    pub fn replay_prefetch_progress(&self) -> Option<u32> {
        let Mode::Replayer(r) = &self.mode else { return None };
        Some(r.prefetch_progress.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Absolute ticks at which rounds 2..N begin (i.e., the boundaries
    /// between rounds, exclusive of 0 and total). Used by the seek-bar UI
    /// to draw round markers. None for non-replay sessions.
    pub fn replay_round_boundaries(&self) -> Option<Vec<u32>> {
        let Mode::Replayer(r) = &self.mode else { return None };
        let mut acc: u32 = 0;
        let boundaries = r
            .replay
            .rounds
            .iter()
            .take(r.replay.rounds.len().saturating_sub(1))
            .map(|round| {
                acc += round.len() as u32;
                acc
            })
            .collect();
        Some(boundaries)
    }

    /// Seek the live replay to `target` synchronously. Both directions go
    /// through the snapshot list to find the closest jumping-off point: for
    /// a backward seek the largest snapshot ≤ target, for a forward seek
    /// the largest snapshot in `(current, target]` (or `current` itself if
    /// no snapshot is closer to target than current). Then runs frames
    /// on the mgba thread until `absolute_tick >= target`, capturing
    /// intermediate snapshots along the way so subsequent scrubs in the
    /// region land instantly.
    ///
    /// The whole call is synchronous via `run_on_core`, so by the time
    /// it returns the playback head is at the target — no async catch-up.
    /// Returns `Ok(false)` if the session isn't a replay or the target is
    /// already at the current tick. Returns an error only if no snapshot
    /// exists at or before a backward target (boot window before the first
    /// round-start snapshot — caller can ignore).
    pub fn replay_seek_to(&self, target: u32) -> anyhow::Result<bool> {
        let Mode::Replayer(r) = &self.mode else {
            return Ok(false);
        };
        let current = r.stepper_state.lock_inner().absolute_tick();
        if target == current {
            return Ok(false);
        }

        let start_snap: Option<ReplaySnapshot> = if target < current {
            r.best_snapshot_for(target)
        } else {
            r.snapshots
                .lock()
                .iter()
                .filter(|s| s.absolute_tick > current && s.absolute_tick <= target)
                .max_by_key(|s| s.absolute_tick)
                .cloned()
        };

        if target < current && start_snap.is_none() {
            anyhow::bail!("no snapshot at or before tick {}", target);
        }

        // Once the user moves the playhead, completion is no longer "now" —
        // clear so the frame_callback's pause-on-complete check stops firing
        // until the next time playback actually reaches the end.
        self.completion_token.reset();

        let stepper_state = r.stepper_state.clone();
        let replay = r.replay.clone();
        let snapshots = r.snapshots.clone();

        self.thread.handle().run_on_core(move |mut core| {
            if let Some(snap) = start_snap.as_ref() {
                if let Err(e) = core.load_state(snap.mgba_state.as_ref()) {
                    log::error!("seek load_state failed: {:?}", e);
                    return;
                }
                let cp = tango_pvp::stepper::ReplayCheckpoint {
                    absolute_tick: snap.absolute_tick,
                    current_round_index: snap.round_index,
                    current_tick_in_round: snap.current_tick_in_round,
                    has_committed_this_round: snap.has_committed_this_round,
                    rng_state: snap.rng_state.clone(),
                    local_packet: snap.local_packet.clone(),
                    inputs_consumed: snap.inputs_consumed,
                };
                if let Err(e) = stepper_state.restore_replay_checkpoint(&cp, &replay.rounds) {
                    log::error!("seek restore_replay_checkpoint failed: {:?}", e);
                    return;
                }
            }

            loop {
                let cur = stepper_state.lock_inner().absolute_tick();
                if cur >= target {
                    break;
                }

                // Capture a snapshot at INTERVAL boundaries during the
                // run-frame loop so future scrubs in this region land
                // instantly without re-running.
                if let Some(cp) = stepper_state.capture_replay_checkpoint() {
                    if cp.has_committed_this_round {
                        let mut snaps = snapshots.lock();
                        let lo = cp.absolute_tick.saturating_sub(MID_ROUND_SNAPSHOT_INTERVAL);
                        let exists = snaps
                            .iter()
                            .any(|s| s.absolute_tick > lo && s.absolute_tick <= cp.absolute_tick);
                        if !exists {
                            if let Ok(state) = core.save_state() {
                                snaps.push(ReplaySnapshot {
                                    round_index: cp.current_round_index,
                                    absolute_tick: cp.absolute_tick,
                                    current_tick_in_round: cp.current_tick_in_round,
                                    has_committed_this_round: cp.has_committed_this_round,
                                    rng_state: cp.rng_state,
                                    local_packet: cp.local_packet,
                                    inputs_consumed: cp.inputs_consumed,
                                    mgba_state: Arc::new(*state),
                                });
                            }
                        }
                    }
                }

                core.run_frame();
            }
        });

        Ok(true)
    }

    pub fn set_paused(&self, pause: bool) {
        let handle = self.thread.handle();
        if pause {
            handle.pause();
        } else {
            handle.unpause();
        }
    }

    pub fn is_paused(&self) -> bool {
        let handle = self.thread.handle();
        handle.is_paused()
    }

    pub fn frame_step(&self) {
        self.pause_on_next_frame
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let handle = self.thread.handle();
        handle.unpause();
    }

    pub fn set_fps_target(&self, fps: f32) {
        let handle = self.thread.handle();
        let audio_guard = handle.lock_audio();
        audio_guard.sync_mut().set_fps_target(fps);
    }

    pub fn set_master_volume(&self, volume: i32) {
        let handle = self.thread.handle();
        let mut audio_guard = handle.lock_audio();
        audio_guard.core_mut().gba_mut().set_master_volume(volume);
    }

    pub fn has_crashed(&self) -> Option<mgba::thread::Handle> {
        let handle = self.thread.handle();
        if handle.has_crashed() {
            Some(handle)
        } else {
            None
        }
    }

    pub fn lock_vbuf(&self) -> parking_lot::MutexGuard<'_, Vec<u8>> {
        self.vbuf.lock()
    }

    pub fn thread_handle(&self) -> mgba::thread::Handle {
        self.thread.handle()
    }

    pub fn set_joyflags(&self, joyflags: u32) {
        self.joyflags.store(joyflags, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn start_time(&self) -> std::time::SystemTime {
        self.start_time
    }

    pub fn opponent_setup(&self) -> &Option<Setup> {
        &self.opponent_setup
    }

    pub fn own_setup(&self) -> &Option<Setup> {
        &self.own_setup
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Mode::PvP(pvp) = &mut self.mode {
            pvp.cancellation_token.cancel();
        }
    }
}

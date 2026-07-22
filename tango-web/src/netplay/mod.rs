//! Netplay state, browser flavor: one spawned task owns the connection
//! for its whole life (signaling → control-channel Hello → lobby loop),
//! mirroring the desktop's `netplay` state machine — the settings
//! exchange, the commit/reveal ready ladders, and the StartMatch
//! handoff — with the UI reading a published [`PhaseView`] and driving
//! the task over a command channel.
//!
//! All wire behavior (packet codec, commitment construction, chunking,
//! determinism derivations) comes from `tango-net-protocol`, so this
//! peer is byte-compatible with the desktop by construction.

use futures::channel::mpsc;
use futures::{FutureExt, StreamExt};

use dioxus::prelude::*;
use subtle::ConstantTimeEq;
use tango_net_protocol::control as protocol;
use tango_net_protocol::derive;

use crate::library;
use crate::net::{control, signaling, webrtc};

/// What the UI renders. Published whole on every state change — the
/// lobby is low-frequency, so a coarse clone-out keeps the task and the
/// component tree decoupled.
#[derive(Clone, Default)]
pub enum PhaseView {
    #[default]
    Idle,
    Connecting {
        link_code: String,
    },
    Lobby(LobbyView),
    /// Both StartMatches exchanged; the PvP session takes over (M4
    /// boots it — until then this is the M3 milestone line).
    Starting,
    Failed {
        error: String,
    },
}

#[derive(Clone)]
pub struct LobbyView {
    pub link_code: String,
    pub local_settings: protocol::Settings,
    pub remote_settings: Option<protocol::Settings>,
    pub local_ready: bool,
    pub remote_ready: bool,
    /// We verified their reveal and sent StartMatch.
    pub match_ready: bool,
    pub latency_ms: Option<f32>,
    /// The compat verdict for the two settings, `None` until the peer's
    /// settings arrive.
    pub compatible: Option<bool>,
}

pub static PHASE: GlobalSignal<PhaseView> = Signal::global(PhaseView::default);

/// Everything the M4 PvP session needs, drained by the session boot.
/// Published when both StartMatches have crossed.
#[allow(dead_code)] // the PvP session boot consumes this (M4)
pub struct PreMatch {
    pub is_offerer: bool,
    pub rng_seed: [u8; 16],
    pub match_ts: u64,
    pub match_type: (u8, u8),
    pub local_settings: protocol::Settings,
    pub remote_settings: protocol::Settings,
    pub local_save: Vec<u8>,
    pub remote_save: Vec<u8>,
    pub local_game: library::GameRef,
    pub remote_game: library::GameRef,
    pub control_tx: control::Sender,
    pub control_rx: control::Receiver,
    pub in_match_tx: webrtc::ChannelSender,
    pub in_match_rx: webrtc::ChannelReceiver,
    pub pc: webrtc::PeerConnection,
    pub reconnect_session_id: String,
}

thread_local! {
    /// The pre-match handoff slot: the lobby task deposits, the session
    /// boot takes. A thread_local because the payload is `!Send` (JS
    /// handles) — same single-thread world as everything else here.
    pub static PRE_MATCH: std::cell::RefCell<Option<PreMatch>> = const { std::cell::RefCell::new(None) };
}

/// UI → task commands.
pub enum Command {
    /// (Re)announce our settings. Material changes drop both commits.
    SetSettings(protocol::Settings),
    /// Commit with this save (the picked save's bytes at press time).
    Ready { save_data: Vec<u8> },
    Unready,
    Disconnect,
}

pub struct Handle {
    pub commands: mpsc::UnboundedSender<Command>,
}

thread_local! {
    /// The live lobby task's command handle, if a connection is up.
    static HANDLE: std::cell::RefCell<Option<Handle>> = const { std::cell::RefCell::new(None) };
}

pub fn send_command(cmd: Command) {
    HANDLE.with(|h| {
        if let Some(handle) = h.borrow().as_ref() {
            let _ = handle.commands.unbounded_send(cmd);
        }
    });
}

pub fn disconnect() {
    send_command(Command::Disconnect);
    HANDLE.with(|h| h.borrow_mut().take());
}

/// Kick a connection off. The task owns everything until handoff or
/// failure. `patch_tags` snapshots the synced patches' compatibility
/// tags — (name, version) → tag — so the compat gate can resolve
/// patched setups (an unknown patch is incompatible: we couldn't
/// simulate the peer's side).
pub fn connect(
    link_code: String,
    local_settings: protocol::Settings,
    patch_tags: std::collections::HashMap<(String, String), String>,
) {
    disconnect();
    let (tx, rx) = mpsc::unbounded();
    HANDLE.with(|h| *h.borrow_mut() = Some(Handle { commands: tx }));
    *PHASE.write() = PhaseView::Connecting {
        link_code: link_code.clone(),
    };
    crate::compat::spawn_local(async move {
        match run(link_code, local_settings, patch_tags, rx).await {
            Ok(()) => {}
            Err(e) => {
                log::error!("netplay: {e:#}");
                *PHASE.write() = PhaseView::Failed {
                    error: format!("{e:#}"),
                };
                HANDLE.with(|h| h.borrow_mut().take());
            }
        }
    });
}

/// Our commit ladder rung, desktop's `LocalReady` compressed to what
/// the web lobby needs (the spawn guards fold into the loop).
#[derive(Default)]
enum LocalReady {
    #[default]
    NotReady,
    Committed {
        state: protocol::NegotiatedState,
        compressed: Vec<u8>,
        chunks_sent: bool,
        start_match_sent: bool,
    },
}

/// The peer's ladder as observed from packets — desktop's
/// `RemoteReady`.
#[derive(Default)]
enum RemoteReady {
    #[default]
    NotReady,
    Committed {
        commitment: [u8; 16],
        expected: Option<u64>,
        chunks: Vec<u8>,
        revealed: bool,
        start_match: bool,
    },
}

async fn run(
    link_code: String,
    mut local_settings: protocol::Settings,
    patch_tags: std::collections::HashMap<(String, String), String>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) -> anyhow::Result<()> {
    let endpoint = crate::config::matchmaking_endpoint();
    let connected = signaling::connect(&endpoint, &link_code, crate::config::use_relay_pref())
        .await
        .map_err(|e| anyhow::anyhow!("signaling: {e}"))?;

    let signaling::Connected {
        pc,
        control_tx,
        control_rx,
        in_match_tx,
        in_match_rx,
        control_open,
        is_offerer,
        local_dtls_fingerprint,
        peer_dtls_fingerprint,
        peer_client_cert_fingerprint: _,
    } = connected;

    // The channel must actually open before the first Hello.
    control_open
        .await
        .map_err(|_| anyhow::anyhow!("control channel never opened"))?;

    let tx = control::Sender::new(control_tx);
    let mut rx = control::Receiver::new(control_rx);
    control::negotiate(&tx, &mut rx).await.map_err(|e| anyhow::anyhow!("negotiate: {e}"))?;
    log::info!("netplay: negotiated protocol 0x{:x}", protocol::VERSION as u32);

    tx.send_settings(local_settings.clone())?;

    let mut view = LobbyView {
        link_code: link_code.clone(),
        local_settings: local_settings.clone(),
        remote_settings: None,
        local_ready: false,
        remote_ready: false,
        match_ready: false,
        latency_ms: None,
        compatible: None,
    };
    *PHASE.write() = PhaseView::Lobby(view.clone());

    let mut local = LocalReady::default();
    let mut remote = RemoteReady::default();

    loop {
        // Re-derived every pass: rungs only climb inside one pairing.
        enum Ev {
            Packet(anyhow::Result<protocol::Packet>),
            Command(Option<Command>),
            PingDue,
        }
        let ev = futures::select! {
            p = rx.receive().fuse() => Ev::Packet(p),
            c = commands.next() => Ev::Command(c),
            _ = crate::compat::sleep_ms(1_000).fuse() => Ev::PingDue,
        };

        match ev {
            Ev::PingDue => {
                let ts = (crate::compat::now_unix_ms() as u64 % 65536) as u16;
                let _ = tx.send_ping(ts);
            }
            Ev::Command(None) | Ev::Command(Some(Command::Disconnect)) => {
                let _ = tx.send_goodbye();
                *PHASE.write() = PhaseView::Idle;
                return Ok(());
            }
            Ev::Command(Some(Command::SetSettings(settings))) => {
                // A material change voids both sides' commits, exactly
                // like the desktop's ladder reset.
                let material = settings.game_info != local_settings.game_info
                    || settings.match_type != local_settings.match_type
                    || settings.blind_setup != local_settings.blind_setup;
                local_settings = settings.clone();
                view.local_settings = settings.clone();
                if material {
                    if matches!(local, LocalReady::Committed { .. }) {
                        let _ = tx.send_uncommit();
                    }
                    local = LocalReady::NotReady;
                    remote_reset_derived(&mut remote, &mut view);
                    view.local_ready = false;
                }
                tx.send_settings(settings)?;
                publish_with(&mut view, &local, &remote, &patch_tags);
            }
            Ev::Command(Some(Command::Ready { save_data })) => {
                if matches!(local, LocalReady::Committed { .. }) {
                    continue;
                }
                let state = protocol::NegotiatedState {
                    nonce: rand::random(),
                    ts: crate::compat::now_unix_ms() as u64,
                    save_data,
                };
                let compressed = zstd::stream::encode_all(
                    state.serialize()?.as_slice(),
                    3,
                )?;
                let commitment = protocol::make_commitment(&compressed);
                tx.send_commit(commitment)?;
                local = LocalReady::Committed {
                    state,
                    compressed,
                    chunks_sent: false,
                    start_match_sent: false,
                };
                maybe_advance(&tx, &mut local, &mut remote)?;
                publish_with(&mut view, &local, &remote, &patch_tags);
            }
            Ev::Command(Some(Command::Unready)) => {
                if matches!(local, LocalReady::Committed { .. }) {
                    let _ = tx.send_uncommit();
                }
                local = LocalReady::NotReady;
                publish_with(&mut view, &local, &remote, &patch_tags);
            }
            Ev::Packet(Err(e)) => {
                anyhow::bail!("peer disconnected: {e}");
            }
            Ev::Packet(Ok(p)) => match p {
                protocol::Packet::Hello(_) => anyhow::bail!("stray Hello after negotiate"),
                protocol::Packet::Ping(ping) => {
                    let _ = tx.send_pong(ping.ts);
                }
                protocol::Packet::Pong(pong) => {
                    let now = (crate::compat::now_unix_ms() as u64 % 65536) as u16;
                    view.latency_ms = Some(now.wrapping_sub(pong.ts) as f32);
                    publish_with(&mut view, &local, &remote, &patch_tags);
                }
                protocol::Packet::Settings(settings) => {
                    // Their material change voids their commit (and our
                    // StartMatch predicated on it).
                    view.remote_settings = Some(settings);
                    remote_reset_derived(&mut remote, &mut view);
                    publish_with(&mut view, &local, &remote, &patch_tags);
                }
                protocol::Packet::Commit(commit) => {
                    remote = RemoteReady::Committed {
                        commitment: commit.commitment,
                        expected: None,
                        chunks: Vec::new(),
                        revealed: false,
                        start_match: false,
                    };
                    maybe_advance(&tx, &mut local, &mut remote)?;
                    publish_with(&mut view, &local, &remote, &patch_tags);
                }
                protocol::Packet::Uncommit(_) => {
                    remote = RemoteReady::NotReady;
                    // Our StartMatch was predicated on their reveal.
                    if let LocalReady::Committed {
                        start_match_sent, ..
                    } = &mut local
                    {
                        *start_match_sent = false;
                    }
                    publish_with(&mut view, &local, &remote, &patch_tags);
                }
                protocol::Packet::ChunkStart(cs) => {
                    if let RemoteReady::Committed { expected, .. } = &mut remote {
                        *expected = Some(cs.len);
                    }
                }
                protocol::Packet::Chunk(chunk) => {
                    let RemoteReady::Committed {
                        expected: Some(expected),
                        chunks,
                        revealed,
                        ..
                    } = &mut remote
                    else {
                        continue; // Stray chunk; drop.
                    };
                    chunks.extend_from_slice(&chunk.chunk);
                    if (chunks.len() as u64) >= *expected {
                        *revealed = true;
                        maybe_advance(&tx, &mut local, &mut remote)?;
                        publish_with(&mut view, &local, &remote, &patch_tags);
                    }
                }
                protocol::Packet::StartMatch(_) => {
                    if let RemoteReady::Committed { start_match, .. } = &mut remote {
                        *start_match = true;
                    }
                }
                protocol::Packet::Goodbye(_) => {
                    anyhow::bail!("opponent left the lobby");
                }
            },
        }

        // Handoff gate: our StartMatch sent + theirs received.
        let ours_sent = matches!(
            local,
            LocalReady::Committed {
                start_match_sent: true,
                ..
            }
        );
        let theirs = matches!(remote, RemoteReady::Committed { start_match: true, .. });
        if ours_sent && theirs {
            let LocalReady::Committed { state, .. } = local else {
                unreachable!();
            };
            let RemoteReady::Committed { chunks, .. } = remote else {
                unreachable!();
            };
            let peer_state = decode_reveal(&chunks)?;
            let remote_settings = view
                .remote_settings
                .clone()
                .ok_or_else(|| anyhow::anyhow!("peer never sent settings"))?;
            let (local_game, remote_game) =
                resolve_games(&local_settings, &remote_settings)?;

            let rng_seed = derive::derive_rng_seed(&state.nonce, &peer_state.nonce);
            let match_ts = derive::pick_match_ts(is_offerer, state.ts, peer_state.ts);
            let reconnect_session_id = derive::derive_reconnect_session_id(
                &rng_seed,
                &local_dtls_fingerprint,
                &peer_dtls_fingerprint,
            );

            PRE_MATCH.with(|slot| {
                *slot.borrow_mut() = Some(PreMatch {
                    is_offerer,
                    rng_seed,
                    match_ts,
                    match_type: local_settings.match_type,
                    local_settings,
                    remote_settings,
                    local_save: state.save_data,
                    remote_save: peer_state.save_data,
                    local_game,
                    remote_game,
                    control_tx: tx,
                    control_rx: rx,
                    in_match_tx,
                    in_match_rx,
                    pc,
                    reconnect_session_id,
                })
            });
            *PHASE.write() = PhaseView::Starting;
            HANDLE.with(|h| h.borrow_mut().take());
            return Ok(());
        }
    }
}

/// Climb every rung currently climbable: once both sides committed,
/// stream our reveal; once their reveal verified, send StartMatch.
fn maybe_advance(
    tx: &control::Sender,
    local: &mut LocalReady,
    remote: &mut RemoteReady,
) -> anyhow::Result<()> {
    let LocalReady::Committed {
        compressed,
        chunks_sent,
        start_match_sent,
        ..
    } = local
    else {
        return Ok(());
    };
    let RemoteReady::Committed {
        commitment,
        revealed,
        chunks,
        ..
    } = remote
    else {
        return Ok(());
    };

    if !*chunks_sent {
        tx.send_chunk_start(compressed.len() as u64)?;
        for chunk in compressed.chunks(protocol::REVEAL_CHUNK_SIZE) {
            tx.send_chunk(chunk.to_vec())?;
        }
        *chunks_sent = true;
    }

    if *revealed && !*start_match_sent {
        // Constant-time verify of their reveal against their
        // commitment before we agree to start.
        let expect = protocol::make_commitment(chunks);
        if !bool::from(expect.ct_eq(commitment)) {
            anyhow::bail!("peer's reveal doesn't match its commitment");
        }
        tx.send_start_match()?;
        *start_match_sent = true;
    }
    Ok(())
}

/// Their settings changed or their commit dropped: everything derived
/// from the old commit is void.
fn remote_reset_derived(remote: &mut RemoteReady, _view: &mut LobbyView) {
    *remote = RemoteReady::NotReady;
}

fn decode_reveal(compressed: &[u8]) -> anyhow::Result<protocol::NegotiatedState> {
    let raw = zstd::stream::decode_all(compressed)?;
    Ok(protocol::NegotiatedState::deserialize(&raw)?)
}

/// The compat gate, the web half of the desktop's `netplay::compat`:
/// both sides' netplay-compatibility tags must match (a patch's tag
/// when patched, the raw family otherwise), the match types must
/// agree, and any patch in play must be one we possess (the match
/// re-simulates the peer's side locally, patched identically).
fn compatible(
    local: &protocol::Settings,
    remote: &protocol::Settings,
    patch_tags: &std::collections::HashMap<(String, String), String>,
) -> bool {
    let (Some(lg), Some(rg)) = (&local.game_info, &remote.game_info) else {
        return false;
    };
    let tag = |g: &protocol::GameInfo| -> Option<String> {
        match &g.patch {
            None => Some(g.family_and_variant.0.clone()),
            Some(p) => patch_tags
                .get(&(p.name.clone(), p.version.to_string()))
                .cloned(),
        }
    };
    let (Some(lt), Some(rt)) = (tag(lg), tag(rg)) else {
        return false;
    };
    lt == rt && local.match_type == remote.match_type
}

/// Resolve both sides' games from their settings; both must be
/// registered and their ROMs importable (checked at boot).
fn resolve_games(
    local: &protocol::Settings,
    remote: &protocol::Settings,
) -> anyhow::Result<(library::GameRef, library::GameRef)> {
    let lg = local
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no local game"))?;
    let rg = remote
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no remote game"))?;
    let local_game =
        library::find_by_family_and_variant(&lg.family_and_variant.0, lg.family_and_variant.1)
            .ok_or_else(|| anyhow::anyhow!("local game not registered"))?;
    let remote_game =
        library::find_by_family_and_variant(&rg.family_and_variant.0, rg.family_and_variant.1)
            .ok_or_else(|| anyhow::anyhow!("opponent's game not supported by this build"))?;
    Ok((local_game, remote_game))
}

fn publish_with(
    view: &mut LobbyView,
    local: &LocalReady,
    remote: &RemoteReady,
    patch_tags: &std::collections::HashMap<(String, String), String>,
) {
    view.local_ready = matches!(local, LocalReady::Committed { .. });
    view.remote_ready = matches!(remote, RemoteReady::Committed { .. });
    view.match_ready = matches!(
        local,
        LocalReady::Committed {
            start_match_sent: true,
            ..
        }
    );
    view.compatible = view
        .remote_settings
        .as_ref()
        .map(|r| compatible(&view.local_settings, r, patch_tags));
    *PHASE.write() = PhaseView::Lobby(view.clone());
}

//! Signaling-free WebRTC `DataChannel` transport for the direct
//! local-play link (`/host` and `/connect`). It replaces the old raw-TCP
//! adapter: instead of a TCP stream we bring up a real libdatachannel
//! peer connection, but with **no signaling exchange whatsoever**.
//!
//! Normally the two peers swap SDP (ICE ufrag/pwd + DTLS fingerprint +
//! candidates) through a signaling server. Here both sides instead
//! *fabricate* each other's description from constants they already
//! agree on:
//!
//! * **ICE credentials** are pinned to fixed values (see [`UFRAG_HOST`] /
//!   [`UFRAG_CLIENT`] / [`ICE_PWD`]) via libdatachannel's
//!   `LocalDescriptionInit`, so each side knows the other's ufrag/pwd up
//!   front and ICE connectivity checks validate.
//! * **The DTLS fingerprint** is unknowable without an exchange, so we
//!   disable fingerprint verification ([`RtcConfig::disable_fingerprint_verification`])
//!   and put a dummy (but well-formed) `sha-256` fingerprint in the
//!   fabricated SDP. The handshake still encrypts; it just doesn't pin
//!   the cert.
//! * **Addresses**: the host pins its UDP port (so the dialer knows where
//!   to send), and the dialer's fabricated offer carries a single host
//!   candidate for the typed `addr`. The host itself needs no remote
//!   candidate — it learns the dialer from the incoming STUN check
//!   (peer-reflexive).
//!
//! Two data channels are pre-negotiated on fixed stream ids so there's no
//! in-band DCEP handshake either: a reliable/ordered control channel (stream 0)
//! and an unreliable/unordered in-match channel (stream 1) for the live
//! `data::wire` datagrams. The peer connection must be kept alive by the caller
//! for the channels' lifetime (see `netplay::NegotiationOutput`).

use super::channel::Channels;
use datachannel_wrapper::{LocalDescriptionInit, PeerConnection, RtcConfig, SdpType, SessionDescription};

/// Fixed local ICE ufrag for the host (offerer) side. Must be a valid
/// ICE ufrag (>= 4 chars); both peers know both values.
const UFRAG_HOST: &str = "tangoHost";
/// Fixed local ICE ufrag for the dialing (answerer) side.
const UFRAG_CLIENT: &str = "tangoClient";
/// Shared ICE pwd. Must be a valid ICE pwd (>= 22 chars). Both sides use
/// the same value — ICE only requires each peer to know the other's pwd,
/// and a fixed shared secret satisfies that without an exchange.
const ICE_PWD: &str = "tangoDirectNoSignalingPwd";

/// Build a fabricated remote SDP for the peer. `setup` is the DTLS role
/// the *peer* advertises (`actpass` for an offer, `active` for an answer);
/// `ufrag` is the peer's pinned ICE ufrag. `candidate` is an optional
/// `a=candidate:` payload (host candidate for the dialer's view of the
/// host; `None` when the host learns the peer reflexively).
fn fabricate_sdp(sdp_type: SdpType, setup: &str, ufrag: &str, candidate: Option<&str>) -> SessionDescription {
    // sha-256 fingerprint: 32 colon-joined hex byte pairs (95 chars). The
    // value is a dummy — verification is disabled — but it must be
    // well-formed or the SDP parser rejects it.
    let fingerprint = ["AB"; 32].join(":");

    let mut lines = vec![
        "v=0".to_string(),
        "o=rtc 0 0 IN IP4 127.0.0.1".to_string(),
        "s=-".to_string(),
        "t=0 0".to_string(),
        "a=group:BUNDLE 0".to_string(),
        "a=msid-semantic:WMS *".to_string(),
        format!("a=fingerprint:sha-256 {fingerprint}"),
        "m=application 9 UDP/DTLS/SCTP webrtc-datachannel".to_string(),
        "c=IN IP4 0.0.0.0".to_string(),
        "a=mid:0".to_string(),
        "a=sendrecv".to_string(),
        "a=sctp-port:5000".to_string(),
        "a=max-message-size:262144".to_string(),
        format!("a=setup:{setup}"),
        format!("a=ice-ufrag:{ufrag}"),
        format!("a=ice-pwd:{ICE_PWD}"),
    ];
    if let Some(candidate) = candidate {
        lines.push(format!("a=candidate:{candidate}"));
    }
    lines.push("a=end-of-candidates".to_string());

    // libdatachannel parses on \r\n line endings; trailing CRLF too.
    let mut sdp = lines.join("\r\n");
    sdp.push_str("\r\n");
    SessionDescription { sdp_type, sdp }
}

/// Open both pre-negotiated data channels on a fresh peer connection (the
/// reliable control channel + the unreliable in-match channel) and split each
/// into our transport-agnostic Sender/Receiver, returning the peer connection
/// so the caller can keep it alive.
fn open_channels(mut pc: PeerConnection) -> Channels {
    let (label, init) = super::channel::control_channel();
    let control_dc = pc
        .create_data_channel(label, init)
        .expect("create pre-negotiated control data channel");
    let (label, init) = super::channel::in_match_channel();
    let in_match_dc = pc
        .create_data_channel(label, init)
        .expect("create pre-negotiated in-match data channel");
    Channels {
        control: super::channel::control_pair(control_dc),
        in_match: super::channel::data_pair(in_match_dc),
        peer_conn: pc,
        // Fabricated SDP with fingerprint verification disabled, so the dummy
        // fingerprint is meaningless; the direct path rebuilds via re-run, not a
        // derived session_id. Leave the pair empty.
        local_dtls_fingerprint: Vec::new(),
        peer_dtls_fingerprint: Vec::new(),
    }
}

/// Drain (and log) a peer connection's state changes for its lifetime. The
/// event stream is otherwise unused on the direct path, so without this a
/// drop's cause (ICE/SCTP tearing the link down) is invisible. The task ends
/// when the connection is dropped (the event sender goes away).
fn spawn_state_logger(mut events: tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>) {
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            if let datachannel_wrapper::PeerConnectionEvent::ConnectionStateChange(state) = ev {
                log::info!("pvp peer connection state: {state:?}");
            }
        }
    });
}

/// Host side: pin the UDP `port`, offer with fixed ICE creds, and accept
/// the dialer reflexively. Returns once the descriptions are set; the
/// channels open asynchronously and the first `send` blocks until they do.
pub async fn host(port: u16) -> std::io::Result<Channels> {
    let (pc, events) = PeerConnection::new(RtcConfig {
        disable_fingerprint_verification: true,
        // We drive setLocalDescription ourselves (with pinned ICE creds);
        // an auto offer would race ahead with random creds.
        disable_auto_negotiation: true,
        // Pin the listen port so the dialer's fabricated host candidate
        // can target it.
        port_range: Some((port, port)),
        ..Default::default()
    })?;
    spawn_state_logger(events);

    let mut channels = open_channels(pc);

    channels.peer_conn.set_local_description(
        SdpType::Offer,
        Some(&LocalDescriptionInit {
            ice_ufrag: Some(UFRAG_HOST.to_string()),
            ice_pwd: Some(ICE_PWD.to_string()),
        }),
    )?;
    // The dialer answers as the DTLS client (`active`); no candidate — we
    // learn its address from the incoming connectivity check.
    channels
        .peer_conn
        .set_remote_description(fabricate_sdp(SdpType::Answer, "active", UFRAG_CLIENT, None))?;

    Ok(channels)
}

/// Dialer side: fabricate the host's offer (carrying a host candidate for
/// `addr`), then answer with fixed ICE creds.
pub async fn connect(addr: &str) -> std::io::Result<Channels> {
    // Resolve the typed address into a concrete host candidate.
    let sock = tokio::net::lookup_host(addr)
        .await?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "could not resolve address"))?;
    let candidate = host_candidate(&sock);

    let (pc, events) = PeerConnection::new(RtcConfig {
        disable_fingerprint_verification: true,
        // We drive setLocalDescription ourselves (with pinned ICE creds);
        // an auto offer would race ahead with random creds and make us an
        // offerer instead of the answerer.
        disable_auto_negotiation: true,
        ..Default::default()
    })?;
    spawn_state_logger(events);

    let mut channels = open_channels(pc);

    // The host offers as `actpass`; we become the DTLS client by answering
    // `active`. Set the remote offer first, then generate our answer.
    channels.peer_conn.set_remote_description(fabricate_sdp(
        SdpType::Offer,
        "actpass",
        UFRAG_HOST,
        Some(&candidate),
    ))?;
    channels.peer_conn.set_local_description(
        SdpType::Answer,
        Some(&LocalDescriptionInit {
            ice_ufrag: Some(UFRAG_CLIENT.to_string()),
            ice_pwd: Some(ICE_PWD.to_string()),
        }),
    )?;

    Ok(channels)
}

/// Format an `a=candidate:` payload (everything after `a=candidate:`) for
/// a single UDP host candidate at `sock`.
fn host_candidate(sock: &std::net::SocketAddr) -> String {
    // foundation=1, component=1 (RTP), udp, an arbitrary host-typed
    // priority. libjuice resolves the rest from the connectivity check.
    format!("1 1 udp 2122260223 {} {} typ host", sock.ip(), sock.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: host + dialer bring up both channels from fabricated SDP
    /// alone (no signaling), run the real protocol-version handshake over the
    /// reliable channel, then round-trip a raw datagram over the unreliable
    /// in-match channel. Proves ICE + DTLS + SCTP all complete and that the
    /// second pre-negotiated (stream-1, unreliable) channel opens and carries
    /// traffic both ways.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fabricated_sdp_round_trips() {
        // A high, unlikely-to-clash loopback port for the test host.
        let port = 24987;
        let addr = format!("127.0.0.1:{port}");
        let (host_res, conn_res) = tokio::join!(host(port), connect(&addr));
        let mut host_ch = host_res.expect("host setup");
        let mut conn_ch = conn_res.expect("connect setup");

        // `negotiate`'s first send blocks until the channel opens, so this
        // drives the whole ICE/DTLS bring-up. Guard with a timeout so a
        // failure surfaces as a panic rather than a hang.
        let handshake = async {
            tokio::try_join!(
                crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
            )
        };
        tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
            .await
            .expect("handshake timed out — channel never opened")
            .expect("negotiate failed");

        // The unreliable in-match channel shares the same association, so it's
        // open by now too — round-trip a raw datagram each way.
        let in_match = async {
            host_ch.in_match.0.send(b"ping-h2c").await?;
            conn_ch.in_match.0.send(b"ping-c2h").await?;
            let got_at_conn = conn_ch.in_match.1.recv().await?;
            let got_at_host = host_ch.in_match.1.recv().await?;
            Ok::<_, std::io::Error>((got_at_conn, got_at_host))
        };
        let (got_at_conn, got_at_host) = tokio::time::timeout(std::time::Duration::from_secs(15), in_match)
            .await
            .expect("in-match datagram timed out — second channel never opened")
            .expect("in-match send/recv failed");
        assert_eq!(got_at_conn, b"ping-h2c");
        assert_eq!(got_at_host, b"ping-c2h");
    }

    /// The transparent-reconnect primitive: after a live link is torn down,
    /// both peers must be able to rebuild it from the *same* recipe (host
    /// re-pins the same UDP port; dialer re-dials the same address) and carry
    /// traffic again. This is exactly what the in-match reconnect coordinator
    /// does on a drop — including the one real race, the host re-binding its
    /// pinned port the instant the old peer connection is dropped.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rebuild_after_drop_round_trips() {
        let port = 24988;
        let addr = format!("127.0.0.1:{port}");

        // Bring up the link, handshake, and prove it carries a datagram.
        let bring_up = || async {
            let (host_res, conn_res) = tokio::join!(host(port), connect(&addr));
            let mut host_ch = host_res.expect("host setup");
            let mut conn_ch = conn_res.expect("connect setup");
            let handshake = async {
                tokio::try_join!(
                    crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                    crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
                )
            };
            tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
                .await
                .expect("handshake timed out — channel never opened")
                .expect("negotiate failed");
            (host_ch, conn_ch)
        };

        let (host_ch, conn_ch) = bring_up().await;
        // Simulate the drop: tearing down both peer connections releases the
        // host's pinned UDP port.
        drop(host_ch);
        drop(conn_ch);

        // Rebuild from the identical recipe — host must re-bind the just-freed
        // port. If the OS hadn't released it, `host(port)` here would fail.
        let (mut host_ch, mut conn_ch) = bring_up().await;

        // The rebuilt in-match channel carries traffic both ways.
        host_ch
            .in_match
            .0
            .send(b"reconnected-h2c")
            .await
            .expect("post-reconnect host send");
        conn_ch
            .in_match
            .0
            .send(b"reconnected-c2h")
            .await
            .expect("post-reconnect conn send");
        let roundtrip = async {
            let at_conn = conn_ch.in_match.1.recv().await?;
            let at_host = host_ch.in_match.1.recv().await?;
            Ok::<_, std::io::Error>((at_conn, at_host))
        };
        let (at_conn, at_host) = tokio::time::timeout(std::time::Duration::from_secs(15), roundtrip)
            .await
            .expect("post-reconnect datagram timed out — rebuilt channel never opened")
            .expect("post-reconnect send/recv failed");
        assert_eq!(at_conn, b"reconnected-h2c");
        assert_eq!(at_host, b"reconnected-c2h");
    }
}

/// Synthetic skew A/B measurement, transport-stack-agnostic: everything it
/// touches (`direct_rtc::{host,connect}` → [`Channels`] → the raw in-match
/// byte pipe) exists identically on the libdatachannel and tango-rtc trees, so
/// the same test compiled on each gives directly comparable numbers.
///
/// It models exactly what tango's in-match skew telemetry computes: two peers
/// tick at 60Hz through a loss/delay/jitter-injecting UDP proxy; each tick a
/// side samples `skew = lead − last_remote_lead` (before sending, as getgud
/// reads it), then sends `(tick, lead)`; `lead` is its tick counter minus the
/// remote tick frontier it has seen. Wobble = how much that skew series
/// swings.
///
/// Run by hand: `cargo test -p tango --release skew_wobble -- --ignored --nocapture`
#[cfg(test)]
mod skew_ab {
    use super::*;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0.wrapping_mul(0x2545F4914F6CDD1D)
        }

        fn chance(&mut self, permille: u32) -> bool {
            (self.next() >> 32) as u32 % 1000 < permille
        }

        fn unit(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    async fn lossy_proxy(
        listen: std::sync::Arc<tokio::net::UdpSocket>,
        forward_to: std::net::SocketAddr,
        loss_permille: u32,
        delay: std::time::Duration,
        jitter: std::time::Duration,
        seed: u64,
    ) {
        let host_side = std::sync::Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let mut dialer: Option<std::net::SocketAddr> = None;
        let mut rng = Rng(seed | 1);
        let mut jitter_rng = Rng((seed ^ 0x9E3779B97F4A7C15) | 1);
        let mut buf_a = vec![0u8; 2048];
        let mut buf_b = vec![0u8; 2048];
        let deliver = |sock: std::sync::Arc<tokio::net::UdpSocket>,
                       payload: Vec<u8>,
                       to: std::net::SocketAddr,
                       wait: std::time::Duration| async move {
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            let _ = sock.send_to(&payload, to).await;
        };
        loop {
            let extra = if jitter.is_zero() {
                std::time::Duration::ZERO
            } else {
                jitter.mul_f64(jitter_rng.unit())
            };
            tokio::select! {
                r = listen.recv_from(&mut buf_a) => {
                    let Ok((n, from)) = r else { break };
                    dialer = Some(from);
                    if !rng.chance(loss_permille) {
                        tokio::spawn(deliver(host_side.clone(), buf_a[..n].to_vec(), forward_to, delay + extra));
                    }
                }
                r = host_side.recv_from(&mut buf_b) => {
                    let Ok((n, _)) = r else { break };
                    if let Some(dialer) = dialer {
                        if !rng.chance(loss_permille) {
                            tokio::spawn(deliver(listen.clone(), buf_b[..n].to_vec(), dialer, delay + extra));
                        }
                    }
                }
            }
        }
    }

    /// One peer's 60Hz tick loop over the raw in-match byte pipe.
    async fn run_side(
        mut tx: crate::net::data::Sender,
        mut rx: crate::net::data::Receiver,
        ticks: u32,
        warmup: u32,
    ) -> Vec<i32> {
        let mut interval = tokio::time::interval(std::time::Duration::from_micros(16_667));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
        let mut tick: u32 = 0;
        // Highest remote tick seen + 1 (count of confirmed remote ticks).
        let mut remote_frontier: u32 = 0;
        let mut last_remote_lead: i32 = 0;
        let mut skew_samples = vec![];
        loop {
            tokio::select! {
                _ = interval.tick(), if tick < ticks => {
                    let lead = tick as i32 - remote_frontier as i32;
                    if tick >= warmup {
                        skew_samples.push(lead - last_remote_lead);
                    }
                    let mut msg = [0u8; 8];
                    msg[..4].copy_from_slice(&tick.to_le_bytes());
                    msg[4..].copy_from_slice(&lead.to_le_bytes());
                    tx.send(&msg).await.expect("send");
                    tick += 1;
                }
                r = rx.recv() => {
                    let msg = r.expect("recv");
                    let rtick = u32::from_le_bytes(msg[..4].try_into().unwrap());
                    let rlead = i32::from_le_bytes(msg[4..8].try_into().unwrap());
                    if rtick + 1 > remote_frontier {
                        remote_frontier = rtick + 1;
                        last_remote_lead = rlead;
                    }
                    if remote_frontier >= ticks && tick >= ticks {
                        break;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)), if tick >= ticks => break,
            }
        }
        skew_samples
    }

    fn report(name: &str, samples: &[i32]) {
        let n = samples.len() as f64;
        let mean = samples.iter().map(|&s| s as f64).sum::<f64>() / n;
        let var = samples.iter().map(|&s| (s as f64 - mean).powi(2)).sum::<f64>() / n;
        // Mean over sliding 1s (60-sample) windows of the within-window swing
        // (max − min): the "how much does the needle move per second" number.
        let mut swings = vec![];
        for w in samples.windows(60) {
            let mx = *w.iter().max().unwrap();
            let mn = *w.iter().min().unwrap();
            swings.push((mx - mn) as f64);
        }
        let mean_swing = swings.iter().sum::<f64>() / swings.len() as f64;
        let max_swing = swings.iter().cloned().fold(0.0, f64::max);
        println!(
            "  {name}: n={} mean={:+.2} std={:.2} min={:+} max={:+} | swing/1s mean={:.1} max={:.1}",
            samples.len(),
            mean,
            var.sqrt(),
            samples.iter().min().unwrap(),
            samples.iter().max().unwrap(),
            mean_swing,
            max_swing,
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "diagnostic measurement, run with --ignored --nocapture"]
    async fn skew_wobble_synthetic() {
        for (loss_permille, jitter_ms, seed) in [
            (0u32, 0u64, 0x5EEDu64),
            (100, 0, 0x5EED),
            (100, 0, 0xACE),
            (100, 10, 0x5EED),
            (100, 10, 0xACE),
        ] {
            let port = 24990;
            let delay = std::time::Duration::from_millis(30);
            let jitter = std::time::Duration::from_millis(jitter_ms);
            println!(
                "=== {}‰ loss, 30ms one-way delay, {}ms jitter (seed {:#x}) ===",
                loss_permille, jitter_ms, seed
            );

            let proxy_sock = std::sync::Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
            let proxy_addr = proxy_sock.local_addr().unwrap();
            let proxy = tokio::spawn(lossy_proxy(
                proxy_sock,
                format!("127.0.0.1:{port}").parse().unwrap(),
                loss_permille,
                delay,
                jitter,
                seed,
            ));

            let proxy_addr_str = proxy_addr.to_string();
            let (host_res, conn_res) = tokio::join!(host(port), connect(&proxy_addr_str));
            let mut host_ch = host_res.expect("host setup");
            let mut conn_ch = conn_res.expect("connect setup");
            let handshake = async {
                tokio::try_join!(
                    crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                    crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
                )
            };
            tokio::time::timeout(std::time::Duration::from_secs(20), handshake)
                .await
                .expect("handshake timed out")
                .expect("negotiate failed");

            // 12s at 60Hz, first 2s discarded as warmup.
            let ticks = 60 * 12;
            let warmup = 120;
            let (h, c) = tokio::join!(
                run_side(host_ch.in_match.0, host_ch.in_match.1, ticks, warmup),
                run_side(conn_ch.in_match.0, conn_ch.in_match.1, ticks, warmup),
            );
            report("host  ", &h);
            report("dialer", &c);
            proxy.abort();
            drop(host_ch.peer_conn);
            drop(conn_ch.peer_conn);
            // Let the ports free up before the next scenario re-binds.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}

//! Signaling-free WebRTC transport for the direct local-play link (`/host`
//! and `/connect`): a [`tango_rtc`] peer connection with **no signaling
//! exchange whatsoever**. Everything a normal SDP exchange would carry is
//! either a fixed constant both ends already know (ICE credentials, DTLS/SCTP
//! roles) or comes from the link code itself (the host's `addr:port`); DTLS
//! fingerprint verification is off — there's no channel to learn the peer's
//! cert over, so the trust model is "address = identity". See
//! [`tango_rtc::PeerConnection::new_direct`].
//!
//! Both pre-declared data channels (the reliable control channel + the
//! unreliable in-match channel, see [`super::channel`]) are opened over DCEP
//! by the dialer and bound by label on the host. The peer connection must be
//! kept alive by the caller for the channels' lifetime (see
//! `netplay::NegotiationOutput`).

use super::channel::Channels;
use tango_rtc::{DirectRole, PeerConnection, PeerConnectionEvent, RtcConfig};

/// Bring up one end of the direct link and bundle it as [`Channels`]. Returns
/// as soon as the transport is set up; the channels open asynchronously and
/// the first `send` blocks until they do.
fn open(role: DirectRole) -> std::io::Result<Channels> {
    let (peer_conn, dcs, events) = PeerConnection::new_direct(
        RtcConfig::default(),
        &[super::channel::control_channel(), super::channel::in_match_channel()],
        role,
    )?;
    spawn_state_logger(events);
    let [control_dc, in_match_dc] = <[_; 2]>::try_from(dcs)
        .map_err(|dcs: Vec<_>| std::io::Error::other(format!("expected 2 data channels, got {}", dcs.len())))?;
    Ok(Channels {
        control: super::channel::control_pair(control_dc),
        in_match: super::channel::data_pair(in_match_dc),
        peer_conn,
        // No SDP exchange and fingerprint verification is off; the direct
        // path rebuilds via re-run, not a fingerprint-derived session_id.
        local_dtls_fingerprint: Vec::new(),
        peer_dtls_fingerprint: Vec::new(),
    })
}

/// Drain (and log) a peer connection's state changes for its lifetime. The
/// event stream is otherwise unused on the direct path, so without this a
/// drop's cause (ICE/DTLS tearing the link down) is invisible. The task ends
/// when the connection is dropped (the event sender goes away).
fn spawn_state_logger(mut events: tokio::sync::mpsc::Receiver<PeerConnectionEvent>) {
    tokio::spawn(async move {
        while let Some(ev) = events.recv().await {
            if let PeerConnectionEvent::ConnectionStateChange(state) = ev {
                log::info!("pvp peer connection state: {state:?}");
            }
        }
    });
}

/// Host side: pin the UDP `port` and accept the first inbound peer (learned
/// peer-reflexively from its ICE checks — the host itself never dials out).
pub async fn host(port: u16) -> std::io::Result<Channels> {
    open(DirectRole::Host { port })
}

/// Dialer side: resolve the typed address and dial the host at it.
pub async fn connect(addr: &str) -> std::io::Result<Channels> {
    let remote = tokio::net::lookup_host(addr)
        .await?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "could not resolve address"))?;
    open(DirectRole::Connect { remote })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: host + dialer bring up both channels with no signaling,
    /// run the real protocol-version handshake over the reliable channel,
    /// then round-trip a raw datagram over the unreliable in-match channel.
    /// Proves ICE + DTLS + SCTP + DCEP all complete and that the second
    /// (unreliable) channel opens and carries traffic both ways.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_signaling_round_trips() {
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
    /// pinned port after the old peer connection's driver releases it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rebuild_after_drop_round_trips() {
        let port = 24988;
        let addr = format!("127.0.0.1:{port}");

        // Bring up the link, handshake, and prove it carries a datagram.
        // The port-rebind race (the old driver task releases the socket
        // asynchronously) is retried, same as the real reconnect coordinator.
        let bring_up = |attempts: usize| {
            let addr = addr.clone();
            async move {
                for attempt in 1..=attempts {
                    let (host_res, conn_res) = tokio::join!(host(port), connect(&addr));
                    let (mut host_ch, mut conn_ch) = match (host_res, conn_res) {
                        (Ok(h), Ok(c)) => (h, c),
                        r => {
                            assert!(attempt < attempts, "direct setup kept failing: {:?}", r.0.err());
                            continue;
                        }
                    };
                    let handshake = async {
                        tokio::try_join!(
                            crate::net::negotiate(&mut host_ch.control.0, &mut host_ch.control.1),
                            crate::net::negotiate(&mut conn_ch.control.0, &mut conn_ch.control.1),
                        )
                    };
                    match tokio::time::timeout(std::time::Duration::from_secs(15), handshake).await {
                        Ok(Ok(_)) => return (host_ch, conn_ch),
                        r => assert!(attempt < attempts, "handshake kept failing: {:?}", r.is_err()),
                    }
                }
                unreachable!()
            }
        };

        let (host_ch, conn_ch) = bring_up(1).await;
        // Simulate the drop: tearing down both peer connections releases the
        // host's pinned UDP port (asynchronously, in the driver task).
        drop(host_ch);
        drop(conn_ch);

        // Rebuild from the identical recipe.
        let (mut host_ch, mut conn_ch) = bring_up(3).await;

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

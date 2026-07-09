//! Packet-loss behavior measurements: two real peers connected through a
//! loss-injecting UDP proxy, reporting delivery/latency statistics per
//! channel. `#[ignore]`d — these are diagnostics to run by hand
//! (`cargo test -p tango-rtc --release --test loss -- --ignored --nocapture`),
//! not pass/fail CI gates (the pass criteria would be inherently flaky).

use std::net::SocketAddr;
use std::sync::Arc;

use tango_rtc::{ChannelConfig, DataChannel, DirectRole, PeerConnection, RtcConfig};

fn channel_configs() -> Vec<ChannelConfig> {
    vec![
        ChannelConfig {
            label: "ctl".to_owned(),
            ordered: true,
            reliable: true,
        },
        ChannelConfig {
            label: "dat".to_owned(),
            ordered: false,
            reliable: false,
        },
    ]
}

/// xorshift64* — deterministic loss decisions without a rand dependency.
struct Rng(u64);

impl Rng {
    fn chance(&mut self, permille: u32) -> bool {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 32) as u32 % 1000 < permille
    }
}

/// Bidirectional UDP forwarder that drops `loss_permille` of datagrams and
/// delays the survivors by `delay` in each direction (so RTT = 2 × `delay`).
/// The dialer talks to `listen`; the proxy relays to `forward_to` (the host)
/// from a second socket, so the host sees the proxy as its peer.
async fn lossy_proxy(
    listen: Arc<tokio::net::UdpSocket>,
    forward_to: SocketAddr,
    loss_permille: u32,
    delay: std::time::Duration,
    seed: u64,
) {
    let host_side = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let mut dialer: Option<SocketAddr> = None;
    let mut rng = Rng(seed | 1);
    let mut buf_a = vec![0u8; 2048];
    let mut buf_b = vec![0u8; 2048];
    // Per-datagram delay tasks; tokio's timer wheel keeps per-sleep cost
    // negligible at these rates, and independent sleeps model a fixed-latency
    // (non-queuing) path.
    let deliver = |sock: Arc<tokio::net::UdpSocket>, payload: Vec<u8>, to: SocketAddr| async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let _ = sock.send_to(&payload, to).await;
    };
    loop {
        tokio::select! {
            r = listen.recv_from(&mut buf_a) => {
                let Ok((n, from)) = r else { break };
                dialer = Some(from);
                if !rng.chance(loss_permille) {
                    tokio::spawn(deliver(host_side.clone(), buf_a[..n].to_vec(), forward_to));
                }
            }
            r = host_side.recv_from(&mut buf_b) => {
                let Ok((n, _)) = r else { break };
                if let Some(dialer) = dialer {
                    if !rng.chance(loss_permille) {
                        tokio::spawn(deliver(listen.clone(), buf_b[..n].to_vec(), dialer));
                    }
                }
            }
        }
    }
}

/// Bring up a direct pair through a lossy proxy. Returns (host chans, dialer
/// chans, keep-alives).
async fn connect_through_proxy(
    host_port: u16,
    loss_permille: u32,
    delay: std::time::Duration,
    seed: u64,
) -> ((PeerConnection, Vec<DataChannel>), (PeerConnection, Vec<DataChannel>)) {
    let proxy_sock = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let proxy_addr = proxy_sock.local_addr().unwrap();
    tokio::spawn(lossy_proxy(
        proxy_sock,
        format!("127.0.0.1:{}", host_port).parse().unwrap(),
        loss_permille,
        delay,
        seed,
    ));

    let (host, host_chans, _he) =
        PeerConnection::new_direct(RtcConfig::default(), &channel_configs(), DirectRole::Host { port: host_port })
            .unwrap();
    let (dialer, dialer_chans, _de) = PeerConnection::new_direct(
        RtcConfig::default(),
        &channel_configs(),
        DirectRole::Connect { remote: proxy_addr },
    )
    .unwrap();

    ((host, host_chans), (dialer, dialer_chans))
}

struct Stats {
    sent: usize,
    received: usize,
    received_seqs: Vec<u32>,
    latencies_ms: Vec<f64>,
}

impl Stats {
    fn report(&mut self, label: &str, wall: std::time::Duration) {
        self.latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct = |p: f64| -> f64 {
            if self.latencies_ms.is_empty() {
                return f64::NAN;
            }
            self.latencies_ms[((self.latencies_ms.len() - 1) as f64 * p) as usize]
        };
        println!(
            "{label}: {}/{} delivered ({:.1}%), latency p50={:.1}ms p95={:.1}ms p99={:.1}ms max={:.1}ms, wall={:?}",
            self.received,
            self.sent,
            100.0 * self.received as f64 / self.sent as f64,
            pct(0.50),
            pct(0.95),
            pct(0.99),
            pct(1.0),
            wall,
        );
        // Missing-seq pattern: random singles vs episodic runs tell very
        // different stories about where frames die.
        let mut have = vec![false; self.sent];
        for s in &self.received_seqs {
            if (*s as usize) < have.len() {
                have[*s as usize] = true;
            }
        }
        let missing: Vec<usize> = (0..self.sent).filter(|i| !have[*i]).collect();
        if !missing.is_empty() {
            let mut runs: Vec<(usize, usize)> = vec![];
            for &m in &missing {
                match runs.last_mut() {
                    Some((_, end)) if *end + 1 == m => *end = m,
                    _ => runs.push((m, m)),
                }
            }
            let shown: Vec<String> = runs
                .iter()
                .take(30)
                .map(|(a, b)| if a == b { format!("{}", a) } else { format!("{}-{}", a, b) })
                .collect();
            println!("  missing {} in {} runs: {}{}", missing.len(), runs.len(), shown.join(","), if runs.len() > 30 { ",…" } else { "" });
        }
    }
}

/// Send `count` timestamped messages at `interval` on `tx`, receive on `rx`
/// until `drain` after the last send, and collect latency stats.
async fn pump(
    tx: &mut DataChannel,
    rx: &mut DataChannel,
    count: u32,
    interval: std::time::Duration,
    drain: std::time::Duration,
    pad_to: usize,
) -> (Stats, std::time::Duration) {
    let epoch = std::time::Instant::now();
    let mut stats = Stats {
        sent: 0,
        received: 0,
        received_seqs: vec![],
        latencies_ms: vec![],
    };

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
    let mut sent = 0u32;
    let deadline_after_last = tokio::time::sleep(std::time::Duration::from_secs(3600));
    tokio::pin!(deadline_after_last);

    loop {
        tokio::select! {
            _ = ticker.tick(), if sent < count => {
                let mut msg = vec![0u8; pad_to.max(12)];
                msg[..4].copy_from_slice(&sent.to_le_bytes());
                msg[4..12].copy_from_slice(&(epoch.elapsed().as_micros() as u64).to_le_bytes());
                tx.send(&msg).await.expect("send");
                stats.sent += 1;
                sent += 1;
                if sent == count {
                    deadline_after_last.as_mut().reset(tokio::time::Instant::now() + drain);
                }
            }
            got = rx.receive() => {
                let got = got.expect("receive EOF");
                let seq = u32::from_le_bytes(got[..4].try_into().unwrap());
                let sent_us = u64::from_le_bytes(got[4..12].try_into().unwrap());
                let latency_us = epoch.elapsed().as_micros() as u64 - sent_us;
                stats.received += 1;
                stats.received_seqs.push(seq);
                stats.latencies_ms.push(latency_us as f64 / 1000.0);
                if stats.received == stats.sent && sent == count {
                    break;
                }
            }
            _ = &mut deadline_after_last => break,
        }
    }
    (stats, epoch.elapsed())
}

async fn run_scenario(host_port: u16, loss_permille: u32, delay: std::time::Duration, seed: u64) {
    println!(
        "=== {}‰ loss, {:?} one-way delay (seed {:#x}) ===",
        loss_permille, delay, seed
    );
    let ((_host, mut host_chans), (_dialer, mut dialer_chans)) =
        connect_through_proxy(host_port, loss_permille, delay, seed).await;

    // Warm the connection up (first send drives the whole bring-up).
    host_chans[0].send(b"warmup-h").await.unwrap();
    dialer_chans[0].send(b"warmup-d").await.unwrap();
    assert!(dialer_chans[0].receive().await.is_some());
    assert!(host_chans[0].receive().await.is_some());

    let mut it = dialer_chans.into_iter();
    let (mut d_ctl, mut d_dat) = (it.next().unwrap(), it.next().unwrap());
    let mut it = host_chans.into_iter();
    let (mut h_ctl, mut h_dat) = (it.next().unwrap(), it.next().unwrap());

    // Unreliable at 60Hz for 6s, 80-byte frames — the in-match shape. Loss
    // here is expected (no retransmits); what must NOT happen is delivered
    // latency growing or spiking (a sender-side stall / bufferbloat).
    let (mut stats, wall) = pump(
        &mut d_dat,
        &mut h_dat,
        360,
        std::time::Duration::from_millis(16),
        std::time::Duration::from_secs(2),
        80,
    )
    .await;
    stats.report("unreliable 60Hz", wall);

    // Reliable at 20Hz, 100 messages — the control-channel shape. Everything
    // must arrive; latency spikes show the retransmit behavior (RTO vs fast
    // retransmit).
    let (mut stats, wall) = pump(
        &mut d_ctl,
        &mut h_ctl,
        100,
        std::time::Duration::from_millis(50),
        std::time::Duration::from_secs(20),
        80,
    )
    .await;
    stats.report("reliable 20Hz  ", wall);

    // Reliable, low-rate: 20 messages at 1/s — the lobby/ping shape. Too
    // sparse for fast retransmit (no following traffic to generate dup
    // SACKs), so every loss is recovered by RTO — this is where RTO_MIN
    // shows up directly.
    let (mut stats, wall) = pump(
        &mut d_ctl,
        &mut h_ctl,
        20,
        std::time::Duration::from_millis(1000),
        std::time::Duration::from_secs(20),
        80,
    )
    .await;
    stats.report("reliable 1Hz   ", wall);
}

#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 4))]
#[ignore = "diagnostic measurement, run by hand with --ignored --nocapture"]
async fn loss_0_baseline() {
    run_scenario(28961, 0, std::time::Duration::ZERO, 0x5EED).await;
}

#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 4))]
#[ignore = "diagnostic measurement, run by hand with --ignored --nocapture"]
async fn loss_5pct() {
    for (i, seed) in [0x5EED, 0xACE, 0xBEEF].into_iter().enumerate() {
        run_scenario(28910 + i as u16, 50, std::time::Duration::ZERO, seed).await;
    }
}

#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 4))]
#[ignore = "diagnostic measurement, run by hand with --ignored --nocapture"]
async fn loss_15pct() {
    for (i, seed) in [0x5EED, 0xACE, 0xBEEF].into_iter().enumerate() {
        run_scenario(28920 + i as u16, 150, std::time::Duration::ZERO, seed).await;
    }
}

/// The report that matters for in-match feel: with a real RTT in the path,
/// does delivered-frame latency stay pinned at the transport delay, or does
/// it wobble upward under loss? (On a zero-RTT loopback everything the
/// congestion machinery does is invisible — SACKs return instantly.)
#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 4))]
#[ignore = "diagnostic measurement, run by hand with --ignored --nocapture"]
async fn wobble_rtt60() {
    let delay = std::time::Duration::from_millis(30);
    for (i, loss) in [0u32, 50, 150].into_iter().enumerate() {
        for (j, seed) in [0x5EEDu64, 0xACE].into_iter().enumerate() {
            run_scenario(28930 + (i * 2 + j) as u16, loss, delay, seed).await;
        }
    }
}

//! End-to-end loopback tests: two real peer connections over real UDP
//! sockets on this machine, exercising ICE, DTLS, SCTP, DCEP and both
//! bring-up paths (trickled signaling exchange and the signaling-free direct
//! link).

use tango_rtc::{ChannelConfig, DataChannel, DirectRole, PeerConnection, PeerConnectionEvent, RtcConfig, SdpType};

const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The same two-channel shape Tango uses: a reliable ordered control channel
/// and an unreliable unordered in-match channel.
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

fn loopback_config() -> RtcConfig {
    RtcConfig {
        include_loopback: true,
        ..Default::default()
    }
}

/// Bring up a signaling-path pair: both sides construct as offerers (so the
/// exchange exercises the glare rollback on the answering side), the SDPs are
/// swapped directly, and candidates trickle through the event streams until
/// both report Connected.
async fn connect_signaled_pair() -> (
    (PeerConnection, Vec<DataChannel>),
    (PeerConnection, Vec<DataChannel>),
) {
    let (mut a, a_chans, mut a_events) = PeerConnection::new(loopback_config(), &channel_configs()).unwrap();
    let (mut b, b_chans, mut b_events) = PeerConnection::new(loopback_config(), &channel_configs()).unwrap();

    // The offer is available synchronously (trickle ICE), before any
    // candidates exist.
    let offer = a.local_description().unwrap();
    assert_eq!(offer.sdp_type, SdpType::Offer);

    // b also holds its own offer; accepting a's rolls it back and produces
    // the answer.
    b.set_remote_description(offer).unwrap();
    let answer = b.local_description().unwrap();
    assert_eq!(answer.sdp_type, SdpType::Answer);
    a.set_remote_description(answer).unwrap();

    // Trickle candidates across and wait for both sides to connect.
    let mut a_connected = false;
    let mut b_connected = false;
    while !(a_connected && b_connected) {
        tokio::select! {
            ev = a_events.recv() => match ev.expect("a's event stream ended early") {
                PeerConnectionEvent::IceCandidate(c) => b.add_remote_candidate(&c).unwrap(),
                PeerConnectionEvent::ConnectionStateChange(tango_rtc::ConnectionState::Connected) => {
                    a_connected = true;
                }
                PeerConnectionEvent::ConnectionStateChange(s) => {
                    assert!(
                        !matches!(s, tango_rtc::ConnectionState::Failed | tango_rtc::ConnectionState::Closed),
                        "a hit terminal state {:?} before connecting",
                        s
                    );
                }
            },
            ev = b_events.recv() => match ev.expect("b's event stream ended early") {
                PeerConnectionEvent::IceCandidate(c) => a.add_remote_candidate(&c).unwrap(),
                PeerConnectionEvent::ConnectionStateChange(tango_rtc::ConnectionState::Connected) => {
                    b_connected = true;
                }
                PeerConnectionEvent::ConnectionStateChange(s) => {
                    assert!(
                        !matches!(s, tango_rtc::ConnectionState::Failed | tango_rtc::ConnectionState::Closed),
                        "b hit terminal state {:?} before connecting",
                        s
                    );
                }
            },
        }
    }

    ((a, a_chans), (b, b_chans))
}

/// Round-trip one message each way on every channel of a connected pair.
async fn round_trip(a_chans: &mut [DataChannel], b_chans: &mut [DataChannel], tag: &str) {
    for (idx, (a_chan, b_chan)) in a_chans.iter_mut().zip(b_chans.iter_mut()).enumerate() {
        let a_msg = format!("{}: a->b on {}", tag, idx).into_bytes();
        let b_msg = format!("{}: b->a on {}", tag, idx).into_bytes();
        a_chan.send(&a_msg).await.expect("a send");
        b_chan.send(&b_msg).await.expect("b send");
        assert_eq!(b_chan.receive().await.expect("b receive"), a_msg);
        assert_eq!(a_chan.receive().await.expect("a receive"), b_msg);
    }
}

/// Signaling path: trickled exchange brings up both channels; data flows both
/// ways on each; the DTLS fingerprints in the SDPs are real (verification is
/// on for this path).
#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn signaled_pair_round_trips() {
    let work = async {
        let ((a, mut a_chans), (b, mut b_chans)) = connect_signaled_pair().await;
        round_trip(&mut a_chans, &mut b_chans, "signaled").await;

        // Both descriptions carry a parseable SHA-256 DTLS fingerprint —
        // tango's signaling client derives the mid-match reconnect
        // rendezvous id from these lines, so their presence is contract.
        for desc in [
            a.local_description().unwrap(),
            a.remote_description().unwrap(),
            b.local_description().unwrap(),
            b.remote_description().unwrap(),
        ] {
            assert!(
                desc.sdp
                    .lines()
                    .any(|l| l.trim().to_ascii_lowercase().starts_with("a=fingerprint:sha-256 ")),
                "no sha-256 fingerprint in {:?} SDP:\n{}",
                desc.sdp_type,
                desc.sdp
            );
        }
    };
    tokio::time::timeout(TEST_TIMEOUT, work).await.expect("test timed out");
}

/// Dropping one side's connection reaches the other as a prompt EOF (DTLS
/// close_notify → `Event::Closed`), not a multi-second disconnect-grace
/// timeout.
#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn hangup_is_prompt_eof() {
    let work = async {
        let ((a, mut a_chans), (_b, mut b_chans)) = connect_signaled_pair().await;
        round_trip(&mut a_chans, &mut b_chans, "pre-hangup").await;

        let hangup_at = std::time::Instant::now();
        drop(a_chans);
        drop(a);

        // The peer sees EOF on its channels well inside the 10s disconnect
        // grace it would otherwise have to sit out.
        let got = b_chans[0].receive().await;
        assert!(got.is_none(), "expected EOF after peer hangup, got {:?}", got);
        assert!(
            hangup_at.elapsed() < std::time::Duration::from_secs(5),
            "EOF took {:?} — close_notify didn't get through, this was the disconnect grace",
            hangup_at.elapsed()
        );
    };
    tokio::time::timeout(TEST_TIMEOUT, work).await.expect("test timed out");
}

/// Bring up a direct (signaling-free) pair on loopback.
async fn connect_direct_pair(port: u16) -> ((PeerConnection, Vec<DataChannel>), (PeerConnection, Vec<DataChannel>)) {
    let (host, host_chans, _host_events) = PeerConnection::new_direct(
        RtcConfig::default(),
        &channel_configs(),
        DirectRole::Host { port },
    )
    .unwrap();
    let (dialer, dialer_chans, _dialer_events) = PeerConnection::new_direct(
        RtcConfig::default(),
        &channel_configs(),
        DirectRole::Connect {
            remote: format!("127.0.0.1:{}", port).parse().unwrap(),
        },
    )
    .unwrap();
    ((host, host_chans), (dialer, dialer_chans))
}

/// Direct path: no signaling exchange at all — fixed ICE creds, fingerprint
/// verification off — and both channels still come up and carry traffic. The
/// first `send` blocks until the channel opens, so this drives the whole
/// ICE/DTLS/SCTP/DCEP bring-up.
#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn direct_round_trips() {
    let work = async {
        let ((_host, mut host_chans), (_dialer, mut dialer_chans)) = connect_direct_pair(28871).await;
        round_trip(&mut host_chans, &mut dialer_chans, "direct").await;
    };
    tokio::time::timeout(TEST_TIMEOUT, work).await.expect("test timed out");
}

/// The transparent-reconnect primitive: after a live direct link is torn
/// down, both peers rebuild from the same recipe — the host re-pins the same
/// UDP port (which the teardown must have released), the dialer re-dials —
/// and traffic flows again.
#[test_log::test(tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn direct_rebuilds_after_drop() {
    let port = 28872;
    let work = async {
        let (host_side, dialer_side) = connect_direct_pair(port).await;
        {
            let (_host, mut host_chans) = host_side;
            let (_dialer, mut dialer_chans) = dialer_side;
            round_trip(&mut host_chans, &mut dialer_chans, "first").await;
            // Both ends drop here.
        }

        // The rebuild races the port release; retry briefly like the real
        // reconnect coordinator does.
        let mut attempt = 0;
        loop {
            attempt += 1;
            let ((_host, mut host_chans), (_dialer, mut dialer_chans)) = connect_direct_pair(port).await;
            let ok = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                round_trip(&mut host_chans, &mut dialer_chans, "rebuilt"),
            )
            .await;
            match ok {
                Ok(()) => break,
                Err(_) if attempt < 3 => continue,
                Err(_) => panic!("rebuilt link never carried traffic"),
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), work)
        .await
        .expect("test timed out");
}

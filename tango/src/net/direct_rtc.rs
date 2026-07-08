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

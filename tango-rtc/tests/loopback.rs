//! End-to-end test of two peer connections meeting over loopback,
//! exercising the same flow tango-signaling drives: both sides gather and
//! produce offers, one side (the "polite" one) receives the other's offer
//! — implicitly rolling back its own — and answers.

async fn make_peer() -> (
    tango_rtc::PeerConnection,
    tango_rtc::DataChannel,
    tokio::sync::mpsc::Receiver<tango_rtc::PeerConnectionEvent>,
) {
    let (mut peer_conn, mut event_rx) = tango_rtc::PeerConnection::new(tango_rtc::RtcConfig {
        include_loopback: true,
        ..Default::default()
    })
    .unwrap();

    let dc = peer_conn.create_data_channel("tango").unwrap();

    loop {
        if let Some(tango_rtc::PeerConnectionEvent::GatheringStateChange(tango_rtc::GatheringState::Complete)) =
            event_rx.recv().await
        {
            break;
        }
    }

    (peer_conn, dc, event_rx)
}

async fn wait_connected(event_rx: &mut tokio::sync::mpsc::Receiver<tango_rtc::PeerConnectionEvent>) {
    loop {
        match event_rx.recv().await.unwrap() {
            tango_rtc::PeerConnectionEvent::ConnectionStateChange(tango_rtc::ConnectionState::Connected) => break,
            tango_rtc::PeerConnectionEvent::ConnectionStateChange(
                state @ (tango_rtc::ConnectionState::Failed
                | tango_rtc::ConnectionState::Closed
                | tango_rtc::ConnectionState::Disconnected),
            ) => panic!("connection reached {:?} before Connected", state),
            _ => {}
        }
    }
}

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn test_loopback_connection() {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let (mut impolite, mut dc_a, mut events_a) = make_peer().await;
        let (mut polite, mut dc_b, mut events_b) = make_peer().await;

        let offer = impolite.local_description().unwrap();
        assert!(matches!(offer.sdp_type, tango_rtc::SdpType::Offer));
        eprintln!("=== OFFER ===\n{}", offer.sdp);

        // The polite side had its own pending offer; receiving the remote
        // offer rolls it back and produces an answer.
        assert!(matches!(
            polite.local_description().unwrap().sdp_type,
            tango_rtc::SdpType::Offer
        ));
        polite.set_remote_description(offer).unwrap();
        let answer = polite.local_description().unwrap();
        assert!(matches!(answer.sdp_type, tango_rtc::SdpType::Answer));
        eprintln!("=== ANSWER ===\n{}", answer.sdp);

        impolite.set_remote_description(answer).unwrap();

        wait_connected(&mut events_a).await;
        wait_connected(&mut events_b).await;

        // Data flows both ways.
        dc_a.send(b"hello from a").await.unwrap();
        assert_eq!(dc_b.receive().await.unwrap(), b"hello from a");
        dc_b.send(b"hello from b").await.unwrap();
        assert_eq!(dc_a.receive().await.unwrap(), b"hello from b");

        // Round-trip latency guard: a write must hit the wire in the same
        // driver pass. (A regression that leaves writes queued until the
        // next unrelated wakeup — e.g. an ICE timer — turns these 20
        // sequential round trips into many seconds, and shows up in the
        // app as wildly unstable lobby ping.)
        let start = std::time::Instant::now();
        for i in 0..20u32 {
            dc_a.send(&i.to_le_bytes()).await.unwrap();
            let echoed = dc_b.receive().await.unwrap();
            dc_b.send(&echoed).await.unwrap();
            assert_eq!(dc_a.receive().await.unwrap(), i.to_le_bytes());
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "20 round trips took {:?}",
            elapsed
        );

        // A bigger volume of messages in both directions at once.
        let a_to_b = async {
            for i in 0..1000u32 {
                dc_a.send(&i.to_le_bytes()).await.unwrap();
            }
            for i in 0..1000u32 {
                assert_eq!(dc_a.receive().await.unwrap(), (!i).to_le_bytes());
            }
        };
        let b_to_a = async {
            for i in 0..1000u32 {
                dc_b.send(&(!i).to_le_bytes()).await.unwrap();
            }
            for i in 0..1000u32 {
                assert_eq!(dc_b.receive().await.unwrap(), i.to_le_bytes());
            }
        };
        tokio::join!(a_to_b, b_to_a);

        // Both sides should agree they're talking directly.
        let (local, remote) = impolite.selected_candidate_pair().unwrap();
        assert!(!local.contains("typ relay"), "local: {}", local);
        assert!(!remote.contains("typ relay"), "remote: {}", remote);

        // Dropping one peer's connection promptly EOFs the other side.
        drop(impolite);
        drop(dc_a);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), dc_b.receive())
                .await
                .expect("EOF should arrive promptly, not via disconnect grace"),
            None
        );
    })
    .await
    .unwrap();
}

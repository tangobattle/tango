//! End-to-end test of two peer connections meeting over loopback,
//! exercising the same flow tango-signaling drives: both sides gather and
//! produce offers, one side (the "polite" one) receives the other's offer
//! — implicitly rolling back its own — and answers.

async fn make_peer() -> (
    tango_rtc::DataChannel,
    tokio::sync::mpsc::Receiver<tango_rtc::ConnectionEvent>,
) {
    let (dc, mut event_rx) = tango_rtc::DataChannel::new(
        tango_rtc::RtcConfig {
            include_loopback: true,
            ..Default::default()
        },
        "tango",
    )
    .unwrap();

    loop {
        if let Some(tango_rtc::ConnectionEvent::GatheringStateChange(tango_rtc::GatheringState::Complete)) =
            event_rx.recv().await
        {
            break;
        }
    }

    (dc, event_rx)
}

async fn wait_connected(event_rx: &mut tokio::sync::mpsc::Receiver<tango_rtc::ConnectionEvent>) {
    loop {
        match event_rx.recv().await.unwrap() {
            tango_rtc::ConnectionEvent::ConnectionStateChange(tango_rtc::ConnectionState::Connected) => break,
            tango_rtc::ConnectionEvent::ConnectionStateChange(
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
        let (mut dc_a, mut events_a) = make_peer().await;
        let (mut dc_b, mut events_b) = make_peer().await;

        let offer = dc_a.local_description().unwrap();
        assert!(matches!(offer.sdp_type, tango_rtc::SdpType::Offer));
        eprintln!("=== OFFER ===\n{}", offer.sdp);

        // The polite side had its own pending offer; receiving the remote
        // offer rolls it back and produces an answer.
        assert!(matches!(
            dc_b.local_description().unwrap().sdp_type,
            tango_rtc::SdpType::Offer
        ));
        dc_b.set_remote_description(offer).unwrap();
        let answer = dc_b.local_description().unwrap();
        assert!(matches!(answer.sdp_type, tango_rtc::SdpType::Answer));
        eprintln!("=== ANSWER ===\n{}", answer.sdp);

        dc_a.set_remote_description(answer).unwrap();

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

        // A bigger volume of messages in both directions at once. Split so
        // each side can send and receive concurrently.
        let (mut tx_a, mut rx_a) = dc_a.split();
        let (mut tx_b, mut rx_b) = dc_b.split();
        let a_to_b = async {
            for i in 0..1000u32 {
                tx_a.send(&i.to_le_bytes()).await.unwrap();
            }
            for i in 0..1000u32 {
                assert_eq!(rx_a.receive().await.unwrap(), (!i).to_le_bytes());
            }
        };
        let b_to_a = async {
            for i in 0..1000u32 {
                tx_b.send(&(!i).to_le_bytes()).await.unwrap();
            }
            for i in 0..1000u32 {
                assert_eq!(rx_b.receive().await.unwrap(), i.to_le_bytes());
            }
        };
        tokio::join!(a_to_b, b_to_a);

        let dc_a = tx_a.unsplit(rx_a);
        let mut dc_b = tx_b.unsplit(rx_b);

        // Both sides should agree they're talking directly.
        let (local, remote) = dc_a.selected_candidate_pair().unwrap();
        assert!(!local.contains("typ relay"), "local: {}", local);
        assert!(!remote.contains("typ relay"), "remote: {}", remote);

        // Dropping one side (both halves at once) promptly EOFs the other.
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

/// The hangup must reach the remote even on process exit, where the tokio
/// runtime is torn down by dropping its tasks without polling them again —
/// so the driver task's own graceful close never runs, and only the
/// transport's (synchronous) `Drop` stands between the remote and a full
/// disconnect-grace wait.
#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn test_hangup_survives_runtime_teardown() {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        // Peer A gets its own runtime so it can be torn down mid-test the
        // way process exit does it; peer B lives on the test's runtime.
        let rt_a = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let (mut dc_a, mut events_a) = rt_a.spawn(make_peer()).await.unwrap();
        let (mut dc_b, mut events_b) = make_peer().await;

        dc_b.set_remote_description(dc_a.local_description().unwrap()).unwrap();
        dc_a.set_remote_description(dc_b.local_description().unwrap()).unwrap();

        wait_connected(&mut events_a).await;
        wait_connected(&mut events_b).await;

        dc_a.send(b"ping").await.unwrap();
        assert_eq!(dc_b.receive().await.unwrap(), b"ping");

        // "Process exit": A's driver task is dropped, never to be polled
        // again, and only then do A's locals drop.
        rt_a.shutdown_background();
        drop(dc_a);
        drop(events_a);

        // B still hears the hangup promptly.
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

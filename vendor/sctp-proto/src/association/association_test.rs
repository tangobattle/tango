use crate::chunk::{Chunk, chunk_init::ChunkInit};
use crate::config::generate_snap_token;

use super::*;

const ACCEPT_CH_SIZE: usize = 16;

fn create_association(config: TransportConfig) -> Association {
    Association::new(
        None,
        Arc::new(config),
        1400,
        0,
        SocketAddr::from_str("0.0.0.0:0").unwrap(),
        None,
        Instant::now(),
    )
}

#[test]
fn test_assoc_is_closing() {
    let closing_states = [
        AssociationState::ShutdownSent,
        AssociationState::ShutdownAckSent,
        AssociationState::ShutdownPending,
        AssociationState::ShutdownReceived,
    ];

    for state in [
        AssociationState::Closed,
        AssociationState::CookieWait,
        AssociationState::CookieEchoed,
        AssociationState::Established,
    ] {
        let a = Association {
            state,
            ..Default::default()
        };

        assert!(!a.is_closing(), "{state} should not be closing");
    }

    for state in closing_states {
        let a = Association {
            state,
            ..Default::default()
        };

        assert!(a.is_closing(), "{state} should be closing");
        assert!(!a.is_closed(), "{state} should not be closed");
    }
}

#[test]
fn test_create_forward_tsn_forward_one_abandoned() -> Result<()> {
    let mut a = Association {
        cumulative_tsn_ack_point: 9,
        advanced_peer_tsn_ack_point: 10,
        ..Default::default()
    };

    a.inflight_queue.push_no_check(ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: 10,
        stream_identifier: 1,
        stream_sequence_number: 2,
        user_data: Bytes::from_static(b"ABC"),
        nsent: 1,
        abandoned: true,
        ..Default::default()
    });

    let fwdtsn = a.create_forward_tsn();

    assert_eq!(10, fwdtsn.new_cumulative_tsn, "should be able to serialize");
    assert_eq!(1, fwdtsn.streams.len(), "there should be one stream");
    assert_eq!(1, fwdtsn.streams[0].identifier, "si should be 1");
    assert_eq!(2, fwdtsn.streams[0].sequence, "ssn should be 2");

    Ok(())
}

#[test]
fn test_create_forward_tsn_forward_two_abandoned_with_the_same_si() -> Result<()> {
    let mut a = Association {
        cumulative_tsn_ack_point: 9,
        advanced_peer_tsn_ack_point: 12,
        ..Default::default()
    };

    a.inflight_queue.push_no_check(ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: 10,
        stream_identifier: 1,
        stream_sequence_number: 2,
        user_data: Bytes::from_static(b"ABC"),
        nsent: 1,
        abandoned: true,
        ..Default::default()
    });
    a.inflight_queue.push_no_check(ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: 11,
        stream_identifier: 1,
        stream_sequence_number: 3,
        user_data: Bytes::from_static(b"DEF"),
        nsent: 1,
        abandoned: true,
        ..Default::default()
    });
    a.inflight_queue.push_no_check(ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: 12,
        stream_identifier: 2,
        stream_sequence_number: 1,
        user_data: Bytes::from_static(b"123"),
        nsent: 1,
        abandoned: true,
        ..Default::default()
    });

    let fwdtsn = a.create_forward_tsn();

    assert_eq!(12, fwdtsn.new_cumulative_tsn, "should be able to serialize");
    assert_eq!(2, fwdtsn.streams.len(), "there should be two stream");

    let mut si1ok = false;
    let mut si2ok = false;
    for s in &fwdtsn.streams {
        match s.identifier {
            1 => {
                assert_eq!(3, s.sequence, "ssn should be 3");
                si1ok = true;
            }
            2 => {
                assert_eq!(1, s.sequence, "ssn should be 1");
                si2ok = true;
            }
            _ => panic!("unexpected stream indentifier"),
        }
    }
    assert!(si1ok, "si=1 should be present");
    assert!(si2ok, "si=2 should be present");

    Ok(())
}

#[test]
fn test_handle_forward_tsn_forward_3unreceived_chunks() -> Result<()> {
    let mut a = Association {
        use_forward_tsn: true,
        ..Default::default()
    };

    let prev_tsn = a.peer_last_tsn;

    let fwdtsn = ChunkForwardTsn {
        new_cumulative_tsn: a.peer_last_tsn + 3,
        streams: vec![ChunkForwardTsnStream {
            identifier: 0,
            sequence: 0,
        }],
    };

    let p = a.handle_forward_tsn(&fwdtsn)?;

    let delayed_ack_triggered = a.delayed_ack_triggered;
    let immediate_ack_triggered = a.immediate_ack_triggered;
    assert_eq!(
        a.peer_last_tsn,
        prev_tsn + 3,
        "peerLastTSN should advance by 3 "
    );
    assert!(delayed_ack_triggered, "delayed sack should be triggered");
    assert!(
        !immediate_ack_triggered,
        "immediate sack should NOT be triggered"
    );
    assert!(p.is_empty(), "should return empty");

    Ok(())
}

#[test]
fn test_handle_forward_tsn_forward_1for1_missing() -> Result<()> {
    let mut a = Association {
        use_forward_tsn: true,
        ..Default::default()
    };

    let prev_tsn = a.peer_last_tsn;

    // this chunk is blocked by the missing chunk at tsn=1
    a.payload_queue.push(
        ChunkPayloadData {
            beginning_fragment: true,
            ending_fragment: true,
            tsn: a.peer_last_tsn + 2,
            stream_identifier: 0,
            stream_sequence_number: 1,
            user_data: Bytes::from_static(b"ABC"),
            ..Default::default()
        },
        a.peer_last_tsn,
    );

    let fwdtsn = ChunkForwardTsn {
        new_cumulative_tsn: a.peer_last_tsn + 1,
        streams: vec![ChunkForwardTsnStream {
            identifier: 0,
            sequence: 1,
        }],
    };

    let p = a.handle_forward_tsn(&fwdtsn)?;

    let delayed_ack_triggered = a.delayed_ack_triggered;
    let immediate_ack_triggered = a.immediate_ack_triggered;
    assert_eq!(
        a.peer_last_tsn,
        prev_tsn + 2,
        "peerLastTSN should advance by 2"
    );
    assert!(delayed_ack_triggered, "delayed sack should be triggered");
    assert!(
        !immediate_ack_triggered,
        "immediate sack should NOT be triggered"
    );
    assert!(p.is_empty(), "should return empty");

    Ok(())
}

#[test]
fn test_handle_forward_tsn_forward_1for2_missing() -> Result<()> {
    let mut a = Association {
        use_forward_tsn: true,
        ..Default::default()
    };

    a.use_forward_tsn = true;
    let prev_tsn = a.peer_last_tsn;

    // this chunk is blocked by the missing chunk at tsn=1
    a.payload_queue.push(
        ChunkPayloadData {
            beginning_fragment: true,
            ending_fragment: true,
            tsn: a.peer_last_tsn + 3,
            stream_identifier: 0,
            stream_sequence_number: 1,
            user_data: Bytes::from_static(b"ABC"),
            ..Default::default()
        },
        a.peer_last_tsn,
    );

    let fwdtsn = ChunkForwardTsn {
        new_cumulative_tsn: a.peer_last_tsn + 1,
        streams: vec![ChunkForwardTsnStream {
            identifier: 0,
            sequence: 1,
        }],
    };

    let p = a.handle_forward_tsn(&fwdtsn)?;

    let immediate_ack_triggered = a.immediate_ack_triggered;
    assert_eq!(
        a.peer_last_tsn,
        prev_tsn + 1,
        "peerLastTSN should advance by 1"
    );
    assert!(
        immediate_ack_triggered,
        "immediate sack should be triggered"
    );
    assert!(p.is_empty(), "should return empty");

    Ok(())
}

#[test]
fn test_handle_forward_tsn_dup_forward_tsn_chunk_should_generate_sack() -> Result<()> {
    let mut a = Association {
        use_forward_tsn: true,
        ..Default::default()
    };

    let prev_tsn = a.peer_last_tsn;

    let fwdtsn = ChunkForwardTsn {
        new_cumulative_tsn: a.peer_last_tsn,
        streams: vec![ChunkForwardTsnStream {
            identifier: 0,
            sequence: 1,
        }],
    };

    let p = a.handle_forward_tsn(&fwdtsn)?;

    let ack_state = a.ack_state;
    assert_eq!(a.peer_last_tsn, prev_tsn, "peerLastTSN should not advance");
    assert_eq!(AckState::Immediate, ack_state, "sack should be requested");
    assert!(p.is_empty(), "should return empty");

    Ok(())
}

#[test]
fn test_assoc_create_new_stream() -> Result<()> {
    let mut a = Association::default();

    for i in 0..ACCEPT_CH_SIZE {
        let stream_identifier =
            if let Some(s) = a.create_stream(i as u16, true, PayloadProtocolIdentifier::Unknown) {
                s.stream_identifier
            } else {
                panic!("{} should success", i);
            };
        let result = a.streams.get(&stream_identifier);
        assert!(result.is_some(), "should be in a.streams map");
    }

    let new_si = ACCEPT_CH_SIZE as u16;
    let result = a.streams.get(&new_si);
    assert!(result.is_none(), "should NOT be in a.streams map");

    let to_be_ignored = ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: a.peer_last_tsn + 1,
        stream_identifier: new_si,
        user_data: Bytes::from_static(b"ABC"),
        ..Default::default()
    };

    let p = a.handle_data(&to_be_ignored)?;
    assert!(p.is_empty(), "should return empty");

    Ok(())
}

fn handle_init_test(name: &str, initial_state: AssociationState, expect_err: bool) {
    let mut a = create_association(TransportConfig::default());
    a.set_state(initial_state);
    let pkt = Packet {
        common_header: CommonHeader {
            source_port: 5001,
            destination_port: 5002,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut init = ChunkInit {
        initial_tsn: 1234,
        num_outbound_streams: 1001,
        num_inbound_streams: 1002,
        initiate_tag: 5678,
        advertised_receiver_window_credit: 512 * 1024,
        ..Default::default()
    };
    init.set_supported_extensions();

    let result = a.handle_init(&pkt, &init);
    if expect_err {
        assert!(result.is_err(), "{} should fail", name);
        return;
    } else {
        assert!(result.is_ok(), "{} should be ok", name);
    }
    assert_eq!(
        if init.initial_tsn == 0 {
            u32::MAX
        } else {
            init.initial_tsn - 1
        },
        a.peer_last_tsn,
        "{} should match",
        name
    );
    assert_eq!(1001, a.my_max_num_outbound_streams, "{} should match", name);
    assert_eq!(1002, a.my_max_num_inbound_streams, "{} should match", name);
    assert_eq!(5678, a.peer_verification_tag, "{} should match", name);
    assert_eq!(
        pkt.common_header.source_port, a.destination_port,
        "{} should match",
        name
    );
    assert_eq!(
        pkt.common_header.destination_port, a.source_port,
        "{} should match",
        name
    );
    assert!(a.use_forward_tsn, "{} should be set to true", name);
    assert_eq!(
        512 * 1024,
        a.rwnd,
        "{} rwnd should be initialized from peer's advertised_receiver_window_credit",
        name
    );
    assert_eq!(
        a.rwnd, a.ssthresh,
        "{} ssthresh should be initialized to rwnd",
        name
    );
}

#[test]
fn test_assoc_handle_init() -> Result<()> {
    handle_init_test("normal", AssociationState::Closed, false);

    handle_init_test(
        "unexpected state established",
        AssociationState::Established,
        true,
    );

    handle_init_test(
        "unexpected state shutdownAckSent",
        AssociationState::ShutdownAckSent,
        true,
    );

    handle_init_test(
        "unexpected state shutdownPending",
        AssociationState::ShutdownPending,
        true,
    );

    handle_init_test(
        "unexpected state shutdownReceived",
        AssociationState::ShutdownReceived,
        true,
    );

    handle_init_test(
        "unexpected state shutdownSent",
        AssociationState::ShutdownSent,
        true,
    );

    Ok(())
}

#[test]
fn test_assoc_max_send_message_size_default() -> Result<()> {
    let mut a = create_association(TransportConfig::default());
    assert_eq!(65536, a.max_send_message_size, "should match");

    let ppi = PayloadProtocolIdentifier::Unknown;
    let stream = a.create_stream(1, false, ppi);
    assert!(stream.is_some(), "should succeed");

    if let Some(mut s) = stream {
        let p = Bytes::from(vec![0u8; 65537]);

        if let Err(err) = s.write_sctp(&p.slice(..65536), ppi) {
            assert_ne!(
                Error::ErrOutboundPacketTooLarge,
                err,
                "should be not Error::ErrOutboundPacketTooLarge"
            );
        } else {
            panic!("should be error");
        }

        if let Err(err) = s.write_sctp(&p.slice(..65537), ppi) {
            assert_eq!(
                Error::ErrOutboundPacketTooLarge,
                err,
                "should be Error::ErrOutboundPacketTooLarge"
            );
        } else {
            panic!("should be error");
        }
    }

    Ok(())
}

#[test]
fn test_assoc_max_send_message_size_explicit() -> Result<()> {
    let mut a = create_association(TransportConfig::default().with_max_send_message_size(30000));
    assert_eq!(30000, a.max_send_message_size, "should match");

    let ppi = PayloadProtocolIdentifier::Unknown;
    let stream = a.create_stream(1, false, ppi);
    assert!(stream.is_some(), "should succeed");

    if let Some(mut s) = stream {
        let p = Bytes::from(vec![0u8; 30001]);

        if let Err(err) = s.write_sctp(&p.slice(..30000), ppi) {
            assert_ne!(
                Error::ErrOutboundPacketTooLarge,
                err,
                "should be not Error::ErrOutboundPacketTooLarge"
            );
        } else {
            panic!("should be error");
        }

        if let Err(err) = s.write_sctp(&p.slice(..30001), ppi) {
            assert_eq!(
                Error::ErrOutboundPacketTooLarge,
                err,
                "should be Error::ErrOutboundPacketTooLarge"
            );
        } else {
            panic!("should be error");
        }
    }

    Ok(())
}

#[test]
fn test_assoc_max_receive_message_size_default() -> Result<()> {
    let mut a = create_association(TransportConfig::default());
    assert_eq!(65536, a.max_receive_message_size, "should match");

    let p = Bytes::from(vec![0u8; 65537]);

    let size_ok = ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: a.peer_last_tsn + 1,
        stream_identifier: 1,
        user_data: p.slice(..65536),
        ..Default::default()
    };

    assert!(a.handle_data(&size_ok).is_ok(), "should succeed");

    let too_large = ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: a.peer_last_tsn + 1,
        stream_identifier: 1,
        user_data: p,
        ..Default::default()
    };

    if let Err(err) = a.handle_data(&too_large) {
        assert_eq!(
            Error::ErrInboundPacketTooLarge,
            err,
            "should be Error::ErrInboundPacketTooLarge"
        );
    } else {
        panic!("should be error");
    }

    Ok(())
}

#[test]
fn test_assoc_max_receive_message_size_explicit() -> Result<()> {
    let mut a = create_association(TransportConfig::default().with_max_receive_message_size(1024));
    assert_eq!(1024, a.max_receive_message_size, "should match");

    let first_chunk = ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: a.peer_last_tsn + 1,
        stream_identifier: 1,
        user_data: Bytes::from(vec![0u8; 512]),
        ..Default::default()
    };

    assert!(a.handle_data(&first_chunk).is_ok(), "should succeed");

    let second_chunk = ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: a.peer_last_tsn + 1,
        stream_identifier: 1,
        user_data: Bytes::from(vec![0u8; 513]),
        ..Default::default()
    };

    if let Err(err) = a.handle_data(&second_chunk) {
        assert_eq!(
            Error::ErrInboundPacketTooLarge,
            err,
            "should be Error::ErrInboundPacketTooLarge"
        );
    } else {
        panic!("should be error");
    }

    Ok(())
}

#[test]
fn test_assoc_max_message_size_asymmetric() -> Result<()> {
    let config = TransportConfig::default()
        .with_max_send_message_size(1024)
        .with_max_receive_message_size(30000);

    let mut a = create_association(config);
    assert_eq!(1024, a.max_send_message_size, "should match");
    assert_eq!(30000, a.max_receive_message_size, "should match");

    let ppi = PayloadProtocolIdentifier::Unknown;
    let stream = a.create_stream(1, false, ppi);
    assert!(stream.is_some(), "should succeed");

    if let Some(mut s) = stream {
        let p = Bytes::from(vec![0u8; 1025]);

        if let Err(err) = s.write_sctp(&p.slice(..1024), ppi) {
            assert_ne!(
                Error::ErrOutboundPacketTooLarge,
                err,
                "should be not Error::ErrOutboundPacketTooLarge"
            );
        } else {
            panic!("should be error");
        }

        if let Err(err) = s.write_sctp(&p.slice(..1025), ppi) {
            assert_eq!(
                Error::ErrOutboundPacketTooLarge,
                err,
                "should be Error::ErrOutboundPacketTooLarge"
            );
        } else {
            panic!("should be error");
        }
    }

    let p = Bytes::from(vec![0u8; 30001]);

    let size_ok = ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: a.peer_last_tsn + 1,
        stream_identifier: 1,
        user_data: p.slice(..30000),
        ..Default::default()
    };

    assert!(a.handle_data(&size_ok).is_ok(), "should succeed");

    let too_large = ChunkPayloadData {
        beginning_fragment: true,
        ending_fragment: true,
        tsn: a.peer_last_tsn + 1,
        stream_identifier: 1,
        user_data: p,
        ..Default::default()
    };

    if let Err(err) = a.handle_data(&too_large) {
        assert_eq!(
            Error::ErrInboundPacketTooLarge,
            err,
            "should be Error::ErrInboundPacketTooLarge"
        );
    } else {
        panic!("should be error");
    }

    Ok(())
}

#[test]
fn test_generate_out_of_band_init() {
    let config = TransportConfig::default();
    let init_bytes = generate_snap_token(&config).unwrap();

    // Parse it back to validate
    let parsed = ChunkInit::unmarshal(&init_bytes).unwrap();

    assert!(!parsed.is_ack, "Should be INIT, not INIT ACK");
    assert!(parsed.initiate_tag != 0, "Initiate tag should not be zero");
    // Token always advertises u16::MAX for stream counts;
    // actual limits are applied from TransportConfig during negotiation.
    assert_eq!(
        parsed.num_outbound_streams,
        u16::MAX,
        "Outbound streams should always be u16::MAX in token"
    );
    assert_eq!(
        parsed.num_inbound_streams,
        u16::MAX,
        "Inbound streams should always be u16::MAX in token"
    );
    assert_eq!(
        parsed.advertised_receiver_window_credit,
        config.max_receive_buffer_size(),
        "ARWND should match config"
    );
}

#[test]
fn test_generate_out_of_band_init_with_custom_config() {
    let config = TransportConfig::default()
        .with_max_receive_buffer_size(2_000_000)
        .with_max_num_outbound_streams(256)
        .with_max_num_inbound_streams(512);

    let init_bytes = generate_snap_token(&config).unwrap();
    let parsed = ChunkInit::unmarshal(&init_bytes).unwrap();

    // Token always advertises u16::MAX for stream counts;
    // actual limits are applied from TransportConfig during negotiation.
    assert_eq!(parsed.num_outbound_streams, u16::MAX);
    assert_eq!(parsed.num_inbound_streams, u16::MAX);
    assert_eq!(parsed.advertised_receiver_window_credit, 2_000_000);
}

#[test]
fn test_generate_out_of_band_init_uniqueness() {
    // Each call to generate_snap_token creates a new INIT with unique random tags.
    let config1 = TransportConfig::default();
    let config2 = TransportConfig::default();

    let init1 = generate_snap_token(&config1).unwrap();
    let init2 = generate_snap_token(&config2).unwrap();

    let parsed1 = ChunkInit::unmarshal(&init1).unwrap();
    let parsed2 = ChunkInit::unmarshal(&init2).unwrap();

    // Initiate tags should be different (random)
    assert_ne!(
        parsed1.initiate_tag, parsed2.initiate_tag,
        "Initiate tags should be unique across calls"
    );

    // Initial TSNs should be different (random)
    assert_ne!(
        parsed1.initial_tsn, parsed2.initial_tsn,
        "Initial TSNs should be unique across calls"
    );
}

#[test]
fn test_out_of_band_association_creation() {
    let local_config = Arc::new(TransportConfig::default());
    let remote_config = TransportConfig::default();
    let max_payload_size = 1200;

    let local_init_bytes = generate_snap_token(&local_config).unwrap();
    let remote_init_bytes = generate_snap_token(&remote_config).unwrap();

    let local_init = ChunkInit::unmarshal(&local_init_bytes).unwrap();
    let remote_init = ChunkInit::unmarshal(&remote_init_bytes).unwrap();

    let remote_addr: SocketAddr = "192.168.1.1:5000".parse().unwrap();

    let assoc = Association::new_with_out_of_band_init(
        local_config.clone(),
        max_payload_size,
        remote_addr,
        None,
        local_init.clone(),
        remote_init.clone(),
    )
    .expect("Should create out-of-band init association");

    // Verify the association is in ESTABLISHED state
    assert_eq!(
        assoc.state(),
        AssociationState::Established,
        "Out-of-band init association should be in ESTABLISHED state"
    );

    // Verify handshake is marked complete
    assert!(
        assoc.handshake_completed,
        "Out-of-band init association should have handshake completed"
    );

    // Verify verification tags
    assert_eq!(
        assoc.my_verification_tag, local_init.initiate_tag,
        "My verification tag should match local init"
    );
    assert_eq!(
        assoc.peer_verification_tag, remote_init.initiate_tag,
        "Peer verification tag should match remote init"
    );

    // Verify TSN setup
    assert_eq!(
        assoc.my_next_tsn, local_init.initial_tsn,
        "My next TSN should match local init"
    );
    assert_eq!(
        assoc.peer_last_tsn,
        remote_init.initial_tsn.wrapping_sub(1),
        "Peer last TSN should be remote init TSN - 1"
    );

    // Verify rwnd
    assert_eq!(
        assoc.rwnd, remote_init.advertised_receiver_window_credit,
        "rwnd should match remote advertised credit"
    );
}

#[test]
fn test_out_of_band_association_stream_negotiation() {
    let config = Arc::new(
        TransportConfig::default()
            .with_max_num_outbound_streams(100)
            .with_max_num_inbound_streams(200),
    );

    // Remote uses default config — token always advertises u16::MAX
    let remote_config = TransportConfig::default();

    let local_init_bytes = generate_snap_token(&config).unwrap();
    let remote_init_bytes = generate_snap_token(&remote_config).unwrap();

    let local_init = ChunkInit::unmarshal(&local_init_bytes).unwrap();
    let remote_init = ChunkInit::unmarshal(&remote_init_bytes).unwrap();

    // Token always advertises u16::MAX for stream counts
    assert_eq!(local_init.num_outbound_streams, u16::MAX);
    assert_eq!(local_init.num_inbound_streams, u16::MAX);
    assert_eq!(remote_init.num_outbound_streams, u16::MAX);
    assert_eq!(remote_init.num_inbound_streams, u16::MAX);

    let remote_addr: SocketAddr = "192.168.1.1:5000".parse().unwrap();

    let assoc = Association::new_with_out_of_band_init(
        config.clone(),
        1200,
        remote_addr,
        None,
        local_init,
        remote_init,
    )
    .expect("Should create out-of-band init association");

    // Stream limits should be clamped by the local config, since the
    // remote token always offers u16::MAX.
    // my_max_num_outbound_streams = min(config.max_out=100, remote_in=MAX) = 100
    assert_eq!(
        assoc.my_max_num_outbound_streams, 100,
        "Outbound streams should be clamped by local config"
    );

    // my_max_num_inbound_streams = min(config.max_in=200, remote_out=MAX) = 200
    assert_eq!(
        assoc.my_max_num_inbound_streams, 200,
        "Inbound streams should be clamped by local config"
    );
}

#[test]
fn test_out_of_band_connected_event() {
    let local_config = Arc::new(TransportConfig::default());
    let remote_config = TransportConfig::default();

    let local_init_bytes = generate_snap_token(&local_config).unwrap();
    let remote_init_bytes = generate_snap_token(&remote_config).unwrap();

    let local_init = ChunkInit::unmarshal(&local_init_bytes).unwrap();
    let remote_init = ChunkInit::unmarshal(&remote_init_bytes).unwrap();

    let remote_addr: SocketAddr = "192.168.1.1:5000".parse().unwrap();

    let mut assoc = Association::new_with_out_of_band_init(
        local_config.clone(),
        1200,
        remote_addr,
        None,
        local_init,
        remote_init,
    )
    .expect("Should create out-of-band init association");

    // Poll should return a Connected event
    let event = assoc.poll();
    assert!(
        matches!(event, Some(Event::Connected)),
        "Should emit Connected event, got {:?}",
        event
    );
}

#[test]
fn test_out_of_band_symmetric_setup() {
    // Test that both sides of an out-of-band init association work correctly
    let config_a = Arc::new(TransportConfig::default());
    let config_b = Arc::new(TransportConfig::default());

    let init_a_bytes = generate_snap_token(&config_a).unwrap();
    let init_b_bytes = generate_snap_token(&config_b).unwrap();

    let init_a = ChunkInit::unmarshal(&init_a_bytes).unwrap();
    let init_b = ChunkInit::unmarshal(&init_b_bytes).unwrap();

    let addr_a: SocketAddr = "192.168.1.1:5000".parse().unwrap();
    let addr_b: SocketAddr = "192.168.1.2:5000".parse().unwrap();

    // Create association A (local=A, remote=B)
    let assoc_a = Association::new_with_out_of_band_init(
        config_a.clone(),
        1200,
        addr_b,
        None,
        init_a.clone(),
        init_b.clone(),
    )
    .expect("Should create association A");

    // Create association B (local=B, remote=A)
    let assoc_b = Association::new_with_out_of_band_init(
        config_b.clone(),
        1200,
        addr_a,
        None,
        init_b.clone(),
        init_a.clone(),
    )
    .expect("Should create association B");

    // Verify both are in ESTABLISHED state
    assert_eq!(assoc_a.state(), AssociationState::Established);
    assert_eq!(assoc_b.state(), AssociationState::Established);

    // Verify verification tags are cross-matched
    assert_eq!(assoc_a.my_verification_tag, assoc_b.peer_verification_tag);
    assert_eq!(assoc_b.my_verification_tag, assoc_a.peer_verification_tag);
}

#[test]
fn test_out_of_band_with_forward_tsn_support() {
    let local_config = Arc::new(TransportConfig::default());
    let remote_config = TransportConfig::default();

    let local_init_bytes = generate_snap_token(&local_config).unwrap();
    let remote_init_bytes = generate_snap_token(&remote_config).unwrap();

    let local_init = ChunkInit::unmarshal(&local_init_bytes).unwrap();
    let remote_init = ChunkInit::unmarshal(&remote_init_bytes).unwrap();

    // Verify supported extensions are present
    let mut has_forward_tsn = false;
    for param in &local_init.params {
        if let Some(ext) = param
            .as_any()
            .downcast_ref::<crate::param::param_supported_extensions::ParamSupportedExtensions>(
        ) {
            for ct in &ext.chunk_types {
                if *ct == crate::chunk::chunk_type::CT_FORWARD_TSN {
                    has_forward_tsn = true;
                }
            }
        }
    }
    assert!(
        has_forward_tsn,
        "Generated INIT should include ForwardTSN support"
    );

    let remote_addr: SocketAddr = "192.168.1.1:5000".parse().unwrap();

    let assoc = Association::new_with_out_of_band_init(
        local_config.clone(),
        1200,
        remote_addr,
        None,
        local_init,
        remote_init,
    )
    .expect("Should create out-of-band init association");

    assert!(
        assoc.use_forward_tsn,
        "Out-of-band init association should have ForwardTSN enabled"
    );
}

#[test]
fn test_out_of_band_initial_tsn_zero_wrap() {
    // Test edge case where initial TSN is 0 (wraps to MAX)
    let config = Arc::new(TransportConfig::default());

    let local_init_bytes = generate_snap_token(&config).unwrap();
    let mut remote_init = ChunkInit::unmarshal(&local_init_bytes).unwrap();

    // Set initial TSN to 0 to test the edge case
    remote_init.initial_tsn = 0;
    remote_init.initiate_tag = 12345;

    // Generate a fresh local init for the association
    let actual_local_init_bytes = generate_snap_token(&config).unwrap();
    let local_init = ChunkInit::unmarshal(&actual_local_init_bytes).unwrap();
    let remote_addr: SocketAddr = "192.168.1.1:5000".parse().unwrap();

    let assoc = Association::new_with_out_of_band_init(
        config.clone(),
        1200,
        remote_addr,
        None,
        local_init,
        remote_init,
    )
    .expect("Should create out-of-band init association");

    // peer_last_tsn should be u32::MAX when initial_tsn is 0
    assert_eq!(
        assoc.peer_last_tsn,
        u32::MAX,
        "peer_last_tsn should wrap to MAX when initial_tsn is 0"
    );
}

#[test]
fn test_out_of_band_rwnd_negotiation() {
    let local_config = Arc::new(TransportConfig::default().with_max_receive_buffer_size(500_000));

    let remote_config = TransportConfig::default().with_max_receive_buffer_size(300_000);

    let local_init_bytes = generate_snap_token(&local_config).unwrap();
    let remote_init_bytes = generate_snap_token(&remote_config).unwrap();

    let local_init = ChunkInit::unmarshal(&local_init_bytes).unwrap();
    let remote_init = ChunkInit::unmarshal(&remote_init_bytes).unwrap();

    let remote_addr: SocketAddr = "192.168.1.1:5000".parse().unwrap();

    let assoc = Association::new_with_out_of_band_init(
        local_config.clone(),
        1200,
        remote_addr,
        None,
        local_init,
        remote_init,
    )
    .expect("Should create out-of-band init association");

    // rwnd should be set to remote's advertised receiver window credit
    assert_eq!(
        assoc.rwnd, 300_000,
        "rwnd should be remote's advertised window"
    );
}

#[test]
fn test_initial_cwnd_small_mtu() {
    // Regression test for #47: a small MTU made 4*MTU < 4380, which previously
    // panicked because clamp(4380, 4*MTU) was called with min > max.
    let max_payload_size = 100;
    let mtu = max_payload_size + COMMON_HEADER_SIZE + DATA_CHUNK_HEADER_SIZE;
    assert!(4 * mtu < 4380, "test must exercise the small-MTU branch");

    let assoc = Association::new(
        None,
        Arc::new(TransportConfig::default()),
        max_payload_size,
        0,
        SocketAddr::from_str("0.0.0.0:0").unwrap(),
        None,
        Instant::now(),
    );

    // RFC 4960 Sec 7.2.1: min(4*MTU, max(2*MTU, 4380)).
    assert_eq!(assoc.cwnd, (4 * mtu).min((2 * mtu).max(4380)));
    assert_eq!(assoc.cwnd, 4 * mtu);
}

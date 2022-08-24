pub mod protocol;
pub mod signaling;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("expected hello")]
    ExpectedHello,

    #[error("protocol version too old")]
    ProtocolVersionTooOld,

    #[error("protocol version too new")]
    ProtocolVersionTooNew,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

async fn create_data_channel(
    ice_servers: &[String],
) -> Result<
    (
        datachannel_wrapper::DataChannel,
        tokio::sync::mpsc::Receiver<datachannel_wrapper::PeerConnectionEvent>,
        datachannel_wrapper::PeerConnection,
    ),
    anyhow::Error,
> {
    let (mut peer_conn, mut event_rx) =
        datachannel_wrapper::PeerConnection::new(datachannel_wrapper::RtcConfig::new(ice_servers))?;

    let dc = peer_conn.create_data_channel(
        "tango",
        datachannel_wrapper::DataChannelInit::default()
            .reliability(datachannel_wrapper::Reliability {
                unordered: false,
                unreliable: false,
                max_packet_life_time: 0,
                max_retransmits: 0,
            })
            .negotiated()
            .manual_stream()
            .stream(0),
    )?;

    loop {
        if let Some(datachannel_wrapper::PeerConnectionEvent::GatheringStateChange(
            datachannel_wrapper::GatheringState::Complete,
        )) = event_rx.recv().await
        {
            break;
        }
    }

    Ok((dc, event_rx, peer_conn))
}

pub async fn negotiate(
    session_id: &str,
    signaling_connect_addr: &str,
    ice_servers: &[String],
) -> Result<
    (
        datachannel_wrapper::DataChannel,
        datachannel_wrapper::PeerConnection,
    ),
    Error,
> {
    let (dc, event_rx, mut peer_conn) = create_data_channel(ice_servers).await?;

    log::info!(
        "negotiating match, session_id = {}, ice_servers = {:?}",
        session_id,
        ice_servers
    );

    let signaling_stream = signaling::open(
        signaling_connect_addr,
        session_id,
        &peer_conn.local_description().unwrap(),
    )
    .await?;
    signaling::connect(&mut peer_conn, signaling_stream, event_rx).await?;

    let (mut dc_tx, mut dc_rx) = dc.split();

    log::debug!(
        "local sdp (type = {:?}): {}",
        peer_conn.local_description().expect("local sdp").sdp_type,
        peer_conn.local_description().expect("local sdp").sdp
    );
    log::debug!(
        "remote sdp (type = {:?}): {}",
        peer_conn.remote_description().expect("remote sdp").sdp_type,
        peer_conn.remote_description().expect("remote sdp").sdp
    );

    dc_tx
        .send(
            protocol::Packet::Hello(protocol::Hello {
                protocol_version: protocol::VERSION,
            })
            .serialize()
            .expect("serialize")
            .as_slice(),
        )
        .await
        .map_err(|e| Error::Other(e.into()))?;

    let hello = match protocol::Packet::deserialize(
        match dc_rx.receive().await {
            Some(d) => d,
            None => {
                return Err(Error::ExpectedHello);
            }
        }
        .as_slice(),
    )
    .map_err(|_| Error::ExpectedHello)?
    {
        protocol::Packet::Hello(hello) => hello,
        _ => {
            return Err(Error::ExpectedHello);
        }
    };

    if hello.protocol_version < protocol::VERSION {
        return Err(Error::ProtocolVersionTooOld);
    }

    if hello.protocol_version > protocol::VERSION {
        return Err(Error::ProtocolVersionTooNew);
    }

    Ok((dc_rx.unsplit(dc_tx), peer_conn))
}

pub struct Transport {
    dc_tx: datachannel_wrapper::DataChannelSender,
}

impl Transport {
    pub fn new(dc_tx: datachannel_wrapper::DataChannelSender) -> Transport {
        Transport { dc_tx }
    }

    pub async fn send_input(
        &mut self,
        round_number: u8,
        local_tick: u32,
        tick_diff: i8,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        self.dc_tx
            .send(
                protocol::Packet::Input(protocol::Input {
                    round_number,
                    local_tick,
                    tick_diff,
                    joyflags,
                })
                .serialize()?
                .as_slice(),
            )
            .await?;
        Ok(())
    }
}

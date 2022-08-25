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

pub async fn negotiate(
    session_id: &str,
    signaling_connect_addr: &str,
) -> Result<
    (
        datachannel_wrapper::DataChannel,
        datachannel_wrapper::PeerConnection,
    ),
    Error,
> {
    let pending = signaling::open(signaling_connect_addr, session_id).await?;
    let (dc, peer_conn) = pending.connect().await?;

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

    async fn send_packet(&mut self, p: &protocol::Packet) -> anyhow::Result<()> {
        self.dc_tx.send(p.serialize()?.as_slice()).await?;
        Ok(())
    }

    pub async fn send_hello(&mut self) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Hello(protocol::Hello {
            protocol_version: protocol::VERSION,
        }))
        .await
    }

    pub async fn send_ping(&mut self, ts: std::time::SystemTime) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Ping(protocol::Ping { ts }))
            .await
    }

    pub async fn send_pong(&mut self, ts: std::time::SystemTime) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Pong(protocol::Pong { ts }))
            .await
    }

    pub async fn send_set_settings(
        &mut self,
        set_settings: protocol::SetSettings,
    ) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::SetSettings(set_settings))
            .await
    }

    pub async fn send_commit(&mut self, commitment: [u8; 16]) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Commit(protocol::Commit { commitment }))
            .await
    }

    pub async fn send_uncommit(&mut self) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Uncommit(protocol::Uncommit {}))
            .await
    }

    pub async fn send_chunk(&mut self, chunk: Vec<u8>) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Chunk(protocol::Chunk { chunk }))
            .await
    }

    pub async fn send_start_match(&mut self) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::StartMatch(protocol::StartMatch {}))
            .await
    }

    pub async fn send_input(
        &mut self,
        round_number: u8,
        local_tick: u32,
        tick_diff: i8,
        joyflags: u16,
    ) -> anyhow::Result<()> {
        self.send_packet(&protocol::Packet::Input(protocol::Input {
            round_number,
            local_tick,
            tick_diff,
            joyflags,
        }))
        .await
    }
}

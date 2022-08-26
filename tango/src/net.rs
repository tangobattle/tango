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

pub async fn negotiate(sender: &mut Sender, receiver: &mut Receiver) -> Result<(), Error> {
    sender
        .send_hello()
        .await
        .map_err(|e| Error::Other(e.into()))?;

    let hello = match receiver.receive().await.map_err(|_| Error::ExpectedHello)? {
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

    Ok(())
}

pub struct Sender {
    dc_tx: datachannel_wrapper::DataChannelSender,
}

impl Sender {
    pub fn new(dc_tx: datachannel_wrapper::DataChannelSender) -> Self {
        Self { dc_tx }
    }

    async fn send_packet(&mut self, p: &protocol::Packet) -> std::io::Result<()> {
        match self.dc_tx.send(p.serialize().unwrap().as_slice()).await {
            Ok(()) => Ok(()),
            Err(datachannel_wrapper::Error::Closed) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "unexpected eof",
                ));
            }
            Err(e) => {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
            }
        }
    }

    pub async fn send_hello(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Hello(protocol::Hello {
            protocol_version: protocol::VERSION,
        }))
        .await
    }

    pub async fn send_ping(&mut self, ts: std::time::SystemTime) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Ping(protocol::Ping { ts }))
            .await
    }

    pub async fn send_pong(&mut self, ts: std::time::SystemTime) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Pong(protocol::Pong { ts }))
            .await
    }

    pub async fn send_settings(&mut self, settings: protocol::Settings) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Settings(settings))
            .await
    }

    pub async fn send_commit(&mut self, commitment: [u8; 16]) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Commit(protocol::Commit { commitment }))
            .await
    }

    pub async fn send_uncommit(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Uncommit(protocol::Uncommit {}))
            .await
    }

    pub async fn send_chunk(&mut self, chunk: Vec<u8>) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Chunk(protocol::Chunk { chunk }))
            .await
    }

    pub async fn send_start_match(&mut self) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::StartMatch(protocol::StartMatch {}))
            .await
    }

    pub async fn send_input(
        &mut self,
        round_number: u8,
        local_tick: u32,
        tick_diff: i8,
        joyflags: u16,
    ) -> std::io::Result<()> {
        self.send_packet(&protocol::Packet::Input(protocol::Input {
            round_number,
            local_tick,
            tick_diff,
            joyflags,
        }))
        .await
    }
}

pub struct Receiver {
    dc_rx: datachannel_wrapper::DataChannelReceiver,
}

impl Receiver {
    pub fn new(dc_rx: datachannel_wrapper::DataChannelReceiver) -> Self {
        Self { dc_rx }
    }

    pub async fn receive(&mut self) -> std::io::Result<protocol::Packet> {
        match protocol::Packet::deserialize(
            match self.dc_rx.receive().await {
                Some(d) => d,
                None => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "stream is empty",
                    ));
                }
            }
            .as_slice(),
        ) {
            Ok(p) => Ok(p),
            Err(e) => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
            }
        }
    }
}

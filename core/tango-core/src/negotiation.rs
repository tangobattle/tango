use crate::{ipc, protocol};
use rand::Rng;
use rand::SeedableRng;
use sha3::digest::ExtendableOutput;
use std::io::Read;
use std::io::Write;
use subtle::ConstantTimeEq;

pub struct Negotiation {
    pub dc: datachannel_wrapper::DataChannel,
    pub peer_conn: datachannel_wrapper::PeerConnection,
    pub rng: rand_pcg::Mcg128Xsl64,
    pub input_delay: u32,
}

#[derive(Debug)]
pub enum Error {
    ExpectedHello,
    ExpectedHola,
    IdenticalCommitment,
    ProtocolVersionMismatch,
    MatchTypeMismatch,
    InvalidCommitment,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Error::Other(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Other(err.into())
    }
}

impl From<datachannel_wrapper::Error> for Error {
    fn from(err: datachannel_wrapper::Error) -> Self {
        Error::Other(err.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::ExpectedHello => write!(f, "expected hello"),
            Error::ExpectedHola => write!(f, "expected hola"),
            Error::IdenticalCommitment => write!(f, "identical commitment"),
            Error::ProtocolVersionMismatch => write!(f, "protocol version mismatch"),
            Error::MatchTypeMismatch => write!(f, "match type mismatch"),
            Error::InvalidCommitment => write!(f, "invalid commitment"),
            Error::Other(e) => write!(f, "other error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

fn make_rng_commitment(nonce: &[u8]) -> std::io::Result<[u8; 32]> {
    let mut shake128 = sha3::Shake128::default();
    shake128.write_all(b"syncrand:nonce:")?;
    shake128.write_all(nonce)?;

    let mut commitment = [0u8; 32];
    shake128
        .finalize_xof()
        .read_exact(commitment.as_mut_slice())?;

    Ok(commitment)
}

pub async fn negotiate(
    ipc_client: &mut ipc::Client,
    session_id: &str,
    matchmaking_connect_addr: &str,
    ice_servers: &[String],
    input_delay: u32,
) -> Result<Negotiation, Error> {
    log::info!("negotiating match, session_id = {}", session_id);
    ipc_client
        .send_notification(ipc::Notification::State(ipc::State::Waiting))
        .await?;

    let (mut peer_conn, signal_receiver) =
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

    tango_matchmaking::client::connect(
        &matchmaking_connect_addr,
        &mut peer_conn,
        signal_receiver,
        &session_id,
    )
    .await?;

    let (mut dc_rx, mut dc_tx) = dc.split();

    log::info!(
        "local sdp (type = {:?}): {}",
        peer_conn.local_description().expect("local sdp").sdp_type,
        peer_conn.local_description().expect("local sdp").sdp
    );
    log::info!(
        "remote sdp (type = {:?}): {}",
        peer_conn.remote_description().expect("remote sdp").sdp_type,
        peer_conn.remote_description().expect("remote sdp").sdp
    );

    ipc_client
        .send_notification(ipc::Notification::State(ipc::State::Connecting))
        .await?;
    let mut nonce = [0u8; 16];
    rand::rngs::OsRng {}.fill(&mut nonce);
    let commitment = make_rng_commitment(&nonce)?;

    log::info!("our nonce={:?}, commitment={:?}", nonce, commitment);

    dc_tx
        .send(
            protocol::Packet::Hello(protocol::Hello {
                protocol_version: protocol::VERSION,
                rng_commitment: commitment.to_vec(),
                input_delay,
            })
            .serialize()
            .expect("serialize")
            .as_slice(),
        )
        .await?;

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

    log::info!("their hello={:?}", hello);

    if commitment.ct_eq(hello.rng_commitment.as_slice()).into() {
        return Err(Error::IdenticalCommitment);
    }

    if hello.protocol_version != protocol::VERSION {
        return Err(Error::ProtocolVersionMismatch);
    }

    dc_tx
        .send(
            protocol::Packet::Hola(protocol::Hola {
                rng_nonce: nonce.to_vec(),
            })
            .serialize()
            .expect("serialize")
            .as_slice(),
        )
        .await?;

    let hola = match protocol::Packet::deserialize(
        match dc_rx.receive().await {
            Some(d) => d,
            None => {
                return Err(Error::ExpectedHola);
            }
        }
        .as_slice(),
    )
    .map_err(|_| Error::ExpectedHola)?
    {
        protocol::Packet::Hola(hola) => hola,
        _ => {
            return Err(Error::ExpectedHola);
        }
    };

    log::info!("their hola={:?}", hola);

    if !bool::from(make_rng_commitment(&hola.rng_nonce)?.ct_eq(hello.rng_commitment.as_slice())) {
        return Err(Error::InvalidCommitment);
    }

    log::info!("connection ok!");

    let seed = hola
        .rng_nonce
        .iter()
        .zip(nonce.iter())
        .map(|(&x1, &x2)| x1 ^ x2)
        .collect::<Vec<u8>>();

    Ok(Negotiation {
        dc: dc_rx.unsplit(dc_tx),
        peer_conn,
        rng: rand_pcg::Mcg128Xsl64::from_seed(seed.try_into().expect("rng seed")),
        input_delay: hello.input_delay,
    })
}

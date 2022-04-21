use crate::{battle, datachannel, ipc, protocol};
use rand::Rng;
use rand::SeedableRng;
use sha3::digest::ExtendableOutput;
use std::io::Read;
use std::io::Write;
use subtle::ConstantTimeEq;

pub struct Negotiation {
    pub dc: std::sync::Arc<datachannel::DataChannel>,
    pub peer_conn: webrtc::peer_connection::RTCPeerConnection,
    pub side: tango_matchmaking::client::ConnectionSide,
    pub rng: rand_pcg::Mcg128Xsl64,
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

impl From<webrtc::Error> for Error {
    fn from(err: webrtc::Error) -> Self {
        Error::Other(err.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
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
    game_title: &str,
    game_crc32: u32,
    session_id: &str,
    matchmaking_connect_addr: &str,
    ice_servers: &[webrtc::ice_transport::ice_server::RTCIceServer],
    battle_settings: &battle::Settings,
) -> Result<Negotiation, Error> {
    log::info!("negotiating match, session_id = {}", session_id);
    ipc_client.send_notification(ipc::Notification::State(ipc::State::Waiting))?;

    let api = webrtc::api::APIBuilder::new().build();
    let (peer_conn, dc, side) = tango_matchmaking::client::connect(
        &matchmaking_connect_addr,
        || async {
            let peer_conn = api
                .new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration {
                    ice_servers: ice_servers.to_owned(),
                    ..Default::default()
                })
                .await?;
            let dc = peer_conn
                .create_data_channel(
                    "tango",
                    Some(
                        webrtc::data_channel::data_channel_init::RTCDataChannelInit {
                            id: Some(1),
                            negotiated: Some(true),
                            ordered: Some(true),
                            ..Default::default()
                        },
                    ),
                )
                .await?;
            Ok((peer_conn, dc))
        },
        &session_id,
    )
    .await?;
    let dc = datachannel::DataChannel::new(dc).await;

    log::info!(
        "local sdp: {}",
        peer_conn.local_description().await.expect("local sdp").sdp
    );
    log::info!(
        "remote sdp: {}",
        peer_conn
            .remote_description()
            .await
            .expect("remote sdp")
            .sdp
    );

    ipc_client.send_notification(ipc::Notification::State(ipc::State::Connecting))?;
    let mut nonce = [0u8; 16];
    rand::rngs::OsRng {}.fill(&mut nonce);
    let commitment = make_rng_commitment(&nonce)?;

    log::info!("our nonce={:?}, commitment={:?}", nonce, commitment);

    dc.send(
        protocol::Packet::Hello(protocol::Hello {
            protocol_version: protocol::VERSION,
            game_title: game_title.to_owned(),
            game_crc32,
            match_type: battle_settings.match_type,
            rng_commitment: commitment.to_vec(),
        })
        .serialize()
        .expect("serialize")
        .as_slice(),
    )
    .await?;

    let hello = match protocol::Packet::deserialize(
        match dc.receive().await {
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

    if hello.match_type != battle_settings.match_type {
        return Err(Error::MatchTypeMismatch);
    }

    dc.send(
        protocol::Packet::Hola(protocol::Hola {
            rng_nonce: nonce.to_vec(),
        })
        .serialize()
        .expect("serialize")
        .as_slice(),
    )
    .await?;

    let hola = match protocol::Packet::deserialize(
        match dc.receive().await {
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
        dc,
        peer_conn,
        side,
        rng: rand_pcg::Mcg128Xsl64::from_seed(seed.try_into().expect("rng seed")),
    })
}

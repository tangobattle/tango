//! A small async/Tokio-friendly facade over [`libdatachannel`].
//!
//! `libdatachannel` exposes a synchronous, callback-based API. The rest of
//! Tango is written against an async, channel-based shape (peer-connection
//! events arrive on an `mpsc::Receiver`, data-channel sends/receives are
//! `await`-able). This crate keeps that shape: native callbacks fire on
//! libdatachannel's own threads and we forward them onto Tokio channels.

// Re-export the libdatachannel enums whose shape/variants we use verbatim.
pub use libdatachannel::{
    GatheringState, LocalDescriptionInit, Reliability, SdpType, State as ConnectionState, TransportPolicy,
};

/// An ICE candidate string emitted by the local agent. Tango does non-trickle
/// ICE (it waits for gathering to complete and ships the full SDP), so nothing
/// consumes these today, but they're surfaced for API completeness.
#[derive(Debug, Clone)]
pub struct IceCandidate {
    pub candidate: String,
}

/// A full SDP session description. Unlike the old wrapper, `sdp` is just the raw
/// SDP text — libdatachannel parses/serializes it internally.
#[derive(Debug, Clone)]
pub struct SessionDescription {
    pub sdp_type: SdpType,
    pub sdp: String,
}

/// Peer connection configuration. A thin stand-in for the old `RtcConfig`,
/// carrying only what Tango sets.
#[derive(Debug, Clone, Default)]
pub struct RtcConfig {
    pub ice_servers: Vec<String>,
    pub ice_transport_policy: TransportPolicy,
    /// Skip the DTLS fingerprint check. Required by the signaling-free
    /// direct transport, whose fabricated remote SDP carries a dummy
    /// (non-matching) fingerprint.
    pub disable_fingerprint_verification: bool,
    /// Don't auto-generate an offer when a data channel is created. The
    /// signaling-free direct transport needs this so its manual
    /// `set_local_description_ex` (with pinned ICE creds) is what runs,
    /// rather than an auto offer with random creds.
    pub disable_auto_negotiation: bool,
    /// Pin the local UDP port range, `(begin, end)`. The signaling-free
    /// host sets `(port, port)` so the dialing peer knows where to reach
    /// it. `None` lets libdatachannel pick.
    pub port_range: Option<(u16, u16)>,
}

impl RtcConfig {
    pub fn new<S: AsRef<str>>(ice_servers: &[S]) -> Self {
        Self {
            ice_servers: ice_servers.iter().map(|s| s.as_ref().to_owned()).collect(),
            ice_transport_policy: TransportPolicy::All,
            ..Default::default()
        }
    }
}

/// Builder for data-channel options, matching the old fluent API
/// (`DataChannelInit::default().reliability(..).negotiated().manual_stream().stream(0)`).
#[derive(Default)]
pub struct DataChannelInit(libdatachannel::DataChannelOptions);

impl DataChannelInit {
    pub fn reliability(mut self, reliability: Reliability) -> Self {
        self.0.reliability = reliability;
        self
    }

    pub fn negotiated(mut self) -> Self {
        self.0.negotiated = true;
        self
    }

    /// Use a caller-assigned stream id. In libdatachannel a manual stream is
    /// simply expressed as `stream = Some(..)`, so this is a no-op kept for API
    /// compatibility — call [`DataChannelInit::stream`] to set the id.
    pub fn manual_stream(self) -> Self {
        self
    }

    pub fn stream(mut self, stream: u16) -> Self {
        self.0.stream = Some(stream);
        self
    }

    pub fn protocol(mut self, protocol: String) -> Self {
        self.0.protocol = protocol;
        self
    }
}

/// Events forwarded off libdatachannel's threads onto the peer-connection's
/// event channel.
#[derive(Debug)]
pub enum PeerConnectionEvent {
    SessionDescription(SessionDescription),
    IceCandidate(IceCandidate),
    ConnectionStateChange(ConnectionState),
    GatheringStateChange(GatheringState),
}

fn error_to_io(err: libdatachannel::Error) -> std::io::Error {
    match err {
        libdatachannel::Error::Invalid => std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid argument"),
        libdatachannel::Error::Failure => std::io::Error::other("runtime error"),
        libdatachannel::Error::NotAvail => std::io::Error::new(std::io::ErrorKind::WouldBlock, "not available"),
        libdatachannel::Error::TooSmall => std::io::Error::new(std::io::ErrorKind::InvalidInput, "buffer too small"),
    }
}

pub struct PeerConnection {
    inner: libdatachannel::PeerConnection,
    data_channel_rx: tokio::sync::mpsc::Receiver<DataChannel>,
}

impl PeerConnection {
    pub fn new(config: RtcConfig) -> Result<(Self, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(1);
        let (data_channel_tx, data_channel_rx) = tokio::sync::mpsc::channel(1);

        let (port_range_begin, port_range_end) = config.port_range.unwrap_or((0, 0));
        let mut inner = libdatachannel::PeerConnection::new(libdatachannel::Configuration {
            ice_servers: config.ice_servers,
            ice_transport_policy: config.ice_transport_policy,
            disable_fingerprint_verification: config.disable_fingerprint_verification,
            disable_auto_negotiation: config.disable_auto_negotiation,
            port_range_begin,
            port_range_end,
            ..Default::default()
        })
        .map_err(error_to_io)?;

        inner.set_on_local_description(Some({
            let event_tx = event_tx.clone();
            move |sdp: &str, type_: SdpType| {
                let _ = event_tx.blocking_send(PeerConnectionEvent::SessionDescription(SessionDescription {
                    sdp_type: type_,
                    sdp: sdp.to_owned(),
                }));
            }
        }));

        inner.set_on_local_candidate(Some({
            let event_tx = event_tx.clone();
            move |cand: &str| {
                let _ = event_tx.blocking_send(PeerConnectionEvent::IceCandidate(IceCandidate {
                    candidate: cand.to_owned(),
                }));
            }
        }));

        inner.set_on_state_change(Some({
            let event_tx = event_tx.clone();
            move |state: ConnectionState| {
                // libdatachannel can fire this synchronously from the destructor
                // on the same thread Tokio is driving (horrible). `blocking_send`
                // would panic in an async context, so try-send there instead.
                let event = PeerConnectionEvent::ConnectionStateChange(state);
                match tokio::runtime::Handle::try_current() {
                    Ok(_) => {
                        let _ = event_tx.try_send(event);
                    }
                    Err(_) => {
                        let _ = event_tx.blocking_send(event);
                    }
                }
            }
        }));

        inner.set_on_gathering_state_change(Some({
            let event_tx = event_tx.clone();
            move |state: GatheringState| {
                let _ = event_tx.blocking_send(PeerConnectionEvent::GatheringStateChange(state));
            }
        }));

        inner.set_on_data_channel(Some({
            let data_channel_tx = data_channel_tx.clone();
            move |dc: libdatachannel::DataChannel| {
                let _ = data_channel_tx.blocking_send(DataChannel::wrap(dc));
            }
        }));

        Ok((PeerConnection { inner, data_channel_rx }, event_rx))
    }

    pub fn create_data_channel(
        &mut self,
        label: &str,
        dc_init: DataChannelInit,
    ) -> Result<DataChannel, std::io::Error> {
        let dc = self.inner.create_data_channel(label, dc_init.0).map_err(error_to_io)?;
        Ok(DataChannel::wrap(dc))
    }

    pub async fn accept(&mut self) -> Option<DataChannel> {
        self.data_channel_rx.recv().await
    }

    pub fn set_local_description(
        &mut self,
        sdp_type: SdpType,
        init: Option<&LocalDescriptionInit>,
    ) -> Result<(), std::io::Error> {
        self.inner
            .set_local_description(Some(sdp_type), init)
            .map_err(error_to_io)
    }

    pub fn set_remote_description(&mut self, sess_desc: SessionDescription) -> Result<(), std::io::Error> {
        self.inner
            .set_remote_description(&libdatachannel::Description {
                type_: sess_desc.sdp_type,
                sdp: sess_desc.sdp,
            })
            .map_err(error_to_io)
    }

    pub fn local_description(&self) -> Option<SessionDescription> {
        self.inner.local_description().ok().map(|d| SessionDescription {
            sdp_type: d.type_,
            sdp: d.sdp,
        })
    }

    pub fn remote_description(&self) -> Option<SessionDescription> {
        self.inner.remote_description().ok().map(|d| SessionDescription {
            sdp_type: d.type_,
            sdp: d.sdp,
        })
    }

    pub fn add_remote_candidate(&mut self, cand: IceCandidate) -> Result<(), std::io::Error> {
        self.inner.add_remote_candidate(&cand.candidate).map_err(error_to_io)
    }

    /// The selected ICE candidate pair as raw candidate strings,
    /// `(local, remote)` — e.g. for telling a relayed (TURN)
    /// connection from a direct one (`typ relay`). Errors until
    /// the agent has picked a pair.
    pub fn selected_candidate_pair(&self) -> Result<(String, String), std::io::Error> {
        self.inner.selected_candidate_pair().map_err(error_to_io)
    }
}

/// The lifecycle of a data channel as observed by the send side.
#[derive(Debug, Clone)]
enum DataChannelStatus {
    Pending,
    Open,
    Closed,
    Error(String),
}

pub struct DataChannel {
    sender: DataChannelSender,
    receiver: DataChannelReceiver,
}

impl DataChannel {
    /// Attach our Tokio-channel callbacks to a raw libdatachannel data channel
    /// and split it into a sender/receiver pair. Shared by `create_data_channel`
    /// (local) and the `on_data_channel` callback (remote-opened).
    fn wrap(mut inner: libdatachannel::DataChannel) -> Self {
        let (status_tx, status_rx) = tokio::sync::watch::channel(DataChannelStatus::Pending);
        // `watch::Sender` isn't `Clone`, so share it across the callbacks via Arc.
        let status_tx = std::sync::Arc::new(status_tx);

        // Capacity-1 bounded channel: `blocking_send` on the network thread
        // applies backpressure when the consumer falls behind. Wrapped in an
        // Option so `on_closed` can drop the sender and signal EOF to the
        // receiver even while the channel object is still alive.
        let (message_tx, message_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1);
        let message_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(message_tx)));

        inner.set_on_open(Some({
            let status_tx = status_tx.clone();
            move || {
                let _ = status_tx.send(DataChannelStatus::Open);
            }
        }));

        inner.set_on_error(Some({
            let status_tx = status_tx.clone();
            move |err: &str| {
                let _ = status_tx.send(DataChannelStatus::Error(err.to_owned()));
            }
        }));

        inner.set_on_closed(Some({
            let status_tx = status_tx.clone();
            let message_tx = message_tx.clone();
            move || {
                let _ = status_tx.send(DataChannelStatus::Closed);
                // Drop the sender so a blocked/idle `receive()` observes EOF.
                *message_tx.lock().unwrap() = None;
            }
        }));

        inner.set_on_message(Some({
            let message_tx = message_tx.clone();
            move |msg: &[u8]| {
                // Clone the sender out from under the lock so we never hold it
                // across a (potentially blocking) send.
                let tx = message_tx.lock().unwrap().clone();
                if let Some(tx) = tx {
                    let _ = tx.blocking_send(msg.to_vec());
                }
            }
        }));

        DataChannel {
            sender: DataChannelSender { inner, status_rx },
            receiver: DataChannelReceiver { message_rx },
        }
    }

    pub async fn send(&mut self, msg: &[u8]) -> Result<(), std::io::Error> {
        self.sender.send(msg).await
    }

    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.receiver.receive().await
    }

    pub fn split(self) -> (DataChannelSender, DataChannelReceiver) {
        (self.sender, self.receiver)
    }
}

pub struct DataChannelSender {
    inner: libdatachannel::DataChannel,
    status_rx: tokio::sync::watch::Receiver<DataChannelStatus>,
}

impl DataChannelSender {
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), std::io::Error> {
        // Block on the first send until the channel leaves `Pending` (opens or
        // dies); once it's `Open` this returns immediately on every later send.
        let status = match self
            .status_rx
            .wait_for(|s| !matches!(s, DataChannelStatus::Pending))
            .await
        {
            Ok(s) => s.clone(),
            // The status sender is gone, i.e. the underlying channel was dropped.
            Err(_) => DataChannelStatus::Closed,
        };

        match status {
            DataChannelStatus::Open => {}
            DataChannelStatus::Error(err) => return Err(std::io::Error::other(err)),
            DataChannelStatus::Closed => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))
            }
            DataChannelStatus::Pending => unreachable!("wait_for guarantees we left Pending"),
        }

        self.inner.send(msg).map_err(error_to_io)
    }

    pub fn unsplit(self, receiver: DataChannelReceiver) -> DataChannel {
        DataChannel { sender: self, receiver }
    }
}

pub struct DataChannelReceiver {
    message_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
}

impl DataChannelReceiver {
    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        self.message_rx.recv().await
    }

    pub fn unsplit(self, tx: DataChannelSender) -> DataChannel {
        tx.unsplit(self)
    }
}

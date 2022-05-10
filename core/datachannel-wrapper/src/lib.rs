pub use datachannel::sdp::parse_sdp;
pub use datachannel::sdp::SdpSession;
pub use datachannel::ConnectionState;
pub use datachannel::DataChannelInit;
pub use datachannel::GatheringState;
pub use datachannel::IceCandidate;
pub use datachannel::Reliability;
pub use datachannel::RtcConfig;
pub use datachannel::SdpType;
pub use datachannel::SessionDescription;
pub use datachannel::SignalingState;

pub struct PeerConnection {
    peer_conn: Box<datachannel::RtcPeerConnection<PeerConnectionHandler>>,
    data_channel_rx: tokio::sync::mpsc::Receiver<DataChannel>,
}

impl PeerConnection {
    pub fn new(
        config: RtcConfig,
    ) -> anyhow::Result<(Self, tokio::sync::mpsc::Receiver<PeerConnectionSignal>)> {
        let (signal_tx, signal_rx) = tokio::sync::mpsc::channel(1);
        let (data_channel_tx, data_channel_rx) = tokio::sync::mpsc::channel(1);
        let pch = PeerConnectionHandler {
            signal_tx: signal_tx,
            pending_dc_receiver: None,
            data_channel_tx,
        };
        let peer_conn = datachannel::RtcPeerConnection::new(&config, pch)?;
        Ok((
            PeerConnection {
                peer_conn,
                data_channel_rx,
            },
            signal_rx,
        ))
    }

    pub fn create_data_channel(
        &mut self,
        label: &str,
        dc_init: DataChannelInit,
    ) -> anyhow::Result<DataChannel> {
        let (message_tx, message_rx) = tokio::sync::mpsc::channel(1);
        let (open_tx, open_rx) = tokio::sync::oneshot::channel();
        let state = std::sync::Arc::new(tokio::sync::Mutex::new(DataChannelState {
            open_rx: Some(open_rx),
            error: None,
        }));
        let dch = DataChannelHandler {
            message_tx: Some(message_tx),
            open_tx: Some(open_tx),
            state: state.clone(),
        };
        let dc = self
            .peer_conn
            .create_data_channel_ex(label, dch, &dc_init)?;
        Ok(DataChannel {
            dc,
            message_rx,
            state,
        })
    }

    pub async fn accept(&mut self) -> Option<DataChannel> {
        self.data_channel_rx.recv().await
    }

    pub fn set_local_description(&mut self, sdp_type: SdpType) -> anyhow::Result<()> {
        self.peer_conn.set_local_description(sdp_type)?;
        Ok(())
    }

    pub fn set_remote_description(&mut self, sess_desc: SessionDescription) -> anyhow::Result<()> {
        self.peer_conn.set_remote_description(&sess_desc)?;
        Ok(())
    }

    pub fn local_description(&self) -> Option<SessionDescription> {
        self.peer_conn.local_description()
    }

    pub fn remote_description(&self) -> Option<SessionDescription> {
        self.peer_conn.remote_description()
    }

    pub fn add_remote_candidate(&mut self, cand: IceCandidate) -> anyhow::Result<()> {
        self.peer_conn.add_remote_candidate(&cand)?;
        Ok(())
    }
}

struct PeerConnectionHandler {
    signal_tx: tokio::sync::mpsc::Sender<PeerConnectionSignal>,
    pending_dc_receiver: Option<(
        tokio::sync::mpsc::Receiver<Vec<u8>>,
        std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    )>,
    data_channel_tx: tokio::sync::mpsc::Sender<DataChannel>,
}

#[derive(Debug)]
pub enum PeerConnectionSignal {
    SessionDescription(SessionDescription),
    IceCandidate(IceCandidate),
    ConnectionStateChange(ConnectionState),
    GatheringStateChange(GatheringState),
    SignalingStateChange(SignalingState),
}

impl datachannel::PeerConnectionHandler for PeerConnectionHandler {
    type DCH = DataChannelHandler;

    fn data_channel_handler(&mut self) -> Self::DCH {
        let (message_tx, message_rx) = tokio::sync::mpsc::channel(1);
        let (open_tx, open_rx) = tokio::sync::oneshot::channel();
        let state = std::sync::Arc::new(tokio::sync::Mutex::new(DataChannelState {
            open_rx: Some(open_rx),
            error: None,
        }));
        let dch = DataChannelHandler {
            message_tx: Some(message_tx),
            open_tx: Some(open_tx),
            state: state.clone(),
        };
        self.pending_dc_receiver = Some((message_rx, state));
        dch
    }

    fn on_description(&mut self, sess_desc: SessionDescription) {
        let _ = self
            .signal_tx
            .blocking_send(PeerConnectionSignal::SessionDescription(sess_desc));
    }

    fn on_candidate(&mut self, cand: IceCandidate) {
        let _ = self
            .signal_tx
            .blocking_send(PeerConnectionSignal::IceCandidate(cand));
    }

    fn on_connection_state_change(&mut self, state: ConnectionState) {
        // This can called by the destructor on the same thread that Tokio is running on (horrible).
        let signal = PeerConnectionSignal::ConnectionStateChange(state);
        let _ = match tokio::runtime::Handle::try_current() {
            Ok(_) => {
                // There is no safe way to do this. This occurs during destruction so just completely ignore it.
            }
            Err(_) => {
                let _ = self.signal_tx.blocking_send(signal);
            }
        };
    }

    fn on_gathering_state_change(&mut self, state: GatheringState) {
        let _ = self
            .signal_tx
            .blocking_send(PeerConnectionSignal::GatheringStateChange(state));
    }

    fn on_signaling_state_change(&mut self, state: SignalingState) {
        let _ = self
            .signal_tx
            .blocking_send(PeerConnectionSignal::SignalingStateChange(state));
    }

    fn on_data_channel(&mut self, dc: Box<datachannel::RtcDataChannel<Self::DCH>>) {
        let (message_rx, state) = self.pending_dc_receiver.take().unwrap();
        let _ = self.data_channel_tx.blocking_send(DataChannel {
            dc,
            message_rx,
            state,
        });
    }
}
struct DataChannelState {
    open_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    error: Option<Error>,
}

pub struct DataChannel {
    state: std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    message_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    dc: Box<datachannel::RtcDataChannel<DataChannelHandler>>,
}

async fn dc_send(
    state: &std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    dc: &mut Box<datachannel::RtcDataChannel<DataChannelHandler>>,
    msg: &[u8],
) -> Result<(), Error> {
    let mut state = state.lock().await;
    if let Some(err) = &state.error {
        return Err(err.clone());
    }

    if let Some(open_rx) = state.open_rx.take() {
        open_rx.await.map_err(|_| Error::Closed)?;
    }

    dc.send(msg)
        .map_err(|e| Error::UnderlyingError(format!("{:?}", e)))?;
    Ok(())
}

async fn dc_receive(message_rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>) -> Option<Vec<u8>> {
    message_rx.recv().await
}

impl DataChannel {
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), Error> {
        dc_send(&self.state, &mut self.dc, msg).await
    }

    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        dc_receive(&mut self.message_rx).await
    }

    pub fn split(self) -> (DataChannelReceiver, DataChannelSender) {
        return (
            DataChannelReceiver {
                message_rx: self.message_rx,
            },
            DataChannelSender {
                state: self.state,
                dc: self.dc,
            },
        );
    }
}

pub struct DataChannelSender {
    state: std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    dc: Box<datachannel::RtcDataChannel<DataChannelHandler>>,
}

impl DataChannelSender {
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), Error> {
        dc_send(&self.state, &mut self.dc, msg).await
    }

    pub fn unsplit(self, rx: DataChannelReceiver) -> DataChannel {
        DataChannel {
            state: self.state,
            message_rx: rx.message_rx,
            dc: self.dc,
        }
    }
}

pub struct DataChannelReceiver {
    message_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
}

impl DataChannelReceiver {
    pub async fn receive(&mut self) -> Option<Vec<u8>> {
        dc_receive(&mut self.message_rx).await
    }

    pub fn unsplit(self, tx: DataChannelSender) -> DataChannel {
        tx.unsplit(self)
    }
}

struct DataChannelHandler {
    state: std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    open_tx: Option<tokio::sync::oneshot::Sender<()>>,
    message_tx: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
}

#[derive(Debug, Clone)]
pub enum Error {
    Closed,
    UnderlyingError(String),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl datachannel::DataChannelHandler for DataChannelHandler {
    fn on_open(&mut self) {
        let _ = self.open_tx.take().unwrap().send(());
    }

    fn on_closed(&mut self) {
        self.message_tx = None;
    }

    fn on_error(&mut self, err: &str) {
        let _ = self.state.blocking_lock().error = Some(Error::UnderlyingError(err.to_owned()));
    }

    fn on_message(&mut self, msg: &[u8]) {
        let _ = self
            .message_tx
            .as_mut()
            .unwrap()
            .blocking_send(msg.to_vec());
    }

    fn on_buffered_amount_low(&mut self) {}

    fn on_available(&mut self) {}
}

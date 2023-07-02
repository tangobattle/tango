pub use datachannel::{
    sdp, ConnectionState, DataChannelInit, GatheringState, IceCandidate, Reliability, RtcConfig, SdpType,
    SessionDescription, SignalingState, TransportPolicy,
};

pub struct PeerConnection {
    peer_conn: Box<datachannel::RtcPeerConnection<PeerConnectionHandler>>,
    data_channel_rx: tokio::sync::mpsc::Receiver<DataChannel>,
}

impl PeerConnection {
    pub fn new(config: RtcConfig) -> Result<(Self, tokio::sync::mpsc::Receiver<PeerConnectionEvent>), std::io::Error> {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(1);
        let (data_channel_tx, data_channel_rx) = tokio::sync::mpsc::channel(1);
        let pch = PeerConnectionHandler {
            event_tx,
            pending_dc_receiver: None,
            data_channel_tx,
        };
        let peer_conn = datachannel::RtcPeerConnection::new(&config, pch).map_err(datachannel_error_to_io_error)?;
        Ok((
            PeerConnection {
                peer_conn,
                data_channel_rx,
            },
            event_rx,
        ))
    }

    pub fn create_data_channel(
        &mut self,
        label: &str,
        dc_init: DataChannelInit,
    ) -> Result<DataChannel, std::io::Error> {
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
            .create_data_channel_ex(label, dch, &dc_init)
            .map_err(datachannel_error_to_io_error)?;
        Ok(DataChannel {
            sender: DataChannelSender { state, dc },
            receiver: DataChannelReceiver { message_rx },
        })
    }

    pub async fn accept(&mut self) -> Option<DataChannel> {
        self.data_channel_rx.recv().await
    }

    pub fn set_local_description(&mut self, sdp_type: SdpType) -> Result<(), std::io::Error> {
        self.peer_conn
            .set_local_description(sdp_type)
            .map_err(datachannel_error_to_io_error)?;
        Ok(())
    }

    pub fn set_remote_description(&mut self, sess_desc: SessionDescription) -> Result<(), std::io::Error> {
        self.peer_conn
            .set_remote_description(&sess_desc)
            .map_err(datachannel_error_to_io_error)?;
        Ok(())
    }

    pub fn local_description(&self) -> Option<SessionDescription> {
        self.peer_conn.local_description()
    }

    pub fn remote_description(&self) -> Option<SessionDescription> {
        self.peer_conn.remote_description()
    }

    pub fn add_remote_candidate(&mut self, cand: IceCandidate) -> Result<(), std::io::Error> {
        self.peer_conn
            .add_remote_candidate(&cand)
            .map_err(datachannel_error_to_io_error)?;
        Ok(())
    }
}

struct PeerConnectionHandler {
    event_tx: tokio::sync::mpsc::Sender<PeerConnectionEvent>,
    pending_dc_receiver: Option<(
        tokio::sync::mpsc::Receiver<Vec<u8>>,
        std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    )>,
    data_channel_tx: tokio::sync::mpsc::Sender<DataChannel>,
}

#[derive(Debug)]
pub enum PeerConnectionEvent {
    SessionDescription(SessionDescription),
    IceCandidate(IceCandidate),
    ConnectionStateChange(ConnectionState),
    GatheringStateChange(GatheringState),
    SignalingStateChange(SignalingState),
}

impl datachannel::PeerConnectionHandler for PeerConnectionHandler {
    type DCH = DataChannelHandler;

    fn data_channel_handler(&mut self, _info: datachannel::DataChannelInfo) -> Self::DCH {
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
            .event_tx
            .blocking_send(PeerConnectionEvent::SessionDescription(sess_desc));
    }

    fn on_candidate(&mut self, cand: IceCandidate) {
        let _ = self.event_tx.blocking_send(PeerConnectionEvent::IceCandidate(cand));
    }

    fn on_connection_state_change(&mut self, state: ConnectionState) {
        // This can called by the destructor on the same thread that Tokio is running on (horrible).
        let event = PeerConnectionEvent::ConnectionStateChange(state);
        match tokio::runtime::Handle::try_current() {
            Ok(_) => {
                // We're running in an async context, just try send it and if we can't, oh well (this is probably during destruction).
                let _ = self.event_tx.try_send(event);
            }
            Err(_) => {
                // We're not running in an async context, block on sending it.
                let _ = self.event_tx.blocking_send(event);
            }
        }
    }

    fn on_gathering_state_change(&mut self, state: GatheringState) {
        let _ = self
            .event_tx
            .blocking_send(PeerConnectionEvent::GatheringStateChange(state));
    }

    fn on_signaling_state_change(&mut self, state: SignalingState) {
        let _ = self
            .event_tx
            .blocking_send(PeerConnectionEvent::SignalingStateChange(state));
    }

    fn on_data_channel(&mut self, dc: Box<datachannel::RtcDataChannel<Self::DCH>>) {
        let (message_rx, state) = self.pending_dc_receiver.take().unwrap();
        let _ = self.data_channel_tx.blocking_send(DataChannel {
            sender: DataChannelSender { state, dc },
            receiver: DataChannelReceiver { message_rx },
        });
    }
}

struct DataChannelState {
    open_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    error: Option<String>,
}

pub struct DataChannel {
    sender: DataChannelSender,
    receiver: DataChannelReceiver,
}

impl DataChannel {
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
    state: std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    dc: Box<datachannel::RtcDataChannel<DataChannelHandler>>,
}

impl DataChannelSender {
    pub async fn send(&mut self, msg: &[u8]) -> Result<(), std::io::Error> {
        let mut state = self.state.lock().await;
        if let Some(err) = &state.error {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, err.clone()));
        }

        if let Some(open_rx) = state.open_rx.take() {
            open_rx
                .await
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))?;
        }

        self.dc.send(msg).map_err(datachannel_error_to_io_error)?;
        Ok(())
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

struct DataChannelHandler {
    state: std::sync::Arc<tokio::sync::Mutex<DataChannelState>>,
    open_tx: Option<tokio::sync::oneshot::Sender<()>>,
    message_tx: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
}

fn datachannel_error_to_io_error(err: datachannel::Error) -> std::io::Error {
    match err {
        datachannel::Error::InvalidArg => std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid argument"),
        datachannel::Error::Runtime => std::io::Error::new(std::io::ErrorKind::Other, "runtime error"),
        datachannel::Error::NotAvailable => std::io::Error::new(std::io::ErrorKind::WouldBlock, "not available"),
        datachannel::Error::TooSmall => std::io::Error::new(std::io::ErrorKind::InvalidInput, "buffer too small"),
        datachannel::Error::Unkown => std::io::Error::new(std::io::ErrorKind::Other, "unknown"),
        datachannel::Error::BadString(s) => {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("bad string: {}", s))
        }
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
        let _ = self.state.blocking_lock().error = Some(err.to_owned());
    }

    fn on_message(&mut self, msg: &[u8]) {
        let _ = self.message_tx.as_mut().unwrap().blocking_send(msg.to_vec());
    }

    fn on_buffered_amount_low(&mut self) {}

    fn on_available(&mut self) {}
}

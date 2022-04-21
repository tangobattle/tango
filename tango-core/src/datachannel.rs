enum ReceiveState {
    Ready(tokio::sync::mpsc::Receiver<Vec<u8>>),
    Closed,
}

enum SendState {
    Waiting(tokio::sync::oneshot::Receiver<()>),
    Ready,
}

pub struct DataChannel {
    dc: std::sync::Arc<webrtc::data_channel::RTCDataChannel>,
    send_state: tokio::sync::Mutex<SendState>,
    receive_state: tokio::sync::Mutex<ReceiveState>,
}

impl DataChannel {
    pub async fn new(
        dc: std::sync::Arc<webrtc::data_channel::RTCDataChannel>,
    ) -> std::sync::Arc<DataChannel> {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let (opened_sender, send_state) = tokio::sync::oneshot::channel();
        let sender = std::sync::Arc::new(sender);
        let dc2 = std::sync::Arc::new(DataChannel {
            dc,
            send_state: tokio::sync::Mutex::new(SendState::Waiting(send_state)),
            receive_state: tokio::sync::Mutex::new(ReceiveState::Ready(receiver)),
        });
        {
            let dc2 = dc2.clone();
            dc2.dc
                .on_message(Box::new(move |msg| {
                    let sender = sender.clone();
                    Box::pin(async move {
                        sender
                            .send(msg.data.to_vec())
                            .await
                            .expect("receive message");
                    })
                }))
                .await;
        }

        {
            let dc2 = dc2.clone();
            let dc3 = dc2.clone();
            dc2.dc
                .on_close(Box::new(move || {
                    let dc3 = dc3.clone();
                    Box::pin(async move {
                        *dc3.receive_state.lock().await = ReceiveState::Closed;
                    })
                }))
                .await;
        }
        {
            let dc2 = dc2.clone();
            dc2.dc
                .on_open(Box::new(move || {
                    Box::pin(async move {
                        let _ = opened_sender.send(());
                    })
                }))
                .await;
        }
        dc2
    }

    pub async fn send(&self, data: &[u8]) -> Result<usize, webrtc::Error> {
        let mut send_state = self.send_state.lock().await;
        match &mut *send_state {
            SendState::Waiting(receiver) => {
                let _ = receiver.await;
                *send_state = SendState::Ready;
            }
            SendState::Ready => {}
        };
        self.dc.send(&bytes::Bytes::copy_from_slice(data)).await
    }

    pub async fn receive(&self) -> Option<Vec<u8>> {
        match &mut *self.receive_state.lock().await {
            ReceiveState::Closed => None,
            ReceiveState::Ready(receiver) => receiver.recv().await,
        }
    }

    pub async fn close(&self) -> Result<(), webrtc::Error> {
        self.dc.close().await
    }
}

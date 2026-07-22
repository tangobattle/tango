//! tokio-tungstenite signaling socket. The whole socket lifecycle runs
//! as one task on the net runtime ([`super::super::rt`]); the facade
//! talks to it over channels, so the API stays sync-send / async-recv
//! like the browser backend. No mTLS client certificate is attached —
//! matching the web client's identity model, not the desktop's.

use futures::channel::mpsc;
use futures::StreamExt;
use tokio_tungstenite::tungstenite::Message;

use crate::net::rt;

enum Outgoing {
    Frame(Vec<u8>),
    Close,
}

pub struct SignalSocket {
    out_tx: tokio::sync::mpsc::UnboundedSender<Outgoing>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl SignalSocket {
    pub async fn connect(url: &str) -> anyhow::Result<SignalSocket> {
        let url = url.to_owned();
        let url_for_task = url.clone();
        let (open_tx, open_rx) = futures::channel::oneshot::channel::<Result<(), String>>();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Outgoing>();
        let (in_tx, in_rx) = mpsc::unbounded::<Vec<u8>>();

        rt::handle().spawn(async move {
            let url = url_for_task;
            let stream = match tokio_tungstenite::connect_async(&url).await {
                Ok((stream, _resp)) => {
                    let _ = open_tx.send(Ok(()));
                    stream
                }
                Err(e) => {
                    let _ = open_tx.send(Err(e.to_string()));
                    return;
                }
            };
            let (mut sink, mut source) = futures::StreamExt::split(stream);
            loop {
                tokio::select! {
                    out = out_rx.recv() => match out {
                        Some(Outgoing::Frame(bytes)) => {
                            if futures::SinkExt::send(&mut sink, Message::Binary(bytes)).await.is_err() {
                                break;
                            }
                        }
                        Some(Outgoing::Close) | None => {
                            let _ = futures::SinkExt::send(&mut sink, Message::Close(None)).await;
                            break;
                        }
                    },
                    msg = source.next() => match msg {
                        Some(Ok(Message::Binary(bytes))) => {
                            if in_tx.unbounded_send(bytes.to_vec()).is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                        Some(Ok(_)) => {} // text/ping/pong — tungstenite answers pings itself
                    },
                }
            }
            in_tx.close_channel();
        });

        open_rx
            .await
            .map_err(|_| anyhow::anyhow!("websocket setup dropped"))?
            .map_err(|e| anyhow::anyhow!("can't reach signaling server at {url}: {e}"))?;

        Ok(SignalSocket { out_tx, rx: in_rx })
    }

    pub fn send(&self, bytes: &[u8]) -> anyhow::Result<()> {
        self.out_tx
            .send(Outgoing::Frame(bytes.to_vec()))
            .map_err(|_| anyhow::anyhow!("websocket closed"))
    }

    /// The next binary frame; `None` once the socket has closed.
    pub async fn next(&mut self) -> Option<Vec<u8>> {
        self.rx.next().await
    }

    pub fn close(&self) {
        let _ = self.out_tx.send(Outgoing::Close);
    }
}

impl Drop for SignalSocket {
    fn drop(&mut self) {
        let _ = self.out_tx.send(Outgoing::Close);
    }
}

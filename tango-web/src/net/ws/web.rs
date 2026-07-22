//! The signaling websocket, browser flavor (after gbaroll's):
//! `web_sys::WebSocket` in arraybuffer mode, bridged to an async pull
//! API. `connect` resolves once the socket opens; incoming binary
//! frames queue in an unbounded channel; the channel closing means the
//! socket closed. Keepalives are the caller's job — tango's signaling
//! protocol pings in-band (`Packet.Ping`), not at the socket layer.

use futures::channel::mpsc;
use futures::StreamExt;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

pub struct SignalSocket {
    ws: web_sys::WebSocket,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    /// Keep the JS callbacks alive as long as the socket.
    _closures: Vec<Closure<dyn FnMut(web_sys::Event)>>,
}

impl SignalSocket {
    pub async fn connect(url: &str) -> anyhow::Result<SignalSocket> {
        let ws = web_sys::WebSocket::new(url)
            .map_err(|e| anyhow::anyhow!("can't open a websocket to {url}: {e:?}"))?;
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let (tx, rx) = mpsc::unbounded::<Vec<u8>>();
        let mut closures = Vec::new();

        // The open barrier: exactly one of onopen/onerror/onclose fires
        // first and decides the connect result.
        let (open_tx, open_rx) = futures::channel::oneshot::channel::<Result<(), String>>();
        let open_tx = std::rc::Rc::new(std::cell::RefCell::new(Some(open_tx)));

        {
            let open_tx = open_tx.clone();
            let onopen: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| {
                if let Some(tx) = open_tx.borrow_mut().take() {
                    let _ = tx.send(Ok(()));
                }
            });
            ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            closures.push(onopen);
        }
        {
            let open_tx = open_tx.clone();
            let onerror: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| {
                if let Some(tx) = open_tx.borrow_mut().take() {
                    let _ = tx.send(Err("websocket error".to_owned()));
                }
                // Post-open errors surface as the close that follows.
            });
            ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            closures.push(onerror);
        }
        {
            let tx = tx.clone();
            let open_tx = open_tx.clone();
            let onclose: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| {
                if let Some(otx) = open_tx.borrow_mut().take() {
                    let _ = otx.send(Err("websocket closed before opening".to_owned()));
                }
                tx.close_channel();
            });
            ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            closures.push(onclose);
        }
        {
            let tx = tx.clone();
            let onmessage: Closure<dyn FnMut(web_sys::Event)> =
                Closure::new(move |e: web_sys::Event| {
                    let e: web_sys::MessageEvent = e.unchecked_into();
                    if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                        let _ = tx.unbounded_send(js_sys::Uint8Array::new(&buf).to_vec());
                    }
                });
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            closures.push(onmessage);
        }

        open_rx
            .await
            .map_err(|_| anyhow::anyhow!("websocket setup dropped"))?
            .map_err(|e| anyhow::anyhow!("can't reach signaling server at {url}: {e}"))?;

        Ok(SignalSocket {
            ws,
            rx,
            _closures: closures,
        })
    }

    pub fn send(&self, bytes: &[u8]) -> anyhow::Result<()> {
        self.ws
            .send_with_u8_array(bytes)
            .map_err(|e| anyhow::anyhow!("websocket send: {e:?}"))
    }

    /// The next binary frame; `None` once the socket has closed.
    pub async fn next(&mut self) -> Option<Vec<u8>> {
        self.rx.next().await
    }

    pub fn close(&self) {
        let _ = self.ws.close();
    }
}

impl Drop for SignalSocket {
    fn drop(&mut self) {
        // Detach callbacks before they dangle, then close.
        self.ws.set_onopen(None);
        self.ws.set_onerror(None);
        self.ws.set_onclose(None);
        self.ws.set_onmessage(None);
        let _ = self.ws.close();
    }
}

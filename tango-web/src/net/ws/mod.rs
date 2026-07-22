//! The signaling websocket, one backend per target with the same
//! `SignalSocket` API: `web_sys::WebSocket` in the browser,
//! tokio-tungstenite on native. `connect` resolves once the socket
//! opens; incoming binary frames queue in an unbounded channel; the
//! channel closing means the socket closed. Keepalives are the
//! caller's job — tango's signaling protocol pings in-band
//! (`Packet.Ping`), not at the socket layer.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use web::*;

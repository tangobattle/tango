#[cfg(feature = "client")]
mod client;

#[cfg(feature = "client")]
pub use client::*;

// The `proto` feature exports the protobuf module for out-of-tree
// consumers (the signaling server lives in its own repo); the client
// in this workspace only uses it internally.
#[cfg(feature = "proto")]
pub mod proto;

#[cfg(not(feature = "proto"))]
mod proto;

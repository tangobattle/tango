//! Client for the Tango lobby server (`lobby.proto`): a long-lived,
//! mTLS-authenticated presence + matchmaking websocket.
//!
//! The wire protocol is protobuf; this crate owns the friendly `FriendCode`
//! rendering (Crockford-Base32 + Luhn), which is deliberately a *client-only*
//! concern — the server only ever speaks the raw 10-byte identifier.

pub mod proto;

pub mod friend_code;
pub use friend_code::{FriendCode, FriendCodeError};

// Leaf protobuf types that appear in the public client API.
pub use proto::lobby::{GameInfo, IceServer, MatchProposal, MatchType};

#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
pub use client::*;

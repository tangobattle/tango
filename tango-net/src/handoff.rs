//! Negotiated match terms and the separately owned live connection.

/// Immutable terms established by the ready handshake.
#[derive(Clone)]
pub struct MatchTerms {
    pub is_offerer: bool,
    pub rng_seed: [u8; 16],
    /// The match clock, milliseconds since the unix epoch: the offerer's
    /// commit-time wall clock, identical on both peers. Every core (primary,
    /// shadow, re-sim stepper) pins its cart RTC here so RTC-reading games
    /// (exe45) stay deterministic, and the replay metadata records it as `ts`
    /// so playback pins to the same value.
    pub match_ts: u64,
    pub local_save_data: Vec<u8>,
    pub remote_save_data: Vec<u8>,
    pub local_settings: tango_net_protocol::control::Settings,
    pub remote_settings: tango_net_protocol::control::Settings,
    pub link_code: String,
    pub match_type: (u8, u8),
}

/// A completed lobby handoff. Dropping it also drops the live connection.
pub struct PreMatchData {
    pub terms: MatchTerms,
    pub link_parts: crate::link::LinkParts,
}
impl std::fmt::Debug for PreMatchData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreMatchData").finish_non_exhaustive()
    }
}

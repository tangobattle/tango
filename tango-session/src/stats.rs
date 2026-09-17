//! Host-provided destination for completed statistics. Sessions own no cache paths.

pub trait StatsSink: crate::platform::WasmNotSend + crate::platform::WasmNotSync {
    fn record(&self, replay_key: &std::path::Path, stats: &tango_match::analysis::MatchStats) -> std::io::Result<()>;
}

//! Host-provided destination for completed statistics. Sessions own no cache paths.

pub trait StatsSink: tango_platform::WasmNotSend + tango_platform::WasmNotSync {
    fn record(&self, replay_key: &std::path::Path, stats: &tango_match::analysis::MatchStats) -> std::io::Result<()>;
}

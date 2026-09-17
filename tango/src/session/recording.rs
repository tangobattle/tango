//! Desktop persistence adapters for portable sessions.
use tango_session::pvp::{Recording, ReplayStore};
/// [`ReplayStore`] over a directory: the desktop's, and the behaviour
/// the live match had built in before the trait existed.
pub struct DirReplayStore(pub std::path::PathBuf);

impl ReplayStore for DirReplayStore {
    fn create(&self, name: &str) -> std::io::Result<Recording> {
        std::fs::create_dir_all(&self.0)?;
        let key = self.0.join(format!("{name}.{}", tango_replay::EXTENSION));
        log::info!("pvp: opening replay file {}", key.display());
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&key)?;
        Ok(Recording {
            sink: Box::new(file),
            key,
        })
    }
}

pub struct StatsCache {
    pub cache_path: std::path::PathBuf,
    pub replays_path: std::path::PathBuf,
}
impl tango_session::stats::StatsSink for StatsCache {
    fn record(&self, key: &std::path::Path, stats: &tango_match::analysis::MatchStats) -> std::io::Result<()> {
        tango_library::stats::write_match_stats(
            crate::library::storage(),
            &tango_library::stats::stats_path(&self.cache_path, &self.replays_path, key),
            stats,
        )
    }
}

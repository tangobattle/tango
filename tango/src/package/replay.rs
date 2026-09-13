//! Exact package replay resolution and explicit import of legacy recordings.
use crate::library::{rom, Scanners};
use std::sync::Arc;
use tango_match::gamemode::Configuration;
use tango_script::PreparedGameMode;

pub struct Replay {
    pub metadata: tango_replay::Metadata,
    pub roms: [rom::Id; 2],
    pub seats: [Arc<PreparedGameMode>; 2],
    pub backend: Arc<dyn tango_match::Backend + Send + Sync>,
}

pub fn recorded_name(side: Option<&tango_replay::metadata::Side>) -> Option<String> {
    let configuration = Configuration::decode(&side?.gamemode).ok()?;
    Some(format!(
        "{} · {}",
        configuration.identity.package.name, configuration.identity.export
    ))
}

/// Recorded configurations are already in absolute player order. Reproduce
/// both from installed content before opening playback or an analysis worker.
pub fn resolve(scanners: &Scanners, metadata: &tango_replay::Metadata) -> anyhow::Result<Option<Replay>> {
    let Some(configurations) = metadata.gamemodes()? else {
        return import(scanners, metadata);
    };
    anyhow::ensure!(
        metadata.match_type == 0 && metadata.match_subtype == 0,
        "package replay contains native match settings"
    );
    let roms = scanners.roms.read();
    let catalog = scanners.package_roms.read().unwrap();
    let (p0_game, p0) = catalog.resolve(&configurations[0], &roms).map_err(anyhow::Error::msg)?;
    let (p1_game, p1) = catalog.resolve(&configurations[1], &roms).map_err(anyhow::Error::msg)?;
    Ok(Some(Replay {
        metadata: metadata.clone(),
        roms: [p0_game, p1_game],
        backend: Arc::new(tango_backend_mgba::gamemode::Backend::new([p0.clone(), p1.clone()])?),
        seats: [p0, p1],
    }))
}

#[derive(Clone, PartialEq)]
pub(super) struct LegacyKey {
    pub game: tango_replay::metadata::GameInfo,
    pub match_type: u32,
    pub match_subtype: u32,
}

fn import(scanners: &Scanners, metadata: &tango_replay::Metadata) -> anyhow::Result<Option<Replay>> {
    let games = [metadata.side(0), metadata.side(1)].map(|side| side.and_then(|side| side.game_info.as_ref()));
    let [Some(p0_game), Some(p1_game)] = games else {
        return Ok(None);
    };
    let roms = scanners.roms.read();
    let catalog = scanners.package_roms.read().unwrap();
    let mut seats = Vec::new();
    for game in [p0_game, p1_game] {
        seats.push(
            catalog
                .import_replay(
                    &LegacyKey {
                        game: game.clone(),
                        match_type: metadata.match_type,
                        match_subtype: metadata.match_subtype,
                    },
                    &roms,
                )
                .map_err(anyhow::Error::msg)?,
        );
    }
    if seats.iter().all(Option::is_none) {
        return Ok(None);
    }
    let mut seats = seats.into_iter();
    let (p0_id, p0) = seats
        .next()
        .unwrap()
        .ok_or_else(|| anyhow::anyhow!("cannot import player 1's legacy replay configuration"))?;
    let (p1_id, p1) = seats
        .next()
        .unwrap()
        .ok_or_else(|| anyhow::anyhow!("cannot import player 2's legacy replay configuration"))?;
    let backend = Arc::new(tango_backend_mgba::gamemode::Backend::new([p0.clone(), p1.clone()])?);
    // Normalize only the in-memory copy. The source file remains a legacy
    // recording; a saved import will use the package container version.
    let mut imported = metadata.clone();
    imported.match_type = 0;
    imported.match_subtype = 0;
    for (side, seat) in [&mut imported.p1_side, &mut imported.p2_side]
        .into_iter()
        .zip([&p0, &p1])
    {
        let side = side.as_mut().unwrap();
        side.game_info = None;
        side.gamemode = seat.configuration()?.encode().map_err(anyhow::Error::from_boxed)?;
    }
    imported.gamemodes()?;
    Ok(Some(Replay {
        metadata: imported,
        roms: [p0_id, p1_id],
        seats: [p0, p1],
        backend,
    }))
}

#[cfg(test)]
mod tests;

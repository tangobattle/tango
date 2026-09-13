//! Package gamemode discovery, selection and exact peer resolution.
use std::sync::Arc;

use tango_library::package::ExportRef;
use tango_script::PreparedGameMode;

use super::Choice;
use crate::library::{rom, Scanners};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct Selection {
    key: Option<((u64, u64), Option<rom::Id>, bool, bool)>,
    pub choices: Vec<Choice>,
    pub selected: Option<ExportRef>,
    pub prepared: Option<Arc<PreparedGameMode>>,
    pub error: Option<String>,
}

impl Selection {
    pub fn refresh(&mut self, scanners: &Scanners, rom_id: Option<rom::Id>, patched: bool, disable_bgm: bool) {
        let catalog = scanners.package_roms.read().unwrap();
        let key = (catalog.revision, rom_id, patched, disable_bgm);
        if self.key == Some(key) {
            return;
        }
        let same_rom = self.key.is_some_and(|key| key.1 == rom_id && key.2 == patched);
        self.key = Some(key);
        self.choices = if patched {
            Vec::new()
        } else {
            rom_id
                .and_then(|id| catalog.choices.get(&id))
                .cloned()
                .unwrap_or_default()
        };
        if !same_rom || self.selected.is_none() {
            self.selected = self
                .choices
                .iter()
                .find(|choice| {
                    choice.profile.packages().last().unwrap().default_gamemode.as_ref() == Some(&choice.reference.name)
                })
                .map(|choice| choice.reference.clone());
        }
        if self.choices.is_empty() && !same_rom {
            self.selected = None;
        }
        self.prepare(scanners, rom_id, disable_bgm);
    }

    /// Restoring a pinned choice must not silently replace a missing package.
    pub fn restore(&mut self, reference: ExportRef, scanners: &Scanners, rom_id: Option<rom::Id>, disable_bgm: bool) {
        self.selected = Some(reference);
        self.prepare(scanners, rom_id, disable_bgm);
    }

    pub fn select(&mut self, reference: ExportRef, scanners: &Scanners, rom_id: Option<rom::Id>, disable_bgm: bool) {
        if !self.choices.iter().any(|choice| choice.reference == reference) {
            return;
        }
        self.selected = Some(reference);
        self.prepare(scanners, rom_id, disable_bgm);
    }

    fn prepare(&mut self, scanners: &Scanners, rom_id: Option<rom::Id>, disable_bgm: bool) {
        self.prepared = None;
        self.error = None;
        let Some(selected) = &self.selected else {
            return;
        };
        let result = (|| {
            let choice = self
                .choices
                .iter()
                .find(|choice| &choice.reference == selected)
                .ok_or("selected gamemode is no longer available")?;
            let roms = scanners.roms.read();
            let rom = rom_id
                .and_then(|id| roms.image(id))
                .map(|image| image.bytes.as_ref())
                .ok_or("selected ROM is no longer available")?;
            let prepared = PreparedGameMode::prepare(
                choice.profile.clone(),
                tango_backend_mgba::gamemode::runtime(),
                rom.clone(),
                disable_bgm,
                Default::default(),
            )
            .map_err(|e| e.to_string())?;
            prepared.program(1, [0; 16]).map_err(|e| e.to_string())?;
            Ok::<_, String>(Arc::new(prepared))
        })();
        match result {
            Ok(prepared) => self.prepared = Some(prepared),
            Err(error) => self.error = Some(error),
        }
    }
}

pub fn verdict(
    scanners: &Scanners,
    local: &tango_net_protocol::control::Settings,
    remote: &tango_net_protocol::control::Settings,
) -> crate::netplay::compat::Verdict {
    use crate::netplay::compat::{self, Verdict};
    let roms = scanners.roms.read();
    let (Some(local_mode), Some(remote_mode)) = (&local.gamemode, &remote.gamemode) else {
        return compat::check(local, remote, &roms, &scanners.patches.read());
    };
    let (a, b) = (&local_mode.identity, &remote_mode.identity);
    if [local, remote]
        .iter()
        .any(|settings| settings.game_info.as_ref().is_some_and(|game| game.patch.is_some()))
    {
        return Verdict::DifferentVersions;
    }
    if a.package != b.package || a.dependencies != b.dependencies || a.engine != b.engine || a.script != b.script {
        return Verdict::DifferentVersions;
    }
    if a.export != b.export || local.match_type != 0 || remote.match_type != 0 {
        return Verdict::DifferentMatchTypes;
    }
    let catalog = scanners.package_roms.read().unwrap();
    if catalog.resolve(local_mode, &roms).is_err() || catalog.resolve(remote_mode, &roms).is_err() {
        return Verdict::DifferentVersions;
    }
    Verdict::Compatible
}

pub struct Match {
    pub local_rom: rom::Id,
    pub remote_rom: rom::Id,
    pub local: Arc<PreparedGameMode>,
    pub remote: Arc<PreparedGameMode>,
    pub backend: Arc<dyn tango_match::Backend + Send + Sync>,
}

pub fn for_match(scanners: &Scanners, pre_match: &crate::netplay::PreMatchData) -> anyhow::Result<Option<Match>> {
    let (local, remote) = (&pre_match.local_settings, &pre_match.remote_settings);
    if local.gamemode.is_none() && remote.gamemode.is_none() {
        return Ok(None);
    }
    anyhow::ensure!(
        verdict(scanners, local, remote) == crate::netplay::compat::Verdict::Compatible,
        "the committed gamemodes cannot be resolved together"
    );
    anyhow::ensure!(
        [local, remote]
            .iter()
            .all(|s| s.game_info.as_ref().is_none_or(|g| g.patch.is_none())),
        "legacy patches cannot be combined with package gamemodes"
    );
    let roms = scanners.roms.read();
    let catalog = scanners.package_roms.read().unwrap();
    let (local_game, local) = catalog
        .resolve(local.gamemode.as_ref().unwrap(), &roms)
        .map_err(anyhow::Error::msg)?;
    let (remote_game, remote) = catalog
        .resolve(remote.gamemode.as_ref().unwrap(), &roms)
        .map_err(anyhow::Error::msg)?;
    let seats: [Arc<dyn tango_match::gamemode::Factory>; 2] = if pre_match.local_player_index() == 0 {
        [local.clone(), remote.clone()]
    } else {
        [remote.clone(), local.clone()]
    };
    Ok(Some(Match {
        local_rom: local_game,
        remote_rom: remote_game,
        local,
        remote,
        backend: Arc::new(tango_backend_mgba::gamemode::Backend::new(seats)?),
    }))
}

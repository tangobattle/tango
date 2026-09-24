//! Turning a selection — or a peer's committed settings — into exactly
//! what a session boots: the patched ROM, the parsed save, and its
//! validation findings.

use super::{Error, Selection};
use crate::{game, patch, rom, storage::Storage};
use std::path::PathBuf;
use tango_net_protocol::control as protocol;

/// Exact ROM, parsed save, patch overrides, and structured validation findings.
pub struct ResolvedLoadout {
    pub sram: Vec<u8>,
    pub rom: std::sync::Arc<Vec<u8>>,
    pub prepared: tango_gamesupport::PreparedSave,
    pub validation: Box<dyn tango_gamesupport::Validation>,
}

impl ResolvedLoadout {
    /// The SRAM image a linked match boots this seat from: the parsed
    /// save dumped back out in the game's own layout. It can differ from
    /// [`sram`](Self::sram), the bytes as supplied — a dump is sized to
    /// the cart's SRAM, re-masked, and for Battle Chip Challenge carries
    /// only the live file — and it is what matches have always booted.
    pub fn match_sram(&self) -> Vec<u8> {
        self.prepared.save.to_sram_dump()
    }
}

/// Both seats in local/remote order. Player assignment remains the session's job.
pub struct PreparedMatch {
    pub local: ResolvedLoadout,
    pub remote: ResolvedLoadout,
}

/// The catalog resources a preparation operation is allowed to read.
pub struct Resolver<'a> {
    pub storage: &'a dyn Storage,
    pub roms: &'a rom::Scanner,
    pub patches: &'a patch::Scanner,
    pub patches_path: PathBuf,
}
impl Resolver<'_> {
    pub fn resolve(&self, selection: &Selection, snapshot: Option<&[u8]>) -> Result<ResolvedLoadout, Error> {
        let game = selection.game.ok_or(Error::MissingGame)?;
        let bytes = match snapshot {
            Some(bytes) => bytes.to_vec(),
            None => self
                .storage
                .read(selection.save.as_deref().ok_or(Error::MissingSave)?)?,
        };
        let save = game.parse_save(&bytes)?;
        let rom = rom::load(self.storage, self.roms, &self.patches_path, game, selection.patch())?;
        let applied_patch = selection.patch().map(|patch| self.applied_patch(game, patch));
        let prepared = tango_gamesupport_common_dataview::model::prepare(
            game,
            &rom,
            selection.save.clone().unwrap_or_default(),
            save,
            applied_patch,
        );
        let validation = prepared.validate();
        Ok(ResolvedLoadout {
            sram: bytes,
            rom: std::sync::Arc::new(rom),
            prepared,
            validation,
        })
    }
    /// Prepare `save` for the save editor's preview, best-effort: a patch
    /// that fails to apply (not installed, or unreadable) falls back to
    /// the clean ROM, logged, so the save still renders. Simulation input
    /// preparation goes through [`resolve`](Self::resolve), which never
    /// substitutes a clean ROM.
    pub fn preview(
        &self,
        game: rom::GameRef,
        save_path: PathBuf,
        save: tango_gamesupport::BoxedSave,
        patch: Option<(&str, &semver::Version)>,
    ) -> Result<tango_gamesupport::PreparedSave, rom::LoadError> {
        let (rom, applied_patch) = match rom::load(self.storage, self.roms, &self.patches_path, game, patch) {
            Ok(rom) => (rom, patch.map(|patch| self.applied_patch(game, patch))),
            Err(e @ rom::LoadError::Patch { .. }) => {
                log::warn!("{e} for {:?}; previewing the clean ROM", game.family_and_variant());
                (
                    rom::load(self.storage, self.roms, &self.patches_path, game, None)?,
                    None,
                )
            }
            Err(e) => return Err(e),
        };
        Ok(tango_gamesupport_common_dataview::model::prepare(
            game,
            &rom,
            save_path,
            save,
            applied_patch,
        ))
    }

    /// [`preview`](Self::preview) one absolute player seat of a
    /// recording: that seat's game (checked against this build's
    /// simulation version) and recorded patch, and its embedded SRAM.
    pub fn preview_replay_seat(
        &self,
        replay: &tango_replay::Replay,
        player: u8,
    ) -> Result<tango_gamesupport::PreparedSave, Error> {
        let info = replay
            .metadata
            .side(player)
            .ok_or(Error::MissingReplaySeat(player))?
            .game_info
            .as_ref()
            .ok_or(Error::MissingGameInfo)?;
        let game = game::find_for_replay_side(info).map_err(|source| Error::ReplaySeat { player, source })?;
        let sram = replay
            .srams
            .get(player as usize)
            .ok_or(Error::MissingReplaySeat(player))?;
        let save = game.parse_save(sram)?;
        // A recorded version that doesn't parse previews unpatched.
        let patch = info
            .patch
            .as_ref()
            .and_then(|p| Some((p.name.as_str(), semver::Version::parse(&p.version).ok()?)));
        Ok(self.preview(
            game,
            PathBuf::new(),
            save,
            patch.as_ref().map(|(name, version)| (*name, version)),
        )?)
    }

    /// The patch record a prepared save carries, with the catalog's ROM
    /// overrides for `game`.
    fn applied_patch(
        &self,
        game: rom::GameRef,
        (name, version): (&str, &semver::Version),
    ) -> tango_gamesupport::AppliedPatch {
        tango_gamesupport::AppliedPatch {
            name: name.to_owned(),
            version: version.clone(),
            rom_overrides: self
                .patches
                .read()
                .version(name, version)
                .map(|meta| meta.rom_overrides_for(game))
                .unwrap_or_default(),
        }
    }

    pub fn prepare_match(
        &self,
        local: &protocol::Settings,
        remote: &protocol::Settings,
        saves: [&[u8]; 2],
    ) -> Result<PreparedMatch, Error> {
        Ok(PreparedMatch {
            local: self.resolve(
                &Selection::from_game_info(local.game_info.as_ref().ok_or(Error::MissingGameInfo)?)?,
                Some(saves[0]),
            )?,
            remote: self.resolve(
                &Selection::from_game_info(remote.game_info.as_ref().ok_or(Error::MissingGameInfo)?)?,
                Some(saves[1]),
            )?,
        })
    }
}

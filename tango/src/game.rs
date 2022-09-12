use std::any::Any;

use crate::{battle, patch, replayer, rom, save, session, shadow};

mod bn1;
mod bn2;
// mod bn3;
mod bn4;
// mod bn5;
// mod bn6;
mod exe45;

impl PartialEq for &'static (dyn Game + Send + Sync) {
    fn eq(&self, other: &Self) -> bool {
        (*self).type_id() == (*other).type_id()
    }
}

impl Eq for &'static (dyn Game + Send + Sync) {}

impl std::hash::Hash for &'static (dyn Game + Send + Sync) {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (*self).type_id().hash(state)
    }
}

impl std::fmt::Debug for &'static (dyn Game + Send + Sync) {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (*self).type_id().fmt(f)
    }
}

pub const GAMES: &[&'static (dyn Game + Send + Sync)] = &[
    bn1::EXE1,
    bn1::BN1,
    bn2::EXE2,
    bn2::BN2,
    // bn3::EXE3W,
    // bn3::EXE3B,
    // bn3::BN3W,
    // bn3::BN3B,
    bn4::EXE4RS,
    bn4::EXE4BM,
    bn4::BN4RS,
    bn4::BN4BM,
    exe45::EXE45,
    // bn5::EXE5B,
    // bn5::EXE5C,
    // bn5::BN5P,
    // bn5::BN5C,
    // bn6::EXE6G,
    // bn6::EXE6F,
    // bn6::BN6G,
    // bn6::BN6F,
];

pub fn scan_roms(path: &std::path::Path) -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<u8>> {
    let mut roms = std::collections::HashMap::new();

    for entry in walkdir::WalkDir::new(path) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("failed to read entry: {:?}", e);
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();

        let rom = match std::fs::read(path) {
            Ok(rom) => rom,
            Err(e) => {
                log::warn!("{}: {}", path.display(), e);
                continue;
            }
        };

        let game = match detect(&rom) {
            Ok(game) => {
                log::info!("{}: {:?}", path.display(), game.family_and_variant());
                game
            }
            Err(e) => {
                log::warn!("{}: {}", path.display(), e);
                continue;
            }
        };

        roms.insert(game, rom);
    }

    roms
}

pub fn sort_games(lang: &unic_langid::LanguageIdentifier, games: &mut [&'static (dyn Game + Send + Sync)]) {
    games.sort_by_key(|g| {
        (
            if g.language().matches(lang, true, true) { 0 } else { 1 },
            g.family_and_variant(),
        )
    });
}

pub fn sorted_all_games(lang: &unic_langid::LanguageIdentifier) -> Vec<&'static (dyn Game + Send + Sync)> {
    let mut games = GAMES.to_vec();
    sort_games(lang, &mut games);
    games
}

pub fn find_by_family_and_variant(family: &str, variant: u8) -> Option<&'static (dyn Game + Send + Sync)> {
    GAMES
        .iter()
        .find(|game| game.family_and_variant() == (family, variant))
        .map(|g| *g)
}

pub fn find_by_rom_info(code: &[u8; 4], revision: u8) -> Option<&'static (dyn Game + Send + Sync)> {
    GAMES
        .iter()
        .find(|game| game.rom_code_and_revision() == (code, revision))
        .map(|g| *g)
}

pub fn detect(rom: &[u8]) -> Result<&'static (dyn Game + Send + Sync), anyhow::Error> {
    let rom_code = rom
        .get(0xac..0xac + 4)
        .ok_or(anyhow::anyhow!("out of range"))?
        .try_into()?;
    let rom_revision = rom.get(0xbc).ok_or(anyhow::anyhow!("out of range"))?;
    let game = find_by_rom_info(rom_code, *rom_revision).ok_or(anyhow::anyhow!("unknown game"))?;
    let crc32 = crc32fast::hash(rom);
    if crc32 != game.expected_crc32() {
        anyhow::bail!(
            "mismatched crc32: expected {:08x}, got {:08x}",
            game.expected_crc32(),
            crc32
        );
    }
    Ok(game)
}

pub trait Game
where
    Self: Any,
{
    fn family_and_variant(&self) -> (&str, u8);
    fn language(&self) -> unic_langid::LanguageIdentifier;
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8);
    fn expected_crc32(&self) -> u32;
    fn match_types(&self) -> &[usize];
    fn hooks(&self) -> &'static (dyn Hooks + Send + Sync);
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn save::Save + Send + Sync>, anyhow::Error>;
    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn save::Save + Send + Sync>, anyhow::Error>;
    fn load_rom_assets(
        &self,
        _rom: &[u8],
        _wram: &[u8],
        _overrides: &patch::ROMOverrides,
    ) -> Result<Box<dyn rom::Assets + Send + Sync>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
}

pub trait Hooks {
    fn patch(&self, _core: mgba::core::CoreMutRef) {}

    fn common_traps(&self) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn replayer_traps(&self, replayer_state: replayer::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn shadow_traps(&self, shadow_state: shadow::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn primary_traps(
        &self,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
        completion_token: session::CompletionToken,
    ) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn packet_size(&self) -> usize {
        return 0x10;
    }

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn predict_rx(&self, _rx: &mut Vec<u8>) {}
}

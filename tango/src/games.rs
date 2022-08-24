use std::any::Any;

use crate::{battle, replayer, session, shadow};

mod bn1;
mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
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

pub const GAMES: &[&'static (dyn Game + Send + Sync)] = &[
    bn1::EXE1,
    bn1::BN1,
    bn2::EXE2,
    bn2::BN2,
    bn3::EXE3W,
    bn3::EXE3B,
    bn3::BN3W,
    bn3::BN3B,
    bn4::EXE4RS,
    bn4::EXE4BM,
    bn4::BN4RS,
    bn4::BN4BM,
    exe45::EXE45,
    bn5::EXE5B,
    bn5::EXE5C,
    bn5::BN5P,
    bn5::BN5C,
    bn6::EXE6G,
    bn6::EXE6F,
    bn6::BN6G,
    bn6::BN6F,
];

pub fn scan_roms(
    path: &std::path::Path,
) -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<u8>> {
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

pub fn scan_saves(
    path: &std::path::Path,
) -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<std::path::PathBuf>> {
    let mut paths = std::collections::HashMap::new();

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
        let buf = match std::fs::read(path) {
            Ok(buf) => buf,
            Err(e) => {
                log::warn!("{}: {}", path.display(), e);
                continue;
            }
        };

        let mut ok = false;
        let mut errors = vec![];
        for game in GAMES.iter() {
            match game.parse_save(&buf) {
                Ok(_) => {
                    log::info!("{}: {:?}", path.display(), game.family_and_variant());
                    let save_paths = paths.entry(*game).or_insert_with(|| vec![]);
                    save_paths.push(path.to_path_buf());
                    ok = true;
                }
                Err(e) => {
                    errors.push((*game, e));
                }
            }
        }

        if !ok {
            log::warn!(
                "{}:\n{}",
                path.display(),
                errors
                    .iter()
                    .map(|(k, v)| format!("{:?}: {}", k.family_and_variant(), v))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    for (_, saves) in paths.iter_mut() {
        saves.sort();
    }

    paths
}

pub fn sorted_games(
    lang: &unic_langid::LanguageIdentifier,
) -> Vec<&'static (dyn Game + Send + Sync)> {
    let mut games = GAMES.to_vec();
    games.sort_by_key(|g| {
        (
            if g.language().matches(lang, true, true) {
                0
            } else {
                1
            },
            g.family_and_variant(),
        )
    });
    games
}

pub fn find_by_family_and_variant(
    family: &str,
    variant: u32,
) -> Option<&'static (dyn Game + Send + Sync)> {
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

pub trait Save {}

pub trait Game
where
    Self: Any,
{
    fn family_and_variant(&self) -> (&str, u32);
    fn language(&self) -> unic_langid::LanguageIdentifier;
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8);
    fn expected_crc32(&self) -> u32;
    fn hooks(&self) -> &'static (dyn Hooks + Send + Sync);
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn Save>, anyhow::Error>;
}

pub trait Hooks {
    fn patch(&self, _core: mgba::core::CoreMutRef) {}

    fn common_traps(&self) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn replayer_traps(
        &self,
        replayer_state: replayer::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn shadow_traps(
        &self,
        shadow_state: shadow::State,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn primary_traps(
        &self,
        handle: tokio::runtime::Handle,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
        completion_token: session::CompletionToken,
    ) -> Vec<(u32, Box<dyn FnMut(mgba::core::CoreMutRef)>)>;

    fn packet_size(&self) -> usize {
        return 0x10;
    }

    fn prepare_for_fastforward(&self, core: mgba::core::CoreMutRef);

    fn predict_rx(&self, _rx: &mut Vec<u8>) {}
}

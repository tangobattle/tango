use std::{any::Any, io::Read};

use crate::{battle, replayer, rom, save, session, shadow};

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

fn scan_bnlc_steam_roms() -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<u8>> {
    let mut roms = std::collections::HashMap::new();

    let mut steamdir = if let Some(steamdir) = steamlocate::SteamDir::locate() {
        steamdir
    } else {
        return roms;
    };

    let apps = steamdir.apps();

    if let Some(app) = apps.get(&1798010).and_then(|v| v.as_ref()) {
        // Vol 1
        roms.extend(scan_bnlc_rom_archives(&app.path));
    }

    if let Some(app) = apps.get(&1798020).and_then(|v| v.as_ref()) {
        // Vol 2
        roms.extend(scan_bnlc_rom_archives(&app.path));
    }

    roms
}

fn scan_bnlc_rom_archive(
    path: &std::path::Path,
) -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<u8>> {
    log::info!("scanning bnlc archive: {}", path.display());

    let mut roms = std::collections::HashMap::new();
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("failed to open lc archive {}: {}", path.display(), e);
            return roms;
        }
    };
    let mut za = match zip::ZipArchive::new(f) {
        Ok(za) => za,
        Err(e) => {
            log::error!("failed to open lc archive {}: {}", path.display(), e);
            return roms;
        }
    };

    for i in 0..za.len() {
        let mut entry = za.by_index(i).unwrap();

        let entry_path = if let Some(entry_path) = entry.enclosed_name() {
            entry_path.to_owned()
        } else {
            log::error!("bnlc: {}({}): failed to get path name", path.display(), i);
            continue;
        };

        if entry_path.extension() != Some(&std::ffi::OsStr::new("srl")) {
            continue;
        }

        let mut rom = vec![];
        if let Err(e) = entry.read_to_end(&mut rom) {
            log::error!("bnlc: {}/{}: {}", path.display(), entry_path.display(), e);
            continue;
        }
        let game = match detect(&rom) {
            Ok(game) => {
                log::info!(
                    "bnlc: {}/{}: {:?}",
                    path.display(),
                    entry_path.display(),
                    game.family_and_variant()
                );
                game
            }
            Err(e) => {
                log::warn!("bnlc: {}/{}: {}", path.display(), entry_path.display(), e);
                continue;
            }
        };
        roms.insert(game, rom);
    }
    roms
}

fn scan_bnlc_rom_archives(
    lc_path: &std::path::Path,
) -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<u8>> {
    let mut roms = std::collections::HashMap::new();

    let data_path = lc_path.join("exe").join("data");
    let read_dir = match std::fs::read_dir(&data_path) {
        Ok(read_dir) => read_dir,
        Err(e) => {
            log::warn!("bnlc: {}: {}", lc_path.display(), e);
            return roms;
        }
    };
    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::warn!("bnlc: {}: {}", lc_path.display(), e);
                continue;
            }
        };

        let entry_path = entry.path();

        let file_name = if let Some(file_name) = entry_path.file_name() {
            file_name
        } else {
            continue;
        };

        if file_name == std::ffi::OsStr::new("exe.dat")
            || !file_name.to_string_lossy().starts_with("exe")
            || entry_path.extension() != Some(&std::ffi::OsStr::new("dat"))
        {
            continue;
        }

        roms.extend(scan_bnlc_rom_archive(&entry.path()));
    }
    roms
}

fn scan_non_bnlc_roms(path: &std::path::Path) -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<u8>> {
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
                log::warn!("roms folder: {}: {}", path.display(), e);
                continue;
            }
        };

        let game = match detect(&rom) {
            Ok(game) => {
                log::info!("roms folder: {}: {:?}", path.display(), game.family_and_variant());
                game
            }
            Err(e) => {
                log::warn!("roms folder: {}: {}", path.display(), e);
                continue;
            }
        };

        roms.insert(game, rom);
    }

    roms
}

pub fn scan_roms(path: &std::path::Path) -> std::collections::HashMap<&'static (dyn Game + Send + Sync), Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    roms.extend(scan_bnlc_steam_roms());
    roms.extend(scan_non_bnlc_roms(path));
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
        _overrides: &rom::Overrides,
    ) -> Result<Box<dyn rom::Assets + Send + Sync>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
}

pub trait Hooks {
    fn patch(&self, _core: mgba::core::CoreMutRef) {}

    fn common_traps(&self) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn replayer_traps(&self, replayer_state: replayer::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn shadow_traps(&self, shadow_state: shadow::State) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>;

    fn replayer_playback_traps(&self) -> Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)> {
        vec![]
    }

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

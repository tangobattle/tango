use crate::{battle, replayer, session, shadow};

mod bn1;
mod bn2;
mod bn3;
mod bn4;
mod bn5;
mod bn6;
mod exe45;

pub struct ROMList {
    paths: std::collections::HashMap<
        by_address::ByAddress<&'static (dyn Game + Send + Sync)>,
        std::path::PathBuf,
    >,
}

impl ROMList {
    pub fn new(path: &std::path::Path) -> Self {
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
            let game = std::fs::read(path)
                .map_err(|e| e.into())
                .and_then(|buf| detect(&buf));

            let game = match game {
                Ok(game) => {
                    log::info!(
                        "{}: family = {}, variant = {}",
                        path.display(),
                        game.family(),
                        game.variant()
                    );
                    game
                }
                Err(e) => {
                    log::warn!("{}: {}", path.display(), e);
                    continue;
                }
            };

            paths.insert(by_address::ByAddress(game), path.to_owned());
        }

        Self { paths }
    }

    pub fn find_by_game(
        &self,
        game: &'static (dyn Game + Send + Sync),
    ) -> Option<&std::path::Path> {
        self.paths
            .get(&by_address::ByAddress(game))
            .map(|p| p.as_path())
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

pub fn find_by_family_and_variant(
    family: &str,
    variant: u32,
) -> Option<&'static (dyn Game + Send + Sync)> {
    GAMES
        .iter()
        .find(|game| game.family() == family && game.variant() == variant)
        .map(|g| *g)
}

pub fn find_by_rom_info(code: &[u8; 4], revision: u8) -> Option<&'static (dyn Game + Send + Sync)> {
    Some(match (code, revision) {
        (b"AREJ", 0x00) => bn1::EXE1,
        (b"AREE", 0x00) => bn1::BN1,
        (b"AE2J", 0x01) => bn2::EXE2,
        (b"AE2E", 0x00) => bn2::BN2,
        (b"A6BJ", 0x01) => bn3::EXE3W,
        (b"A3XJ", 0x01) => bn3::EXE3B,
        (b"A6BE", 0x00) => bn3::BN3W,
        (b"A3XE", 0x00) => bn3::BN3B,
        (b"B4WJ", 0x01) => bn4::EXE4RS,
        (b"B4BJ", 0x00) => bn4::EXE4BM,
        (b"B4WE", 0x00) => bn4::BN4RS,
        (b"B4BE", 0x00) => bn4::BN4BM,
        (b"BR4J", 0x00) => exe45::EXE45,
        (b"BRBJ", 0x00) => bn5::EXE5B,
        (b"BRKJ", 0x00) => bn5::EXE5C,
        (b"BRBE", 0x00) => bn5::BN5P,
        (b"BRKE", 0x00) => bn5::BN5C,
        (b"BR5J", 0x00) => bn6::EXE6G,
        (b"BR6J", 0x00) => bn6::EXE6F,
        (b"BR5E", 0x00) => bn6::BN6G,
        (b"BR6E", 0x00) => bn6::BN6F,
        _ => {
            return None;
        }
    })
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

pub trait Game {
    fn family(&self) -> &str;
    fn variant(&self) -> u32;
    fn language(&self) -> unic_langid::LanguageIdentifier;
    fn expected_crc32(&self) -> u32;
    fn hooks(&self) -> &'static (dyn Hooks + Send + Sync);
    fn parse_save(&self, data: Vec<u8>) -> Result<Box<dyn Save>, anyhow::Error> {
        anyhow::bail!("not implemented");
    }
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

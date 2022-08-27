mod hooks;
mod save;

use crate::games;

const MATCH_TYPES: &[usize] = &[1, 1];

struct EXE45Impl;
pub const EXE45: &'static (dyn games::Game + Send + Sync) = &EXE45Impl {};

impl games::Game for EXE45Impl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR4J", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe45", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xa646601b
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn games::Hooks + Send + Sync) {
        &hooks::BR4J_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn games::Save>, anyhow::Error> {
        let save = save::Save::new(data)?;
        Ok(Box::new(save))
    }
}

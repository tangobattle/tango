use crate::game;

const MATCH_TYPES: &[usize] = &[2, 2];

struct EXE5BImpl;
pub const EXE5B: &'static (dyn game::Game + Send + Sync) = &EXE5BImpl {};

impl game::Game for EXE5BImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRBJ", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe5", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xc73f23c0
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn5::BRBJ_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn5::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::JP,
                variant: tango_dataview::game::bn5::save::Variant::Protoman,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn5::save::Save::from_wram(
            data,
            tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::JP,
                variant: tango_dataview::game::bn5::save::Variant::Protoman,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn5::rom::Assets::new(
                &tango_dataview::game::bn5::rom::BRBJ_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn5::rom::JA_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_protoman_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_protoman_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct EXE5CImpl;
pub const EXE5C: &'static (dyn game::Game + Send + Sync) = &EXE5CImpl {};

impl game::Game for EXE5CImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRKJ", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe5", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x16842635
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn5::BRKJ_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn5::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::JP,
                variant: tango_dataview::game::bn5::save::Variant::Colonel,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn5::save::Save::from_wram(
            data,
            tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::JP,
                variant: tango_dataview::game::bn5::save::Variant::Colonel,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn5::rom::Assets::new(
                &tango_dataview::game::bn5::rom::BRKJ_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn5::rom::JA_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_colonel_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_colonel_jp.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::JP,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN5PImpl;
pub const BN5P: &'static (dyn game::Game + Send + Sync) = &BN5PImpl {};

impl game::Game for BN5PImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRBE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn5", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa73e83a4
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn5::BRBE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn5::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::US,
                variant: tango_dataview::game::bn5::save::Variant::Protoman,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn5::save::Save::from_wram(
            data,
            tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::US,
                variant: tango_dataview::game::bn5::save::Variant::Protoman,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn5::rom::Assets::new(
                &tango_dataview::game::bn5::rom::BRBE_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn5::rom::EN_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_protoman_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_protoman_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Protoman
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN5CImpl;
pub const BN5C: &'static (dyn game::Game + Send + Sync) = &BN5CImpl {};

impl game::Game for BN5CImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BRKE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn5", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xa552f683
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn5::BRKE_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn5::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::US,
                variant: tango_dataview::game::bn5::save::Variant::Colonel,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn5::save::Save::from_wram(
            data,
            tango_dataview::game::bn5::save::GameInfo {
                region: tango_dataview::game::bn5::save::Region::US,
                variant: tango_dataview::game::bn5::save::Variant::Colonel,
            },
        )?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn5::rom::Assets::new(
                &tango_dataview::game::bn5::rom::BRKE_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn5::rom::EN_CHARSET
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                }),
                rom.to_vec(),
                wram.to_vec(),
            ),
            overrides,
        )))
    }

    fn save_templates(&self) -> &[(&'static str, &(dyn tango_dataview::save::Save + Send + Sync))] {
        lazy_static! {
            static ref DARK_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/dark_colonel_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref LIGHT_SAVE: tango_dataview::game::bn5::save::Save =
                tango_dataview::game::bn5::save::Save::from_wram(
                    include_bytes!("bn5/save/light_colonel_us.raw"),
                    tango_dataview::game::bn5::save::GameInfo {
                        region: tango_dataview::game::bn5::save::Region::US,
                        variant: tango_dataview::game::bn5::save::Variant::Colonel
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("dark", &*DARK_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                ("light", &*LIGHT_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
            ];
        }
        TEMPLATES.as_slice()
    }
}

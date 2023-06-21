use crate::game;

const MATCH_TYPES: &[usize] = &[2, 2];

struct EXE5BImpl;
pub const EXE5B: &'static (dyn game::Game + Send + Sync) = &EXE5BImpl {};

impl game::Game for EXE5BImpl {
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BRBJ_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn5::rom::JA_CHARSET)),
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
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BRKJ_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn5::rom::JA_CHARSET)),
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
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BRBE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn5::rom::EN_CHARSET)),
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
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BRKE_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn5::rom::EN_CHARSET)),
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

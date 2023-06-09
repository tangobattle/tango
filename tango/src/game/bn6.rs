use crate::game;

const MATCH_TYPES: &[usize] = &[1, 1];

struct EXE6GImpl;
pub const EXE6G: &'static (dyn game::Game + Send + Sync) = &EXE6GImpl {};

impl game::Game for EXE6GImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR5J", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe6", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x6285918a
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR5J_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn6::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::JP,
                variant: tango_dataview::game::bn6::save::Variant::Gregar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn6::save::Save::from_wram(
            data,
            tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::JP,
                variant: tango_dataview::game::bn6::save::Variant::Gregar,
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
            tango_dataview::game::bn6::rom::Assets::new(
                &tango_dataview::game::bn6::rom::BR5J_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn6::rom::JA_CHARSET
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
            static ref SAVE: tango_dataview::game::bn6::save::Save = tango_dataview::game::bn6::save::Save::from_wram(
                include_bytes!("bn6/save/g_jp.raw"),
                tango_dataview::game::bn6::save::GameInfo {
                    region: tango_dataview::game::bn6::save::Region::JP,
                    variant: tango_dataview::game::bn6::save::Variant::Gregar,
                }
            )
            .unwrap();
            static ref HEATMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/heatman_g_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref ELECMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/elecman_g_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref SLASHMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/slashman_g_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref ERASEMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/eraseman_g_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref CHARGEMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/chargeman_g_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref PROTOMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/protoman_g_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("megaman", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                (
                    "heatman",
                    &*HEATMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "elecman",
                    &*ELECMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "slashman",
                    &*SLASHMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "eraseman",
                    &*ERASEMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "chargeman",
                    &*CHARGEMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "protoman",
                    &*PROTOMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                )
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct EXE6FImpl;
pub const EXE6F: &'static (dyn game::Game + Send + Sync) = &EXE6FImpl {};

impl game::Game for EXE6FImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR6J", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe6", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0x2dfb603e
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR6J_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn6::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::JP,
                variant: tango_dataview::game::bn6::save::Variant::Falzar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn6::save::Save::from_wram(
            data,
            tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::JP,
                variant: tango_dataview::game::bn6::save::Variant::Falzar,
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
            tango_dataview::game::bn6::rom::Assets::new(
                &tango_dataview::game::bn6::rom::BR6J_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn6::rom::JA_CHARSET
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
            static ref SAVE: tango_dataview::game::bn6::save::Save = tango_dataview::game::bn6::save::Save::from_wram(
                include_bytes!("bn6/save/f_jp.raw"),
                tango_dataview::game::bn6::save::GameInfo {
                    region: tango_dataview::game::bn6::save::Region::JP,
                    variant: tango_dataview::game::bn6::save::Variant::Falzar,
                }
            )
            .unwrap();
            static ref SPOUTMAN: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/spoutman_f_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref TOMAHAWKMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/tomahawkman_f_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref TENGUMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/tenguman_f_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref GROUNDMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/groundman_f_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref DUSTMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/dustman_f_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref PROTOMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/protoman_f_jp.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::JP,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("megaman", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                (
                    "spoutman",
                    &*SPOUTMAN as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "tomahawkman",
                    &*TOMAHAWKMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "tenguman",
                    &*TENGUMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "groundman",
                    &*GROUNDMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "dustman",
                    &*DUSTMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "protoman",
                    &*PROTOMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                )
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN6GImpl;
pub const BN6G: &'static (dyn game::Game + Send + Sync) = &BN6GImpl {};

impl game::Game for BN6GImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR5E", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn6", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x79452182
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR5E_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn6::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::US,
                variant: tango_dataview::game::bn6::save::Variant::Gregar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn6::save::Save::from_wram(
            data,
            tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::US,
                variant: tango_dataview::game::bn6::save::Variant::Gregar,
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
            tango_dataview::game::bn6::rom::Assets::new(
                &tango_dataview::game::bn6::rom::BR5E_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn6::rom::EN_CHARSET
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
            static ref SAVE: tango_dataview::game::bn6::save::Save = tango_dataview::game::bn6::save::Save::from_wram(
                include_bytes!("bn6/save/g_us.raw"),
                tango_dataview::game::bn6::save::GameInfo {
                    region: tango_dataview::game::bn6::save::Region::US,
                    variant: tango_dataview::game::bn6::save::Variant::Gregar,
                }
            )
            .unwrap();
            static ref HEATMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/heatman_g_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref ELECMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/elecman_g_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref SLASHMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/slashman_g_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref ERASEMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/eraseman_g_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref CHARGEMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/chargeman_g_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref PROTOMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/protoman_g_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Gregar,
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("megaman", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                (
                    "heatman",
                    &*HEATMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "elecman",
                    &*ELECMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "slashman",
                    &*SLASHMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "eraseman",
                    &*ERASEMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "chargeman",
                    &*CHARGEMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "protoman",
                    &*PROTOMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                )
            ];
        }
        TEMPLATES.as_slice()
    }
}

struct BN6FImpl;
pub const BN6F: &'static (dyn game::Game + Send + Sync) = &BN6FImpl {};

impl game::Game for BN6FImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"BR6E", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn6", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xdee6f2a9
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn tango_pvp::hooks::Hooks + Send + Sync) {
        &tango_pvp::game::bn6::BR6E_00
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn6::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::US,
                variant: tango_dataview::game::bn6::save::Variant::Falzar,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn6::save::Save::from_wram(
            data,
            tango_dataview::game::bn6::save::GameInfo {
                region: tango_dataview::game::bn6::save::Region::US,
                variant: tango_dataview::game::bn6::save::Variant::Falzar,
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
            tango_dataview::game::bn6::rom::Assets::new(
                &tango_dataview::game::bn6::rom::BR6E_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn6::rom::EN_CHARSET
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
            static ref SAVE: tango_dataview::game::bn6::save::Save = tango_dataview::game::bn6::save::Save::from_wram(
                include_bytes!("bn6/save/f_us.raw"),
                tango_dataview::game::bn6::save::GameInfo {
                    region: tango_dataview::game::bn6::save::Region::US,
                    variant: tango_dataview::game::bn6::save::Variant::Falzar,
                }
            )
            .unwrap();
            static ref SPOUTMAN: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/spoutman_f_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref TOMAHAWKMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/tomahawkman_f_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref TENGUMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/tenguman_f_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref GROUNDMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/groundman_f_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref DUSTMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/dustman_f_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref PROTOMAN_SAVE: tango_dataview::game::bn6::save::Save =
                tango_dataview::game::bn6::save::Save::from_wram(
                    include_bytes!("bn6/save/protoman_f_us.raw"),
                    tango_dataview::game::bn6::save::GameInfo {
                        region: tango_dataview::game::bn6::save::Region::US,
                        variant: tango_dataview::game::bn6::save::Variant::Falzar,
                    }
                )
                .unwrap();
            static ref TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
                ("megaman", &*SAVE as &(dyn tango_dataview::save::Save + Send + Sync)),
                (
                    "spoutman",
                    &*SPOUTMAN as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "tomahawkman",
                    &*TOMAHAWKMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "tenguman",
                    &*TENGUMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "groundman",
                    &*GROUNDMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "dustman",
                    &*DUSTMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                ),
                (
                    "protoman",
                    &*PROTOMAN_SAVE as &(dyn tango_dataview::save::Save + Send + Sync)
                )
            ];
        }
        TEMPLATES.as_slice()
    }
}

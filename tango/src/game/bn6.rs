use crate::game;

const MATCH_TYPES: &[usize] = &[1, 1];

struct EXE6GImpl;
pub const EXE6G: &'static (dyn game::Game + Send + Sync) = &EXE6GImpl {};

impl game::Game for EXE6GImpl {
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BR5J_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn6::rom::JA_CHARSET)),
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
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BR6J_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn6::rom::JA_CHARSET)),
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
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BR5E_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn6::rom::EN_CHARSET)),
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
    fn gamedb_entry(&self) -> &tango_gamedb::Game {
        &tango_gamedb::BR6E_00
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
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
                &overrides
                    .charset
                    .as_ref()
                    .map(|charset| std::borrow::Cow::Owned(charset.iter().map(|c| c.as_str()).collect::<Vec<_>>()))
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(tango_dataview::game::bn6::rom::EN_CHARSET)),
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

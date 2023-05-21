mod hooks;

use crate::game;

const MATCH_TYPES: &[usize] = &[4, 1];

lazy_static! {
    static ref HEAT_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_GUTS_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_guts_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_CUSTOM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_custom_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_SHIELD_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_shield_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_TEAM_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_team_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_GROUND_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_ground_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref HEAT_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref AQUA_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref ELEC_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WOOD_BUG_WHITE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_bug_white_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White
            }
        )
        .unwrap();
    static ref WHITE_TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
        ("heat-guts", &*HEAT_GUTS_WHITE_SAVE),
        ("aqua-guts", &*AQUA_GUTS_WHITE_SAVE),
        ("elec-guts", &*ELEC_GUTS_WHITE_SAVE),
        ("wood-guts", &*WOOD_GUTS_WHITE_SAVE),
        ("heat-custom", &*HEAT_CUSTOM_WHITE_SAVE),
        ("aqua-custom", &*AQUA_CUSTOM_WHITE_SAVE),
        ("elec-custom", &*ELEC_CUSTOM_WHITE_SAVE),
        ("wood-custom", &*WOOD_CUSTOM_WHITE_SAVE),
        ("heat-shield", &*HEAT_SHIELD_WHITE_SAVE),
        ("aqua-shield", &*AQUA_SHIELD_WHITE_SAVE),
        ("elec-shield", &*ELEC_SHIELD_WHITE_SAVE),
        ("wood-shield", &*WOOD_SHIELD_WHITE_SAVE),
        ("heat-team", &*HEAT_TEAM_WHITE_SAVE),
        ("aqua-team", &*AQUA_TEAM_WHITE_SAVE),
        ("elec-team", &*ELEC_TEAM_WHITE_SAVE),
        ("wood-team", &*WOOD_TEAM_WHITE_SAVE),
        ("heat-ground", &*HEAT_GROUND_WHITE_SAVE),
        ("aqua-ground", &*AQUA_GROUND_WHITE_SAVE),
        ("elec-ground", &*ELEC_GROUND_WHITE_SAVE),
        ("wood-ground", &*WOOD_GROUND_WHITE_SAVE),
        ("heat-bug", &*HEAT_BUG_WHITE_SAVE),
        ("aqua-bug", &*AQUA_BUG_WHITE_SAVE),
        ("elec-bug", &*ELEC_BUG_WHITE_SAVE),
        ("wood-bug", &*WOOD_BUG_WHITE_SAVE),
    ];
}

lazy_static! {
    static ref HEAT_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_GUTS_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_guts_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_CUSTOM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_custom_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_SHIELD_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_shield_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_TEAM_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_team_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_SHADOW_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_shadow_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref HEAT_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/heat_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref AQUA_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/aqua_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref ELEC_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/elec_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref WOOD_BUG_BLUE_SAVE: tango_dataview::game::bn3::save::Save =
        tango_dataview::game::bn3::save::Save::from_wram(
            include_bytes!("bn3/save/wood_bug_blue_any.raw"),
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue
            }
        )
        .unwrap();
    static ref BLUE_TEMPLATES: Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))> = vec![
        ("heat-guts", &*HEAT_GUTS_BLUE_SAVE),
        ("aqua-guts", &*AQUA_GUTS_BLUE_SAVE),
        ("elec-guts", &*ELEC_GUTS_BLUE_SAVE),
        ("wood-guts", &*WOOD_GUTS_BLUE_SAVE),
        ("heat-custom", &*HEAT_CUSTOM_BLUE_SAVE),
        ("aqua-custom", &*AQUA_CUSTOM_BLUE_SAVE),
        ("elec-custom", &*ELEC_CUSTOM_BLUE_SAVE),
        ("wood-custom", &*WOOD_CUSTOM_BLUE_SAVE),
        ("heat-shield", &*HEAT_SHIELD_BLUE_SAVE),
        ("aqua-shield", &*AQUA_SHIELD_BLUE_SAVE),
        ("elec-shield", &*ELEC_SHIELD_BLUE_SAVE),
        ("wood-shield", &*WOOD_SHIELD_WHITE_SAVE),
        ("heat-team", &*HEAT_TEAM_BLUE_SAVE),
        ("aqua-team", &*AQUA_TEAM_BLUE_SAVE),
        ("elec-team", &*ELEC_TEAM_BLUE_SAVE),
        ("wood-team", &*WOOD_TEAM_BLUE_SAVE),
        ("heat-shadow", &*HEAT_SHADOW_BLUE_SAVE),
        ("aqua-shadow", &*AQUA_SHADOW_BLUE_SAVE),
        ("elec-shadow", &*ELEC_SHADOW_BLUE_SAVE),
        ("wood-shadow", &*WOOD_SHADOW_BLUE_SAVE),
        ("heat-bug", &*HEAT_BUG_BLUE_SAVE),
        ("aqua-bug", &*AQUA_BUG_BLUE_SAVE),
        ("elec-bug", &*ELEC_BUG_BLUE_SAVE),
        ("wood-bug", &*WOOD_BUG_BLUE_SAVE),
    ];
}

struct EXE3WImpl;
pub const EXE3W: &'static (dyn game::Game + Send + Sync) = &EXE3WImpl {};

impl game::Game for EXE3WImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A6BJ", 0x01)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe3", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xe48e6bc9
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A6BJ_01
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn3::save::Save::from_wram(
            data,
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White,
            },
        )?))
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn3::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn3::rom::Assets::new(
                &tango_dataview::game::bn3::rom::A6BJ_01,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn3::rom::JA_CHARSET
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
        WHITE_TEMPLATES.as_slice()
    }
}

struct EXE3BImpl;
pub const EXE3B: &'static (dyn game::Game + Send + Sync) = &EXE3BImpl {};

impl game::Game for EXE3BImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A3XJ", 0x01)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("exe3", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn expected_crc32(&self) -> u32 {
        0xfd57493b
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A3XJ_01
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn3::save::Save::from_wram(
            data,
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue,
            },
        )?))
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn3::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn3::rom::Assets::new(
                &tango_dataview::game::bn3::rom::A3XJ_01,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn3::rom::JA_CHARSET
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
        BLUE_TEMPLATES.as_slice()
    }
}

struct BN3WImpl;
pub const BN3W: &'static (dyn game::Game + Send + Sync) = &BN3WImpl {};

impl game::Game for BN3WImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A6BE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn3", 0)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0x0be4410a
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A6BE_00
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn3::save::Save::from_wram(
            data,
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White,
            },
        )?))
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn3::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::White,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn3::rom::Assets::new(
                &tango_dataview::game::bn3::rom::A6BE_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn3::rom::EN_CHARSET
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
        WHITE_TEMPLATES.as_slice()
    }
}

struct BN3BImpl;
pub const BN3B: &'static (dyn game::Game + Send + Sync) = &BN3BImpl {};

impl game::Game for BN3BImpl {
    fn rom_code_and_revision(&self) -> (&[u8; 4], u8) {
        (b"A3XE", 0x00)
    }

    fn family_and_variant(&self) -> (&str, u8) {
        ("bn3", 1)
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn expected_crc32(&self) -> u32 {
        0xc0c780f9
    }

    fn match_types(&self) -> &[usize] {
        MATCH_TYPES
    }

    fn hooks(&self) -> &'static (dyn game::Hooks + Send + Sync) {
        &hooks::A3XE_00
    }

    fn save_from_wram(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        Ok(Box::new(tango_dataview::game::bn3::save::Save::from_wram(
            data,
            tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue,
            },
        )?))
    }

    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Error> {
        let save = tango_dataview::game::bn3::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn3::save::GameInfo {
                variant: tango_dataview::game::bn3::save::Variant::Blue,
            })
        {
            anyhow::bail!("save is not compatible: got {:?}", save.game_info());
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        overrides: &crate::rom::Overrides,
    ) -> Result<Box<dyn tango_dataview::rom::Assets + Send + Sync>, anyhow::Error> {
        Ok(Box::new(crate::rom::OverridenAssets::new(
            tango_dataview::game::bn3::rom::Assets::new(
                &tango_dataview::game::bn3::rom::A3XE_00,
                &overrides.charset.as_ref().cloned().unwrap_or_else(|| {
                    tango_dataview::game::bn3::rom::EN_CHARSET
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
        BLUE_TEMPLATES.as_slice()
    }
}

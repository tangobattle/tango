//! Per-game [`Game`](crate::Game) impls. Each ROM revision is a zero-sized
//! unit struct with a `pub static` instance under its rom-code name. The
//! type name carries a trailing underscore to dodge the naming collision
//! with the static.
//!
//! Game identity is by TypeId (see the `&dyn Game` PartialEq impl in
//! `lib.rs`) — every variant must be its own type, so we deliberately
//! don't share an impl across variants even when the body is identical.

#![allow(non_camel_case_types)]

use crate::{Game, Region};

// ──────── BN1 ────────
pub struct AREJ_00_;
pub static AREJ_00: AREJ_00_ = AREJ_00_;
impl Game for AREJ_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe1", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"AREJ", 0x00)
    }
    fn crc32(&self) -> u32 {
        0xd9516e50
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        let save = tango_dataview::game::bn1::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn1::save::GameInfo {
                region: tango_dataview::game::bn1::save::Region::JP,
            })
        {
            return Err(crate::Error::IncompatibleSave);
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn1::rom::Assets::new(
            &tango_dataview::game::bn1::rom::AREJ_00,
            charset.unwrap_or(tango_dataview::game::bn1::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct AREE_00_;
pub static AREE_00: AREE_00_ = AREE_00_;
impl Game for AREE_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn1", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"AREE", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x1d347971
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        let save = tango_dataview::game::bn1::save::Save::new(data)?;
        if save.game_info()
            != &(tango_dataview::game::bn1::save::GameInfo {
                region: tango_dataview::game::bn1::save::Region::US,
            })
        {
            return Err(crate::Error::IncompatibleSave);
        }
        Ok(Box::new(save))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn1::rom::Assets::new(
            &tango_dataview::game::bn1::rom::AREE_00,
            charset.unwrap_or(tango_dataview::game::bn1::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

// ──────── BN2 ────────
pub struct AE2J_00_AC_;
pub static AE2J_00_AC: AE2J_00_AC_ = AE2J_00_AC_;
impl Game for AE2J_00_AC_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe2", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"AE2J", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x46eed8d
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        Ok(Box::new(tango_dataview::game::bn2::save::Save::new(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn2::rom::Assets::new(
            &tango_dataview::game::bn2::rom::AE2J_00_AC,
            charset.unwrap_or(tango_dataview::game::bn2::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct AE2E_00_;
pub static AE2E_00: AE2E_00_ = AE2E_00_;
impl Game for AE2E_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn2", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"AE2E", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x6d961f82
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        Ok(Box::new(tango_dataview::game::bn2::save::Save::new(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn2::rom::Assets::new(
            &tango_dataview::game::bn2::rom::AE2E_00,
            charset.unwrap_or(tango_dataview::game::bn2::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

fn bn3_parse_save(
    data: &[u8],
    variant: tango_dataview::game::bn3::save::Variant,
) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
    let save = tango_dataview::game::bn3::save::Save::new(data)?;
    if save.game_info() != &(tango_dataview::game::bn3::save::GameInfo { variant }) {
        return Err(crate::Error::IncompatibleSave);
    }
    Ok(Box::new(save))
}

pub struct A6BJ_01_;
pub static A6BJ_01: A6BJ_01_ = A6BJ_01_;
impl Game for A6BJ_01_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe3", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"A6BJ", 0x01)
    }
    fn crc32(&self) -> u32 {
        0xe48e6bc9
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn3_parse_save(data, tango_dataview::game::bn3::save::Variant::White)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn3::rom::Assets::new(
            &tango_dataview::game::bn3::rom::A6BJ_01,
            charset.unwrap_or(tango_dataview::game::bn3::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct A3XJ_01_;
pub static A3XJ_01: A3XJ_01_ = A3XJ_01_;
impl Game for A3XJ_01_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe3", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"A3XJ", 0x01)
    }
    fn crc32(&self) -> u32 {
        0xfd57493b
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn3_parse_save(data, tango_dataview::game::bn3::save::Variant::Blue)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn3::rom::Assets::new(
            &tango_dataview::game::bn3::rom::A3XJ_01,
            charset.unwrap_or(tango_dataview::game::bn3::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct A6BE_00_;
pub static A6BE_00: A6BE_00_ = A6BE_00_;
impl Game for A6BE_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn3", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"A6BE", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x0be4410a
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn3_parse_save(data, tango_dataview::game::bn3::save::Variant::White)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn3::rom::Assets::new(
            &tango_dataview::game::bn3::rom::A6BE_00,
            charset.unwrap_or(tango_dataview::game::bn3::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct A3XE_00_;
pub static A3XE_00: A3XE_00_ = A3XE_00_;
impl Game for A3XE_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn3", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"A3XE", 0x00)
    }
    fn crc32(&self) -> u32 {
        0xc0c780f9
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn3_parse_save(data, tango_dataview::game::bn3::save::Variant::Blue)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn3::rom::Assets::new(
            &tango_dataview::game::bn3::rom::A3XE_00,
            charset.unwrap_or(tango_dataview::game::bn3::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

// ──────── BN4 ────────
fn bn4_parse_save(
    data: &[u8],
    is_jp: bool,
    variant: tango_dataview::game::bn4::save::Variant,
) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
    let save = tango_dataview::game::bn4::save::Save::new(data)?;
    let game_info = save.game_info();
    let region_ok = if is_jp {
        game_info.region.jp
    } else {
        game_info.region.us
    };
    if game_info.variant != variant || !region_ok {
        return Err(crate::Error::IncompatibleSave);
    }
    Ok(Box::new(save))
}

pub struct B4WJ_01_;
pub static B4WJ_01: B4WJ_01_ = B4WJ_01_;
impl Game for B4WJ_01_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe4", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"B4WJ", 0x01)
    }
    fn crc32(&self) -> u32 {
        0xcf0e8b05
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn4_parse_save(data, true, tango_dataview::game::bn4::save::Variant::RedSun)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn4::rom::Assets::new(
            &tango_dataview::game::bn4::rom::B4WJ_01,
            charset.unwrap_or(tango_dataview::game::bn4::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct B4BJ_01_;
pub static B4BJ_01: B4BJ_01_ = B4BJ_01_;
impl Game for B4BJ_01_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe4", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"B4BJ", 0x01)
    }
    fn crc32(&self) -> u32 {
        0x709bbf07
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn4_parse_save(data, true, tango_dataview::game::bn4::save::Variant::BlueMoon)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn4::rom::Assets::new(
            &tango_dataview::game::bn4::rom::B4BJ_01,
            charset.unwrap_or(tango_dataview::game::bn4::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct B4WE_00_;
pub static B4WE_00: B4WE_00_ = B4WE_00_;
impl Game for B4WE_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn4", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"B4WE", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x2120695c
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn4_parse_save(data, false, tango_dataview::game::bn4::save::Variant::RedSun)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn4::rom::Assets::new(
            &tango_dataview::game::bn4::rom::B4WE_00,
            charset.unwrap_or(tango_dataview::game::bn4::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct B4BE_00_;
pub static B4BE_00: B4BE_00_ = B4BE_00_;
impl Game for B4BE_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn4", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"B4BE", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x758a46e9
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn4_parse_save(data, false, tango_dataview::game::bn4::save::Variant::BlueMoon)
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn4::rom::Assets::new(
            &tango_dataview::game::bn4::rom::B4BE_00,
            charset.unwrap_or(tango_dataview::game::bn4::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

// ──────── BN5 ────────
fn bn5_parse_save(
    data: &[u8],
    region: tango_dataview::game::bn5::save::Region,
    variant: tango_dataview::game::bn5::save::Variant,
) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
    let save = tango_dataview::game::bn5::save::Save::new(data)?;
    if save.game_info() != &(tango_dataview::game::bn5::save::GameInfo { region, variant }) {
        return Err(crate::Error::IncompatibleSave);
    }
    Ok(Box::new(save))
}

pub struct BRBJ_00_;
pub static BRBJ_00: BRBJ_00_ = BRBJ_00_;
impl Game for BRBJ_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe5", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BRBJ", 0x00)
    }
    fn crc32(&self) -> u32 {
        0xc73f23c0
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn5_parse_save(
            data,
            tango_dataview::game::bn5::save::Region::JP,
            tango_dataview::game::bn5::save::Variant::Protoman,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn5::rom::Assets::new(
            &tango_dataview::game::bn5::rom::BRBJ_00,
            charset.unwrap_or(tango_dataview::game::bn5::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct BRKJ_00_;
pub static BRKJ_00: BRKJ_00_ = BRKJ_00_;
impl Game for BRKJ_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe5", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BRKJ", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x16842635
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn5_parse_save(
            data,
            tango_dataview::game::bn5::save::Region::JP,
            tango_dataview::game::bn5::save::Variant::Colonel,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn5::rom::Assets::new(
            &tango_dataview::game::bn5::rom::BRKJ_00,
            charset.unwrap_or(tango_dataview::game::bn5::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct BRBE_00_;
pub static BRBE_00: BRBE_00_ = BRBE_00_;
impl Game for BRBE_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn5", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BRBE", 0x00)
    }
    fn crc32(&self) -> u32 {
        0xa73e83a4
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn5_parse_save(
            data,
            tango_dataview::game::bn5::save::Region::US,
            tango_dataview::game::bn5::save::Variant::Protoman,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn5::rom::Assets::new(
            &tango_dataview::game::bn5::rom::BRBE_00,
            charset.unwrap_or(tango_dataview::game::bn5::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct BRKE_00_;
pub static BRKE_00: BRKE_00_ = BRKE_00_;
impl Game for BRKE_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn5", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BRKE", 0x00)
    }
    fn crc32(&self) -> u32 {
        0xa552f683
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn5_parse_save(
            data,
            tango_dataview::game::bn5::save::Region::US,
            tango_dataview::game::bn5::save::Variant::Colonel,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn5::rom::Assets::new(
            &tango_dataview::game::bn5::rom::BRKE_00,
            charset.unwrap_or(tango_dataview::game::bn5::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

// ──────── BN6 ────────
fn bn6_parse_save(
    data: &[u8],
    region: tango_dataview::game::bn6::save::Region,
    variant: tango_dataview::game::bn6::save::Variant,
) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
    let save = tango_dataview::game::bn6::save::Save::new(data)?;
    if save.game_info() != &(tango_dataview::game::bn6::save::GameInfo { region, variant }) {
        return Err(crate::Error::IncompatibleSave);
    }
    Ok(Box::new(save))
}

pub struct BR5J_00_;
pub static BR5J_00: BR5J_00_ = BR5J_00_;
impl Game for BR5J_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe6", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BR5J", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x6285918a
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn6_parse_save(
            data,
            tango_dataview::game::bn6::save::Region::JP,
            tango_dataview::game::bn6::save::Variant::Gregar,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn6::rom::Assets::new(
            &tango_dataview::game::bn6::rom::BR5J_00,
            charset.unwrap_or(tango_dataview::game::bn6::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct BR6J_00_;
pub static BR6J_00: BR6J_00_ = BR6J_00_;
impl Game for BR6J_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe6", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BR6J", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x2dfb603e
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn6_parse_save(
            data,
            tango_dataview::game::bn6::save::Region::JP,
            tango_dataview::game::bn6::save::Variant::Falzar,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn6::rom::Assets::new(
            &tango_dataview::game::bn6::rom::BR6J_00,
            charset.unwrap_or(tango_dataview::game::bn6::rom::JA_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct BR5E_00_;
pub static BR5E_00: BR5E_00_ = BR5E_00_;
impl Game for BR5E_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn6", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BR5E", 0x00)
    }
    fn crc32(&self) -> u32 {
        0x79452182
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn6_parse_save(
            data,
            tango_dataview::game::bn6::save::Region::US,
            tango_dataview::game::bn6::save::Variant::Gregar,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn6::rom::Assets::new(
            &tango_dataview::game::bn6::rom::BR5E_00,
            charset.unwrap_or(tango_dataview::game::bn6::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

pub struct BR6E_00_;
pub static BR6E_00: BR6E_00_ = BR6E_00_;
impl Game for BR6E_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("bn6", 1)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BR6E", 0x00)
    }
    fn crc32(&self) -> u32 {
        0xdee6f2a9
    }
    fn region(&self) -> Region {
        Region::US
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        bn6_parse_save(
            data,
            tango_dataview::game::bn6::save::Region::US,
            tango_dataview::game::bn6::save::Variant::Falzar,
        )
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::bn6::rom::Assets::new(
            &tango_dataview::game::bn6::rom::BR6E_00,
            charset.unwrap_or(tango_dataview::game::bn6::rom::EN_CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

// ──────── EXE4.5 ────────
pub struct BR4J_00_;
pub static BR4J_00: BR4J_00_ = BR4J_00_;
impl Game for BR4J_00_ {
    fn family_and_variant(&self) -> (&'static str, u8) {
        ("exe45", 0)
    }
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (b"BR4J", 0x00)
    }
    fn crc32(&self) -> u32 {
        0xa646601b
    }
    fn region(&self) -> Region {
        Region::JP
    }
    fn parse_save(&self, data: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, crate::Error> {
        Ok(Box::new(tango_dataview::game::exe45::save::Save::new(data)?))
    }

    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync> {
        Box::new(tango_dataview::game::exe45::rom::Assets::new(
            &tango_dataview::game::exe45::rom::BR4J_00,
            charset.unwrap_or(tango_dataview::game::exe45::rom::CHARSET),
            rom.to_vec(),
            wram.to_vec(),
        ))
    }
}

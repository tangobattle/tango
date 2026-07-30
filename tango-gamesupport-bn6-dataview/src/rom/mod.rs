mod msg;
pub mod navicust;

pub struct Offsets {
    chip_data: u32,
    chip_names_pointers: u32,
    chip_descriptions_pointers: u32,
    chip_icon_palette_pointer: u32,
    ncp_data: u32,
    ncp_names_pointer: u32,
    ncp_descriptions_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
    navi_names_pointer: u32,
    emblem_icons_pointer: u32,
    emblem_icons_palette_pointer: u32,
    emblem_icons_offsets_pointer: u32,
    emblem_icons_palette_offsets_pointer: u32,
    patch_card_data: u32,
    patch_card_names_pointer: u32,
    patch_card_details_names_pointer: u32,
    navicust_bg: image::Rgba<u8>,
    /// The link navis this version may equip (MegaMan is 0), grouped into the
    /// rows the navi-edit grid lays them out in. `navi()` returns `None` for
    /// any id outside this set.
    navi_order: &'static [&'static [usize]],
}

const NAVICUST_BG_G: image::Rgba<u8> = image::Rgba([0x08, 0xbd, 0x73, 0xff]);
const NAVICUST_BG_F: image::Rgba<u8> = image::Rgba([0xe7, 0x8c, 0x39, 0xff]);

// Cybeast Gregar and Cybeast Falzar each get their own roster, one row apiece.
const NAVI_ORDER_G: &[&[usize]] = &[&[0, 1, 2, 3, 4, 5, 11]];
const NAVI_ORDER_F: &[&[usize]] = &[&[0, 6, 7, 8, 9, 10, 11]];

#[rustfmt::skip]
pub static BR5J_00: Offsets = Offsets {
    chip_data:                            0x080221bc,
    chip_names_pointers:                  0x08043274,
    chip_descriptions_pointers:           0x08028164,
    chip_icon_palette_pointer:            0x0801f144,
    ncp_data:                             0x081460cc,
    ncp_names_pointer:                    0x08043284,
    ncp_descriptions_pointer:             0x08139240,
    element_icon_palette_pointer:         0x081226e4,
    element_icons_pointer:                0x081226dc,
    navi_names_pointer:                   0x08043290,
    emblem_icons_pointer:                 0x08028594,
    emblem_icons_palette_pointer:         0x08028598,
    emblem_icons_offsets_pointer:         0x080285ac,
    emblem_icons_palette_offsets_pointer: 0x0802859c,
    patch_card_data:                      0x08144778,
    patch_card_names_pointer:             0x08130fe0,
    patch_card_details_names_pointer:     0x08130fec,

    navicust_bg: NAVICUST_BG_G,
    navi_order: NAVI_ORDER_G,
};

#[rustfmt::skip]
pub static BR6J_00: Offsets = Offsets {
    chip_data:                            0x080221bc,
    chip_names_pointers:                  0x080432a4,
    chip_descriptions_pointers:           0x08028164,
    chip_icon_palette_pointer:            0x0801f144,
    ncp_data:                             0x08144300,
    ncp_names_pointer:                    0x080432b4,
    ncp_descriptions_pointer:             0x08137478,
    element_icon_palette_pointer:         0x081213c4,
    element_icons_pointer:                0x081213bc,
    navi_names_pointer:                   0x080432c0,
    emblem_icons_pointer:                 0x08028594,
    emblem_icons_palette_pointer:         0x08028598,
    emblem_icons_offsets_pointer:         0x080285ac,
    emblem_icons_palette_offsets_pointer: 0x0802859c,
    patch_card_data:                      0x081429b0,
    patch_card_names_pointer:             0x0812f218,
    patch_card_details_names_pointer:     0x0812f224,

    navicust_bg: NAVICUST_BG_F,
    navi_order: NAVI_ORDER_F,
};

#[rustfmt::skip]
pub static BR5E_00: Offsets = Offsets {
    chip_data:                            0x08021da8,
    chip_names_pointers:                  0x08042038,
    chip_descriptions_pointers:           0x08027d50,
    chip_icon_palette_pointer:            0x0801ed20,
    ncp_data:                             0x0813b22c,
    ncp_names_pointer:                    0x08042048,
    ncp_descriptions_pointer:             0x08130878,
    element_icon_palette_pointer:         0x0811a9a4,
    element_icons_pointer:                0x0811a99c,
    navi_names_pointer:                   0x08042054,
    emblem_icons_pointer:                 0x08028180,
    emblem_icons_offsets_pointer:         0x08028198,
    emblem_icons_palette_pointer:         0x08028184,
    emblem_icons_palette_offsets_pointer: 0x08028188,
    patch_card_data:                      0,
    patch_card_names_pointer:             0,
    patch_card_details_names_pointer:     0,

    navicust_bg: NAVICUST_BG_G,
    navi_order: NAVI_ORDER_G,
};

#[rustfmt::skip]
pub static BR6E_00: Offsets = Offsets {
    chip_data:                            0x08021da8,
    chip_names_pointers:                  0x08042068,
    chip_descriptions_pointers:           0x08027d50,
    chip_icon_palette_pointer:            0x0801ed20,
    ncp_data:                             0x0813944c,
    ncp_names_pointer:                    0x08042078,
    ncp_descriptions_pointer:             0x0812ea9c,
    element_icon_palette_pointer:         0x08119674,
    element_icons_pointer:                0x0811966c,
    navi_names_pointer:                   0x08042084,
    emblem_icons_pointer:                 0x08028180,
    emblem_icons_offsets_pointer:         0x08028198,
    emblem_icons_palette_pointer:         0x08028184,
    emblem_icons_palette_offsets_pointer: 0x08028188,
    patch_card_data:                      0,
    patch_card_names_pointer:             0,
    patch_card_details_names_pointer:     0,

    navicust_bg: NAVICUST_BG_F,
    navi_order: NAVI_ORDER_F,
};

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
    mapper: tango_gamesupport_common::dataview::rom::MemoryMapper,
    chip_icon_palette: tango_gamesupport_common::dataview::rom::Palette,
    element_icon_palette: tango_gamesupport_common::dataview::rom::Palette,
}

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawChip {
    codes: [u8; 4],
    _attack_element: u8,
    _rarity: u8,
    element: u8,
    class: u8,
    mb: u8,
    effect_flags: [u8; 1],
    _counter_settings: u8,
    _attack_family: u8,
    _attack_subfamily: u8,
    _dark_soul_usage_behavior: u8,
    _unk_0e: u8,
    _lock_on: u8,
    _attack_params: [u8; 4],
    _delay: u8,
    _library_number: u8,
    _library_flags: [u8; 1],
    _lock_on_type: u8,
    _alphabet_sort: u16,
    attack_power: u16,
    library_sort_order: u16,
    _battle_chip_gate_usage: u8,
    _dark_chip_id: u8,
    icon_ptr: u32,
    image_ptr: u32,
    palette_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2c);

impl<'a> Chip<'a> {
    fn raw(&self) -> RawChip {
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.chip_data)[self.id * std::mem::size_of::<RawChip>()..]
                [..std::mem::size_of::<RawChip>()],
        )
    }
}

impl Chip<'_> {
    /// This chip's list icon. `None` when a (patched) ROM's icon
    /// pointer or sheet runs out of mappable range — the trait method
    /// renders those blank instead of panicking.
    fn try_icon(&self) -> Option<image::RgbaImage> {
        let raw = self.raw();
        let tiles = self.assets.mapper.get(raw.icon_ptr);
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common::dataview::rom::apply_palette(
            paletted,
            &self.assets.chip_icon_palette,
        ))
    }

    /// This chip's artwork, same fallible-read rule as
    /// [`try_icon`](Self::try_icon).
    fn try_image(&self) -> Option<image::RgbaImage> {
        let raw = self.raw();
        let tiles = self.assets.mapper.get(raw.image_ptr);
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 7 * 6)?,
            7,
        )
        .ok()?;
        let palette_raw = self.assets.mapper.get(raw.palette_ptr);
        let palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common::dataview::rom::Palette>(
            palette_raw.get(..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>())?,
        );
        Some(tango_gamesupport_common::dataview::rom::apply_palette(
            paletted, &palette,
        ))
    }
}

impl<'a> tango_gamesupport_common::dataview::rom::Chip for Chip<'a> {
    fn name(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common::dataview::msg::get_entry(&region, id)?;

        self.assets
            .msg_parser
            .parse(entry)
            .ok()?
            .into_iter()
            .map(|part| {
                Some(match part {
                    tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                    tango_gamesupport_common::dataview::msg::Chunk::Command(command) => match command {
                        msg::Command::EreaderName(cmd) => {
                            if let Ok(parts) = self.assets.msg_parser.parse(&self.assets.mapper.get(
                                (super::save::EREADER_NAME_OFFSET + cmd.index as usize * super::save::EREADER_NAME_SIZE)
                                    as u32
                                    | 0x02000000,
                            )) {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                                            _ => "".to_string(),
                                        }
                                        .chars()
                                        .collect::<Vec<_>>()
                                    })
                                    .collect::<String>()
                            } else {
                                return None;
                            }
                        }
                        _ => "".to_string(),
                    },
                })
            })
            .collect::<Option<String>>()
    }

    fn description(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_descriptions_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common::dataview::msg::get_entry(&region, id)?;

        self.assets
            .msg_parser
            .parse(entry)
            .ok()?
            .into_iter()
            .map(|part| {
                Some(match part {
                    tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                    tango_gamesupport_common::dataview::msg::Chunk::Command(command) => match command {
                        msg::Command::EreaderDescription(cmd) => {
                            if let Ok(parts) = self.assets.msg_parser.parse(&self.assets.mapper.get(
                                (super::save::EREADER_DESCRIPTION_OFFSET
                                    + cmd.index as usize * super::save::EREADER_DESCRIPTION_SIZE)
                                    as u32
                                    | 0x02000000,
                            )) {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                                            _ => "".to_string(),
                                        }
                                        .chars()
                                        .collect::<Vec<_>>()
                                    })
                                    .collect::<String>()
                            } else {
                                return None;
                            }
                        }
                        _ => "".to_string(),
                    },
                })
            })
            .collect::<Option<String>>()
    }

    fn icon(&self) -> image::RgbaImage {
        // A (patched) ROM whose pointers run out of mappable range
        // renders blank instead of panicking.
        self.try_icon().unwrap_or_else(|| {
            image::RgbaImage::new(
                (2 * tango_gamesupport_common::dataview::rom::TILE_WIDTH) as u32,
                (2 * tango_gamesupport_common::dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn image(&self) -> image::RgbaImage {
        self.try_image().unwrap_or_else(|| {
            image::RgbaImage::new(
                (7 * tango_gamesupport_common::dataview::rom::TILE_WIDTH) as u32,
                (6 * tango_gamesupport_common::dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn codes(&self) -> Vec<char> {
        let raw = self.raw();
        raw.codes
            .iter()
            .cloned()
            .filter(|code| *code != 0xff)
            .map(|code| b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[code as usize] as char)
            .collect()
    }

    fn element(&self) -> usize {
        let raw = self.raw();
        raw.element as usize
    }

    fn class(&self) -> tango_gamesupport_common::dataview::rom::ChipClass {
        let raw = self.raw();
        match raw.class {
            0 => tango_gamesupport_common::dataview::rom::ChipClass::Standard,
            1 => tango_gamesupport_common::dataview::rom::ChipClass::Mega,
            2 => tango_gamesupport_common::dataview::rom::ChipClass::Giga,
            4 => tango_gamesupport_common::dataview::rom::ChipClass::ProgramAdvance,
            _ => tango_gamesupport_common::dataview::rom::ChipClass::None,
        }
    }

    fn dark(&self) -> bool {
        false
    }

    fn mb(&self) -> u8 {
        let raw = self.raw();
        raw.mb
    }

    fn attack_power(&self) -> u32 {
        let raw = self.raw();
        if raw.attack_power < 1000 {
            raw.attack_power as u32
        } else {
            0
        }
    }

    fn library_sort_order(&self) -> Option<usize> {
        let raw = self.raw();
        Some(raw.library_sort_order as usize)
    }
}

struct NavicustPart<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawNavicustPart {
    _unk_00: u8,
    is_solid: u8,
    _unk_02: u8,
    color: u8,
    effect_group: u8,
    _unk_06: [u8; 3],
    uncompressed_bitmap_ptr: u32,
    compressed_bitmap_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x10);

/// Decode a raw navicust-part color byte. Shared with the save layer
/// (the color bar uses the same encoding).
pub fn navicust_part_color(raw: u8) -> Option<tango_gamesupport_common::dataview::rom::NavicustPartColor> {
    use tango_gamesupport_common::dataview::rom::NavicustPartColor as C;
    Some(match raw {
        1 => C::White,
        2 => C::Yellow,
        3 => C::Pink,
        4 => C::Red,
        5 => C::Blue,
        6 => C::Green,
        _ => return None,
    })
}

impl<'a> NavicustPart<'a> {
    fn raw(&'a self) -> RawNavicustPart {
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.ncp_data)[self.id * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        )
    }
}

impl<'a> tango_gamesupport_common::dataview::rom::NavicustPart for NavicustPart<'a> {
    fn name(&self) -> Option<String> {
        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common::dataview::msg::get_entry(&region, self.id / 4)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match &part {
                        tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                        _ => "",
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn description(&self) -> Option<String> {
        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_descriptions_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common::dataview::msg::get_entry(&region, self.id / 4)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn color(&self) -> Option<tango_gamesupport_common::dataview::rom::NavicustPartColor> {
        navicust_part_color(self.raw().color)
    }

    fn is_solid(&self) -> bool {
        let raw = self.raw();
        raw.is_solid == 0
    }

    fn uncompressed_bitmap(&self) -> tango_gamesupport_common::dataview::rom::NavicustBitmap {
        let raw = self.raw();
        ndarray::Array2::from_shape_vec(
            (7, 7),
            self.assets.mapper.get(raw.uncompressed_bitmap_ptr)[..49]
                .iter()
                .map(|x| *x != 0)
                .collect(),
        )
        .unwrap()
    }

    fn compressed_bitmap(&self) -> Option<tango_gamesupport_common::dataview::rom::NavicustBitmap> {
        let raw = self.raw();
        Some(
            ndarray::Array2::from_shape_vec(
                (7, 7),
                self.assets.mapper.get(raw.compressed_bitmap_ptr)[..49]
                    .iter()
                    .map(|x| *x != 0)
                    .collect(),
            )
            .unwrap(),
        )
    }
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[&str], rom: Vec<u8>, wram: Vec<u8>) -> Self {
        let mapper = tango_gamesupport_common::dataview::rom::MemoryMapper::new(rom, wram);

        let chip_icon_palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common::dataview::rom::Palette>(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.chip_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>()],
        );

        let element_icon_palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common::dataview::rom::Palette>(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.element_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>()],
        );

        Self {
            offsets,
            msg_parser: msg::parser(charset),
            mapper,
            chip_icon_palette,
            element_icon_palette,
        }
    }
}

struct PatchCard56<'a> {
    id: usize,
    assets: &'a Assets,
}

impl<'a> PatchCard56<'a> {
    fn raw_header(&self) -> RawPatchCard56Header {
        let buf = self.assets.mapper.get(self.assets.offsets.patch_card_data);
        let [offset, next_offset] = bytemuck::pod_read_unaligned::<[u16; 2]>(
            &buf[self.id * std::mem::size_of::<u16>()..][..std::mem::size_of::<[u16; 2]>()],
        );
        let buf = &buf[offset as usize..next_offset as usize];

        bytemuck::pod_read_unaligned(&buf[0..][..std::mem::size_of::<RawPatchCard56Header>()])
    }

    fn raw_effects(&self) -> Vec<RawPatchCard56Effect> {
        let buf = self.assets.mapper.get(self.assets.offsets.patch_card_data);
        let [offset, next_offset] = bytemuck::pod_read_unaligned::<[u16; 2]>(
            &buf[self.id * std::mem::size_of::<u16>()..][..std::mem::size_of::<[u16; 2]>()],
        );
        let buf = &buf[offset as usize..next_offset as usize];

        buf[std::mem::size_of::<RawPatchCard56Header>()..]
            .chunks(std::mem::size_of::<RawPatchCard56Effect>())
            .map(bytemuck::pod_read_unaligned)
            .collect()
    }
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawPatchCard56Header {
    _unk_00: u8,
    mb: u8,
    _unused: u8,
}
const _: () = assert!(std::mem::size_of::<RawPatchCard56Header>() == 0x3);

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawPatchCard56Effect {
    id: u8,
    parameter: u8,
    is_debuff: u8,
}
const _: () = assert!(std::mem::size_of::<RawPatchCard56Effect>() == 0x3);

impl<'a> tango_gamesupport_common::dataview::rom::PatchCard56 for PatchCard56<'a> {
    fn name(&self) -> Option<String> {
        if self.id == 0 {
            return Some("".to_string());
        }

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.patch_card_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common::dataview::msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn mb(&self) -> u8 {
        if self.id == 0 {
            return 0;
        }

        let header = self.raw_header();
        header.mb
    }

    fn effects(&self) -> Vec<tango_gamesupport_common::dataview::rom::PatchCard56Effect> {
        if self.id == 0 {
            return vec![];
        }

        let effects = self.raw_effects();
        effects
            .into_iter()
            .filter_map(|effect| {
                Some(tango_gamesupport_common::dataview::rom::PatchCard56Effect {
                    id: effect.id as usize,
                    kind: match effect.id {
                        0x00 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpPlus,
                        0x01 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpPlusPercent,
                        0x02 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpMinus,
                        0x03 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpMinusPercent,
                        0x04 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::NormalBody,
                        0x05 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::FireBody,
                        0x06 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::AquaBody,
                        0x07 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::ElecBody,
                        0x08 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::WoodBody,
                        0x09 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::AttackPlus,
                        0x0a => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::AttackMinus,
                        0x0b => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::AttackTimes,
                        0x0c => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::SpeedPlus,
                        0x0d => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::SpeedMinus,
                        0x0e => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::ChargePlus,
                        0x0f => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::ChargeMinus,
                        0x10 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::CustomPlus,
                        0x11 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::CustomMinus,
                        0x12 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MegaFolderPlus,
                        0x13 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MegaFolderMinus,
                        0x14 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::GigaFolderPlus,
                        0x15 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::GigaFolderMinus,
                        0x16 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::SuperArmor,
                        0x17 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::StatusGuard,
                        0x18 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::FloatShoes,
                        0x19 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::AirShoes,
                        0x1a => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::UnderShirt,
                        0x20 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::TripleBuster,
                        0x1b..=0x1f | 0x21 | 0x22 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::BButtonChip,
                        0x23..=0x29 => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::BusterModifier,
                        0x2a..=0x7f => tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::BChargeChip,
                        _ => return None,
                    },
                    name: {
                        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
                            &self
                                .assets
                                .mapper
                                .get(self.assets.offsets.patch_card_details_names_pointer)
                                [..std::mem::size_of::<u32>()],
                        ));
                        tango_gamesupport_common::dataview::msg::get_entry(&region, effect.id as usize)
                            .and_then(|entry| self.assets.msg_parser.parse(entry).ok())
                            .map(|chunks| {
                                chunks
                                    .into_iter()
                                    .flat_map(|chunk| match chunk {
                                        tango_gamesupport_common::dataview::msg::Chunk::Text(s) => {
                                            vec![tango_gamesupport_common::dataview::rom::PatchCard56EffectTemplatePart::String(s)]
                                        }
                                        tango_gamesupport_common::dataview::msg::Chunk::Command(command) => match command {
                                            msg::Command::PrintVar(cmd) => {
                                                vec![tango_gamesupport_common::dataview::rom::PatchCard56EffectTemplatePart::PrintVar(
                                                    cmd.buffer as usize,
                                                )]
                                            }
                                            _ => vec![],
                                        },
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .map(|parts| {
                                parts
                                    .into_iter()
                                    .flat_map(|p| {
                                        match p {
                                            tango_gamesupport_common::dataview::rom::PatchCard56EffectTemplatePart::String(s) => s,
                                            tango_gamesupport_common::dataview::rom::PatchCard56EffectTemplatePart::PrintVar(v) => {
                                                if v == 1 {
                                                    let mut parameter = effect.parameter as u32;
                                                    if effect.id == 0x00 || effect.id == 0x02 {
                                                        parameter *= 10;
                                                    }
                                                    format!("{}", parameter)
                                                } else {
                                                    "".to_string()
                                                }
                                            }
                                        }
                                        .chars()
                                        .collect::<Vec<_>>()
                                    })
                                    .collect()
                            })
                    },
                    parameter: effect.parameter,
                    is_debuff: effect.is_debuff == 1,
                    is_ability: effect.id > 0x15,
                })
            })
            .collect::<Vec<_>>()
    }
}

struct Navi<'a> {
    id: usize,
    assets: &'a Assets,
}

impl Navi<'_> {
    /// This navi's emblem. `None` when a (patched) ROM's pointers or
    /// tables run out of mappable range — the trait method renders
    /// those blank instead of panicking.
    fn try_emblem(&self) -> Option<image::RgbaImage> {
        let icon_offsets = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            self.assets
                .mapper
                .get(self.assets.offsets.emblem_icons_offsets_pointer)
                .get(..std::mem::size_of::<u32>())?,
        ));
        let icon_offset = icon_offsets.get(..super::NUM_NAVIS)?.get(self.id).cloned().unwrap_or(0) as usize;

        let palette_offsets = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            self.assets
                .mapper
                .get(self.assets.offsets.emblem_icons_palette_offsets_pointer)
                .get(..std::mem::size_of::<u32>())?,
        ));
        let palette_offset = palette_offsets
            .get(..super::NUM_NAVIS)?
            .get(self.id)
            .cloned()
            .unwrap_or(0) as usize;

        let tiles = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            self.assets
                .mapper
                .get(self.assets.offsets.emblem_icons_pointer)
                .get(..std::mem::size_of::<u32>())?,
        ));
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            tiles
                .get(tango_gamesupport_common::dataview::rom::TILE_BYTES * 4 * icon_offset..)?
                .get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;

        let palettes = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            self.assets
                .mapper
                .get(self.assets.offsets.emblem_icons_palette_pointer)
                .get(..std::mem::size_of::<u32>())?,
        ));
        let palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common::dataview::rom::Palette>(
            palettes
                .get(std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>() * palette_offset..)?
                .get(..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>())?,
        );
        Some(tango_gamesupport_common::dataview::rom::apply_palette(
            paletted, &palette,
        ))
    }
}

impl<'a> tango_gamesupport_common::dataview::rom::Navi for Navi<'a> {
    fn name(&self) -> Option<String> {
        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.navi_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common::dataview::msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn emblem(&self) -> image::RgbaImage {
        // A (patched) ROM whose pointers run out of mappable range
        // renders blank instead of panicking.
        self.try_emblem().unwrap_or_else(|| {
            image::RgbaImage::new(
                (2 * tango_gamesupport_common::dataview::rom::TILE_WIDTH) as u32,
                (2 * tango_gamesupport_common::dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }
}

impl tango_gamesupport_common::dataview::rom::Assets for Assets {
    fn chip(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common::dataview::rom::Chip + '_>> {
        if id >= self.num_chips() {
            return None;
        }
        Some(Box::new(Chip { id, assets: self }))
    }

    fn num_chips(&self) -> usize {
        super::NUM_CHIPS
    }

    fn num_navis(&self) -> usize {
        super::NUM_NAVIS
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 11 {
            return None;
        }

        let buf = self.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            self.mapper
                .get(self.offsets.element_icons_pointer)
                .get(..std::mem::size_of::<u32>())?,
        ));
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            buf.get(id * tango_gamesupport_common::dataview::rom::TILE_BYTES * 4..)?
                .get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common::dataview::rom::apply_palette(
            paletted,
            &self.element_icon_palette,
        ))
    }

    fn navicust_part(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common::dataview::rom::NavicustPart + '_>> {
        if id >= self.num_navicust_parts() {
            return None;
        }
        Some(Box::new(NavicustPart { id, assets: self }))
    }

    fn num_navicust_parts(&self) -> usize {
        super::NUM_NAVICUST_PARTS
    }

    fn patch_card56(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common::dataview::rom::PatchCard56 + '_>> {
        // id 0 is the "no card" placeholder, not a real patch card.
        if id == 0 || id >= self.num_patch_card56s() {
            return None;
        }
        Some(Box::new(PatchCard56 { id, assets: self }))
    }

    fn num_patch_card56s(&self) -> usize {
        super::NUM_PATCH_CARD56S
    }

    fn navicust_layout(&self) -> Option<tango_gamesupport_common::dataview::rom::NavicustLayout> {
        Some(tango_gamesupport_common::dataview::rom::NavicustLayout {
            command_line: 3,
            has_out_of_bounds: true,
            background: self.offsets.navicust_bg,
        })
    }

    fn navi(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common::dataview::rom::Navi + '_>> {
        if id >= self.num_navis() || !self.offsets.navi_order.iter().any(|row| row.contains(&id)) {
            return None;
        }
        Some(Box::new(Navi { id, assets: self }))
    }

    fn navi_order(&self) -> &[&[usize]] {
        self.offsets.navi_order
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "RV", "BX", "EX", "SP", "FZ", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "-", "×", "=", ":", "%", "?", "+", "█", "[bat]", "ー", "!", "&", ",", "゜", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "�", "_", "ƶ", "[L]", "[B]", "[R]", "[A]", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "[END]", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "×", "緑", "道", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "来", "日", "目", "月", "獣", "各", "人", "入", "出", "山", "口", "光", "電", "気", "綾", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "高", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "究", "門", "城", "王", "兄", "化", "葉", "行", "街", "屋", "水", "見", "終", "新", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "代", "年", "火", "改", "計", "画", "職", "体", "波", "回", "外", "地", "員", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "晴", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "教", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "向", "犬", "々", "ヶ", "連", "射", "舟", "戸", "切", "土", "炎", "伊", "夫", "鉄", "国", "男", "天", "老", "師", "堀", "杉", "士", "悟", "森", "霧", "麻", "剛", "垣", "★", "[bracket1]", "[bracket2]", "[.]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "■", "[bat]", "ー", "!", "RV", "BX", "&", "、", "゜", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "EX", "SP", "FZ", "�", "_", "ƶ", "周", "え", "お", "う", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "[END]", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "×", "緑", "道", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "来", "日", "目", "月", "獣", "各", "人", "入", "出", "山", "口", "光", "電", "気", "綾", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "高", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "究", "門", "城", "王", "兄", "化", "葉", "行", "街", "屋", "水", "見", "終", "新", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "代", "年", "火", "改", "計", "画", "職", "体", "波", "回", "外", "地", "員", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "晴", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "教", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "向", "犬", "々", "ヶ", "連", "射", "舟", "戸", "切", "土", "炎", "伊", "夫", "鉄", "国", "男", "天", "老", "師", "堀", "杉", "士", "悟", "森", "霧", "麻", "剛", "垣"];

pub const SAVE_SIZE: usize = 0x57b0;
pub const GAME_NAME_OFFSET: usize = 0x1e00;
pub const CHECKSUM_OFFSET: usize = 0x1dd8;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Variant {
    White,
    Blue,
}

const fn checksum_start_for_variant(variant: Variant) -> u32 {
    match variant {
        Variant::White => 0x16,
        Variant::Blue => 0x22,
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub variant: Variant,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8]) -> u32 {
    crate::save::compute_raw_checksum(buf, CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, crate::save::Error> {
        let buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(crate::save::Error::InvalidSize(buf.len()))?;

        let n = &buf[GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE3 20021002" && n != b"BBN3 v0.5.0 20021002" {
            return Err(crate::save::Error::InvalidGameName(n.to_vec()));
        }

        let save_checksum = bytemuck::pod_read_unaligned::<u32>(&buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()]);
        let raw_checksum = compute_raw_checksum(&buf);
        let game_info = {
            const WHITE: u32 = checksum_start_for_variant(Variant::White);
            const BLUE: u32 = checksum_start_for_variant(Variant::Blue);
            GameInfo {
                variant: match save_checksum.checked_sub(raw_checksum) {
                    Some(WHITE) => Variant::White,
                    Some(BLUE) => Variant::Blue,
                    _ => {
                        return Err(crate::save::Error::ChecksumMismatch {
                            actual: save_checksum,
                            expected: vec![raw_checksum + WHITE, raw_checksum + BLUE],
                            shift: 0,
                        });
                    }
                },
            }
        };

        let save = Self { buf, game_info };

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, crate::save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(crate::save::Error::InvalidSize(buf.len()))?,
            game_info,
        })
    }

    #[allow(dead_code)]
    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    #[allow(dead_code)]
    pub fn compute_checksum(&self) -> u32 {
        compute_raw_checksum(&self.buf) + checksum_start_for_variant(self.game_info.variant)
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    fn flag(&self, i: usize) -> bool {
        (self.buf[i / 8] & (0b10000000 >> (i % 8))) != 0
    }

    fn set_flag(&mut self, i: usize, v: bool) {
        self.buf[i / 8] = (self.buf[i / 8] & !(0b10000000 >> (i % 8))) | (((v as u8) << 7) >> (i % 8));
    }
}

impl crate::save::Save for Save {
    fn as_raw_wram(&self) -> std::borrow::Cow<[u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn view_chips(&self) -> Option<Box<dyn crate::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<crate::save::NaviView> {
        Some(crate::save::NaviView::Navicust(Box::new(NavicustView { save: self })))
    }

    fn as_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&checksum));
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawChip {
    id: u16,
    code: u16,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x4);

impl<'a> crate::save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        3 // TODO
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x1882] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[0x189d + folder_index];
        if idx >= 30 {
            None
        } else {
            Some(idx as usize)
        }
    }

    fn tag_chip_indexes(&self, _folder_index: usize) -> Option<[usize; 2]> {
        None
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<crate::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x1410
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(crate::save::Chip {
            id: raw.id as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code)?,
        })
    }
}

pub struct NavicustView<'a> {
    save: &'a Save,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawNavicustPart {
    id: u8,
    _unk_01: u8,
    col: u8,
    row: u8,
    rot: u8,
    _unk_05: [u8; 3],
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x8);

impl<'a> crate::save::NavicustView<'a> for NavicustView<'a> {
    fn size(&self) -> [usize; 2] {
        [5, 5]
    }

    fn style(&self) -> Option<usize> {
        Some((self.save.buf[0x1881] & 0x3f) as usize)
    }

    fn navicust_part(&self, i: usize) -> Option<crate::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[0x1300 + i * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        );

        if raw.id == 0 {
            return None;
        }

        Some(crate::save::NavicustPart {
            id: raw.id as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: self.save.flag(0x0310 + raw.id as usize),
        })
    }

    fn materialized(&self) -> crate::navicust::MaterializedNavicust {
        crate::navicust::materialized_from_wram(&self.save.buf[0x1d90..][..(5 * 5)], [5, 5])
    }
}

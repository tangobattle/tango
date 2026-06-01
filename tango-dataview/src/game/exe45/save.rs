use crate::save::LinkNaviView as _;

pub const SAVE_SIZE: usize = 0xc7a8;
pub const MASK_OFFSET: usize = 0x3c84;
pub const GAME_NAME_OFFSET: usize = 0x4ba8;
pub const CHECKSUM_OFFSET: usize = 0x4b88;

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, crate::save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(crate::save::Error::InvalidSize(buf.len()))?;
        crate::save::mask(&mut buf[..], MASK_OFFSET);

        let n = &buf[GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE4RO 040607" && n != b"ROCKMANEXE4RO 041217" {
            return Err(crate::save::Error::InvalidGameName(n.to_vec()));
        }

        let save = Self { buf };
        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(crate::save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                shift: 0,
            });
        }

        Ok(save)
    }

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn compute_checksum(&self) -> u32 {
        crate::save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x38
    }

    pub fn from_wram(buf: &[u8]) -> Result<Self, crate::save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(crate::save::Error::InvalidSize(buf.len()))?,
        })
    }
}

impl crate::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn crate::save::ChipsView<'_> + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn crate::save::ChipsViewMut<'_> + '_>> {
        Some(Box::new(ChipsViewMut { save: self }))
    }

    fn view_navi(&self) -> Option<crate::save::NaviView<'_>> {
        Some(crate::save::NaviView::LinkNavi(Box::new(LinkNaviView { save: self })))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        crate::save::mask(&mut buf[..SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn folder_limits(&self, _assets: &dyn crate::rom::Assets) -> crate::save::FolderLimits {
        crate::save::FolderLimits {
            mega_limit: Some(3),
            giga_limit: Some(1),
            reg_memory: None,
            max_copies: |chip| match chip.class() {
                crate::rom::ChipClass::Mega | crate::rom::ChipClass::Giga => 1,
                crate::rom::ChipClass::Standard => 3,
                _ => 0,
            },
            ..Default::default()
        }
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&checksum));
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    #[bitfield(name = "id", ty = "u16", bits = "0..=8")]
    #[bitfield(name = "code", ty = "u16", bits = "9..=15")]
    id_and_code: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2);

impl<'a> crate::save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        1
    }

    fn equipped_folder_index(&self) -> usize {
        0
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<crate::save::Chip> {
        if folder_index >= 1 || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x7500
                + LinkNaviView { save: self.save }.navi() * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(crate::save::Chip {
            id: raw.id() as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code())?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= super::NUM_PACK_CHIPS {
            return None;
        }
        // counts-first record: buf[base + id*0xc + variant], variant = code position.
        // Unused code slots are 0 padding; a real count never exceeds 99, so treat
        // anything larger as "not owned".
        self.save
            .buf
            .get(0x52c8 + id * 0xc + variant)
            .map(|&b| if b <= 99 { b as usize } else { 0 })
    }
}

pub struct ChipsViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> crate::save::ChipsViewMut<'a> for ChipsViewMut<'a> {
    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: crate::save::Chip) -> bool {
        if folder_index >= 1 || chip_index >= 30 {
            return false;
        }

        let navi = LinkNaviView { save: self.save }.navi();
        self.save.buf
            [0x7500 + navi * (30 * std::mem::size_of::<RawChip>()) + chip_index * std::mem::size_of::<RawChip>()..]
            [..std::mem::size_of::<RawChip>()]
            .copy_from_slice(bytemuck::bytes_of(&{
                let mut raw = RawChip::default();
                raw.set_id(chip.id as u16);
                raw.set_code(chip.code as u16);
                raw
            }));

        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= 1 || chip_index >= 30 {
            return false;
        }

        // 0xffff code reads back as an invalid ChipCode, so `chip()` returns None.
        let navi = LinkNaviView { save: self.save }.navi();
        self.save.buf
            [0x7500 + navi * (30 * std::mem::size_of::<RawChip>()) + chip_index * std::mem::size_of::<RawChip>()..]
            [..std::mem::size_of::<RawChip>()]
            .fill(0xff);

        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= super::NUM_PACK_CHIPS {
            return false;
        }
        if let Some(b) = self.save.buf.get_mut(0x52c8 + id * 0xc + variant) {
            *b = count as u8;
            true
        } else {
            false
        }
    }

    fn rebuild_anticheat(&mut self) {
        // exe45 has no anti-cheat shadow copy of the folder/pack.
    }
}

pub struct LinkNaviView<'a> {
    save: &'a Save,
}

impl<'a> crate::save::LinkNaviView<'a> for LinkNaviView<'a> {
    fn navi(&self) -> usize {
        self.save.buf[0x4ad1] as usize
    }
}

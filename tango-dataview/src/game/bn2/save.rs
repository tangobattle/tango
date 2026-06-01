use crate::save::ChipsView as _;

pub const SAVE_SIZE: usize = 0x3a78;
pub const GAME_NAME_OFFSET: usize = 0x1198;
pub const CHECKSUM_OFFSET: usize = 0x114c;

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, crate::save::Error> {
        let save = Save::from_wram(buf)?;
        let n = &save.buf[GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE2 20011016" {
            return Err(crate::save::Error::InvalidGameName(n.to_vec()));
        }

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

    pub fn from_wram(buf: &[u8]) -> Result<Self, crate::save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(crate::save::Error::InvalidSize(buf.len()))?,
        })
    }

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn compute_checksum(&self) -> u32 {
        crate::save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x16
    }
}

impl crate::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn crate::save::ChipsView<'_> + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn crate::save::ChipsViewMut<'_> + '_>> {
        Some(Box::new(ChipsViewMut { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        buf
    }

    fn folder_limits(&self, _assets: &dyn crate::rom::Assets) -> crate::save::FolderLimits {
        crate::save::FolderLimits {
            max_copies: |_| 10,
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

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawChip {
    id: u16,
    code: u16,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x4);

impl<'a> crate::save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        3
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x0dc2] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        let idx = self.save.buf[0x0ddd + folder_index];
        Some(if idx >= 30 { None } else { Some(idx as usize) })
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<crate::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x0ab0
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(crate::save::Chip {
            id: raw.id as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code)?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= super::NUM_PACK_CHIPS {
            return None;
        }
        // counts-first record: buf[base + id*0x12 + variant], variant = code position.
        // Unused code slots hold 0xff padding; a real count never exceeds 99, so
        // treat anything larger as "not owned".
        self.save
            .buf
            .get(0x11b0 + id * 0x12 + variant)
            .map(|&b| if b <= 99 { b as usize } else { 0 })
    }
}

pub struct ChipsViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> crate::save::ChipsViewMut<'a> for ChipsViewMut<'a> {
    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: crate::save::Chip) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() || chip_index >= 30 {
            return false;
        }

        self.save.buf[0x0ab0
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .copy_from_slice(bytemuck::bytes_of(&RawChip {
                id: chip.id as u16,
                code: chip.code as u16,
            }));

        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() || chip_index >= 30 {
            return false;
        }

        // 0xffff code reads back as an invalid ChipCode, so `chip()` returns None.
        self.save.buf[0x0ab0
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .fill(0xff);

        true
    }

    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: Option<usize>) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() {
            return false;
        }

        // 0xff (out of the 0..30 range) reads back as "no regular".
        let raw = match chip_index {
            Some(i) if i < 30 => i as u8,
            None => 0xff,
            Some(_) => return false,
        };
        self.save.buf[0x0ddd + folder_index] = raw;
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= super::NUM_PACK_CHIPS {
            return false;
        }
        if let Some(b) = self.save.buf.get_mut(0x11b0 + id * 0x12 + variant) {
            *b = count as u8;
            true
        } else {
            false
        }
    }

    fn rebuild_anticheat(&mut self) {
        // BN2 has no anti-cheat shadow copy (introduced in BN4).
    }
}

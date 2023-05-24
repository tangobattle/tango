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
                attempt: 0,
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
        crate::save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x16
    }
}

impl crate::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn crate::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
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

impl<'a> crate::save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        3
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x0dc2] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[0x0ddd + folder_index];
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

        #[repr(packed, C)]
        #[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
        struct RawChip {
            id: u16,
            code: u16,
        }
        const _: () = assert!(std::mem::size_of::<RawChip>() == 0x4);

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x0ab0
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(crate::save::Chip {
            id: raw.id as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[raw.code as usize] as char,
        })
    }
}

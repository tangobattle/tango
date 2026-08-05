use tango_gamesupport_common_dataview::save::ChipsView as _;

pub const SAVE_SIZE: usize = 0x3a78;
pub const GAME_NAME_OFFSET: usize = 0x1198;
pub const CHECKSUM_OFFSET: usize = 0x114c;

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        let save = Save::from_wram(buf)?;
        let n = &save.buf[GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE2 20011016" {
            return Err(tango_gamesupport_common_dataview::save::Error::InvalidGameName(
                n.to_vec(),
            ));
        }

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(tango_gamesupport_common_dataview::save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                shift: 0,
            });
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8]) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(tango_gamesupport_common_dataview::save::Error::InvalidSize(buf.len()))?,
        })
    }

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn compute_checksum(&self) -> u32 {
        tango_gamesupport_common_dataview::save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x16
    }

    /// The current Style Change, packed as `(type << 3) | element` with the
    /// 0x80 "style acquired" flag masked off; the type decodes via this
    /// crate's `rom::style_type`. Stored next to `equipped_folder_index`
    /// (0x0dc2).
    pub fn style(&self) -> usize {
        (self.buf[0x0dc1] & 0x3f) as usize
    }
}

impl tango_gamesupport_common_dataview::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
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

pub struct ChipsView<S> {
    save: S,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawChip {
    id: u16,
    code: u16,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x4);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::ChipsView for ChipsView<S> {
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

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<tango_gamesupport_common_dataview::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x0ab0
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(tango_gamesupport_common_dataview::save::Chip {
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

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_chip(
        &mut self,
        folder_index: usize,
        chip_index: usize,
        chip: tango_gamesupport_common_dataview::save::Chip,
    ) -> bool {
        if folder_index >= self.num_folders() || chip_index >= 30 {
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
        if folder_index >= self.num_folders() || chip_index >= 30 {
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
        if folder_index >= self.num_folders() {
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

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NaviView for NaviView<S> {
    // BN2 has no link-navi roster; the player navi is implicit, so the id is a
    // placeholder the Navi card ignores (the ROM has no navi entry for it).
    fn navi(&self) -> usize {
        0
    }

    fn max_hp(&self, _assets: &dyn tango_gamesupport_common_dataview::rom::Assets) -> u16 {
        bytemuck::pod_read_unaligned::<u16>(&self.save.buf[0x0de2..][..std::mem::size_of::<u16>()])
    }

    fn folder_limits(
        &self,
        _assets: &dyn tango_gamesupport_common_dataview::rom::Assets,
    ) -> tango_gamesupport_common_dataview::save::FolderLimits {
        tango_gamesupport_common_dataview::save::FolderLimits {
            // Regular Memory (raised permanently by RegUP items).
            reg_memory: Some(self.save.buf[0x0dd7]),
            navi_limit: Some(
                if matches!(
                    super::rom::style_type(self.save.style() as u8),
                    super::rom::StyleType::Team | super::rom::StyleType::Hub
                ) {
                    8
                } else {
                    5
                },
            ),
            max_copies: |_| 5,
            ..Default::default()
        }
    }
}

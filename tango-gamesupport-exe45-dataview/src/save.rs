use tango_gamesupport_common_dataview::save::NaviView as _;

pub const SAVE_SIZE: usize = 0xc7a8;
pub const MASK_OFFSET: usize = 0x3c84;
pub const GAME_NAME_OFFSET: usize = 0x4ba8;
pub const CHECKSUM_OFFSET: usize = 0x4b88;

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(tango_gamesupport_common_dataview::save::Error::InvalidSize(buf.len()))?;
        tango_gamesupport_common_dataview::save::mask(&mut buf[..], MASK_OFFSET);

        let n = &buf[GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE4RO 040607" && n != b"ROCKMANEXE4RO 041217" {
            return Err(tango_gamesupport_common_dataview::save::Error::InvalidGameName(
                n.to_vec(),
            ));
        }

        let save = Self { buf };
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

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn compute_checksum(&self) -> u32 {
        tango_gamesupport_common_dataview::save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x38
    }

    pub fn from_wram(buf: &[u8]) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(tango_gamesupport_common_dataview::save::Error::InvalidSize(buf.len()))?,
        })
    }

    /// Base of operated-navi `id`'s per-navi stats block; see [`RawNaviStats`].
    fn navi_stats_offset(&self, id: usize) -> usize {
        0x8542 + id * std::mem::size_of::<RawNaviStats>()
    }

    /// Operated navi `id`'s stats block, decoded.
    fn navi_stats(&self, id: usize) -> RawNaviStats {
        bytemuck::pod_read_unaligned::<RawNaviStats>(
            &self.buf[self.navi_stats_offset(id)..][..std::mem::size_of::<RawNaviStats>()],
        )
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::AnyBitPattern)]
#[allow(dead_code)] // reserved/HP fields are mapped for documentation, not all read
struct RawNaviStats {
    /// Custom-screen draw count. Grows with the navi in vanilla (5 at
    /// base); the bn45_us_pvp patch rebalances it per navi (5-10).
    custom: u8,
    /// MegaChip folder limit for this navi. 5 across the board in
    /// vanilla; the bn45_us_pvp patch rebalances it per navi (MegaMan 7,
    /// Bass 4, everyone else 5). Verified against saves built by both
    /// builds and the patch's published navi tables.
    mega_folder: u8,
    /// GigaChip folder limit (vanilla: 1 everywhere; patch: MegaMan 2).
    giga_folder: u8,
    _stats: [u8; 0x1b],
    base_max_hp: u16,
    current_hp: u16,
    effective_max_hp: u16,
    _rest: [u8; 0x40 - 0x24],
}
const _: () = assert!(std::mem::size_of::<RawNaviStats>() == 0x40);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, mega_folder) == 0x1);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, giga_folder) == 0x2);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, base_max_hp) == 0x1e);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, current_hp) == 0x20);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, effective_max_hp) == 0x22);

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

    fn view_navi_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NaviViewMut + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        tango_gamesupport_common_dataview::save::mask(&mut buf[..SAVE_SIZE], MASK_OFFSET);
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

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    #[bitfield(name = "id", ty = "u16", bits = "0..=8")]
    #[bitfield(name = "code", ty = "u16", bits = "9..=15")]
    id_and_code: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::ChipsView for ChipsView<S> {
    fn num_folders(&self) -> usize {
        1
    }

    fn equipped_folder_index(&self) -> usize {
        0
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<tango_gamesupport_common_dataview::save::Chip> {
        if folder_index >= 1 || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x7500
                + (NaviView { save: &*self.save }).navi() * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(tango_gamesupport_common_dataview::save::Chip {
            id: raw.id() as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code())?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= super::NUM_PACK_CHIPS || variant >= 4 {
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

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        if folder_index >= 1 {
            return None;
        }
        // Per-navi Regular chip: a folder-slot index at +0x1d of the
        // equipped navi's stats block, out-of-range (0xff) = none.
        // Derived by diffing a save before/after setting a Regular
        // in-game under the bn45_us_pvp patch; other saves corroborate
        // (slot indexes / 0xff across navis). The field exists in
        // vanilla progressions too (in-range values observed), so it
        // isn't gated to the patch.
        let navi = (NaviView { save: &*self.save }).navi();
        let v = self.save.buf[self.save.navi_stats_offset(navi) + 0x1d] as usize;
        Some((v < 30).then_some(v))
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_chip(
        &mut self,
        folder_index: usize,
        chip_index: usize,
        chip: tango_gamesupport_common_dataview::save::Chip,
    ) -> bool {
        if folder_index >= 1 || chip_index >= 30 {
            return false;
        }

        let navi = (NaviView { save: &*self.save }).navi();
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
        let navi = (NaviView { save: &*self.save }).navi();
        self.save.buf
            [0x7500 + navi * (30 * std::mem::size_of::<RawChip>()) + chip_index * std::mem::size_of::<RawChip>()..]
            [..std::mem::size_of::<RawChip>()]
            .fill(0xff);

        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= super::NUM_PACK_CHIPS || variant >= 4 {
            return false;
        }
        if let Some(b) = self.save.buf.get_mut(0x52c8 + id * 0xc + variant) {
            *b = count as u8;
            true
        } else {
            false
        }
    }

    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: Option<usize>) -> bool {
        if folder_index >= 1 {
            return false;
        }
        // See `ChipsView::regular_chip_index`: +0x1d of the equipped
        // navi's stats block, 0xff = none.
        let raw = match chip_index {
            Some(i) if i < 30 => i as u8,
            None => 0xff,
            Some(_) => return false,
        };
        let navi = (NaviView { save: &*self.save }).navi();
        let off = self.save.navi_stats_offset(navi) + 0x1d;
        self.save.buf[off] = raw;
        true
    }

    fn rebuild_anticheat(&mut self) {
        // exe45 has no anti-cheat shadow copy of the folder/pack.
    }
}

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NaviView for NaviView<S> {
    fn navi(&self) -> usize {
        self.save.buf[0x4ad1] as usize
    }

    fn max_hp(&self, assets: &dyn tango_gamesupport_common_dataview::rom::Assets) -> u16 {
        assets
            .navi(self.navi())
            .and_then(|navi| navi.base_max_hp())
            .unwrap_or_else(|| self.save.navi_stats(self.navi()).base_max_hp)
    }

    fn folder_limits(
        &self,
        _assets: &dyn tango_gamesupport_common_dataview::rom::Assets,
    ) -> tango_gamesupport_common_dataview::save::FolderLimits {
        // Mega/Giga limits are PER NAVI and live in the save's navi stats
        // block — the game maintains them there, so reading the save is
        // correct for vanilla (5/1 across the board) and for the
        // bn45_us_pvp patch's per-navi rebalances (MegaMan 7/2, Bass 4/1)
        // alike, whatever patch version produced the save. Clamped in
        // case of a corrupted block (0xff padding on empty slots).
        let stats = self.save.navi_stats(self.navi());
        tango_gamesupport_common_dataview::save::FolderLimits {
            mega_limit: Some((stats.mega_folder as usize).min(30)),
            giga_limit: Some((stats.giga_folder as usize).min(30)),
            reg_memory: Some(50),
            max_copies: |chip| match chip.class() {
                tango_gamesupport_common_dataview::rom::ChipClass::Mega
                | tango_gamesupport_common_dataview::rom::ChipClass::Giga => 1,
                // Like the rest of the bn4 engine. The 3 this used to say
                // rejected legal folders: saves built by the in-game
                // editor carry standard chips x4.
                tango_gamesupport_common_dataview::rom::ChipClass::Standard => 4,
                _ => 0,
            },
            ..Default::default()
        }
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::NaviViewMut for NaviView<S> {
    fn set_navi(&mut self, navi: usize) -> bool {
        self.save.buf[0x4ad1] = navi as u8;
        // Operating a navi in-game also loads its HP into the working
        // PlayerData; without this the game keeps the previously-operated
        // navi's HP (wrong HP, and a hang on battle entry). Traced from the
        // ROM: the operate routine writes the navi's effective max HP into
        // the working current/max HP (0x4af0/0x4af2) and the linked stat
        // block (0x4b68/0x4b6a).
        let hp = self.save.navi_stats(navi).effective_max_hp.to_le_bytes();
        for off in [0x4af0usize, 0x4af2, 0x4b68, 0x4b6a] {
            self.save.buf[off..][..2].copy_from_slice(&hp);
        }
        true
    }
}

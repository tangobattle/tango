use byteorder::ByteOrder;

use crate::save;

const SRAM_START_OFFSET: usize = 0x0100;
const SRAM_SIZE: usize = 0x7c14;
const MASK_OFFSET: usize = 0x1a34;
const GAME_NAME_OFFSET: usize = 0x29e0;
const CHECKSUM_OFFSET: usize = 0x29dc;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Variant {
    Protoman,
    Colonel,
}

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub region: Region,
    pub variant: Variant,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
    game_info: GameInfo,
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf: [u8; SRAM_SIZE] = buf
            .get(SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;
        save::mask_save(&mut buf[..], MASK_OFFSET);

        let game_info = match &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20] {
            b"REXE5TOB 20041104 JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Protoman,
            },
            b"REXE5TOK 20041104 JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Colonel,
            },
            b"REXE5TOB 20041006 US" => GameInfo {
                region: Region::US,
                variant: Variant::Protoman,
            },
            b"REXE5TOK 20041006 US" => GameInfo {
                region: Region::US,
                variant: Variant::Colonel,
            },
            n => {
                anyhow::bail!("unknown game name: {:02x?}", n);
            }
        };

        let save = Self { buf, game_info };

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            anyhow::bail!(
                "checksum mismatch: expected {:08x}, got {:08x}",
                save.checksum(),
                computed_checksum
            );
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, anyhow::Error> {
        Ok(Self {
            buf: buf
                .get(..SRAM_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(anyhow::anyhow!("save is wrong size"))?,
            game_info,
        })
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Protoman => 0x72,
                Variant::Colonel => 0x18,
            }
    }
}

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_modcards(&self) -> Option<save::ModcardsView> {
        Some(save::ModcardsView::Modcards56(Box::new(Modcards56View {
            save: self,
        })))
    }

    fn view_dark_ai(&self) -> Option<Box<dyn save::DarkAIView + '_>> {
        Some(Box::new(DarkAIView { save: self }))
    }

    fn as_raw_wram(&self) -> &[u8] {
        &self.buf
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE].copy_from_slice(&self.buf);
        save::mask_save(
            &mut buf[SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE],
            MASK_OFFSET,
        );
        buf
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn chip_codes(&self) -> &'static [u8] {
        &b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[..]
    }

    fn num_folders(&self) -> usize {
        3 // TODO
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x52d5] as usize
    }

    fn regular_chip_is_in_place(&self) -> bool {
        false
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[0x52d6 + folder_index];
        if idx == 0xff {
            None
        } else {
            Some(idx as usize)
        }
    }

    fn tag_chip_indexes(&self, _folder_index: usize) -> Option<[usize; 2]> {
        None
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let offset = 0x2df4 + folder_index * (30 * 2) + chip_index * 2;
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]);

        Some(save::Chip {
            id: (raw & 0x1ff) as usize,
            code: (raw >> 9) as usize,
        })
    }
}

pub struct Modcards56View<'a> {
    save: &'a Save,
}

impl<'a> save::Modcards56View<'a> for Modcards56View<'a> {
    fn count(&self) -> usize {
        self.save.buf[0x79a0] as usize
    }

    fn modcard(&self, slot: usize) -> Option<save::Modcard> {
        if slot >= self.count() {
            return None;
        }
        let raw = self.save.buf[0x79d0 + slot];
        Some(save::Modcard {
            id: (raw & 0x7f) as usize,
            enabled: raw >> 7 == 0,
        })
    }
}

pub struct NavicustView<'a> {
    save: &'a Save,
}

impl<'a> save::NavicustView<'a> for NavicustView<'a> {
    fn width(&self) -> usize {
        5
    }

    fn height(&self) -> usize {
        5
    }

    fn command_line(&self) -> usize {
        2
    }

    fn has_out_of_bounds(&self) -> bool {
        false
    }

    fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
        if i >= 25 {
            return None;
        }

        let buf = &self.save.buf[0x4d6c + i * 8..0x4d6c + (i + 1) * 8];
        let raw = buf[0];
        if raw == 0 {
            return None;
        }

        Some(save::NavicustPart {
            id: (raw / 4) as usize,
            variant: (raw % 4) as usize,
            col: buf[0x2],
            row: buf[0x3],
            rot: buf[0x4],
            compressed: buf[0x5] != 0,
        })
    }
}

pub struct DarkAIView<'a> {
    save: &'a Save,
}

impl<'a> save::DarkAIView<'a> for DarkAIView<'a> {
    fn chip_use_count(&self, id: usize) -> u16 {
        let offset = 0x7340 + id * 2;
        byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2])
    }

    fn secondary_chip_use_count(&self, id: usize) -> u16 {
        let offset = 0x2340 + id * 2;
        byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2])
    }
}

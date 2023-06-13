use crate::save;

pub const SAVE_SIZE: usize = 0x2308;
pub const GAME_NAME_OFFSET: usize = 0x03fc;
pub const CHECKSUM_OFFSET: usize = 0x03f0;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub region: Region,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
    game_info: GameInfo,
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, save::Error> {
        let buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        let game_info = match &buf[GAME_NAME_OFFSET..][..20] {
            b"ROCKMAN EXE 20010120" => GameInfo { region: Region::JP },
            b"ROCKMAN EXE 20010727" => GameInfo { region: Region::US },
            n => {
                return Err(save::Error::InvalidGameName(n.to_vec()));
            }
        };

        let save = Self { buf, game_info };

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                attempt: 0,
                shift: 0,
            });
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(save::Error::InvalidSize(buf.len()))?,
            game_info,
        })
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x16
    }

    #[allow(dead_code)]
    pub fn armor(&self) -> usize {
        self.buf[0x0227] as usize
    }
}

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
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
    id: u8,
    code: u8,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2);

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        1
    }

    fn equipped_folder_index(&self) -> usize {
        0
    }

    fn regular_chip_index(&self, _folder_index: usize) -> Option<usize> {
        None
    }

    fn tag_chip_indexes(&self, _folder_index: usize) -> Option<[usize; 2]> {
        None
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        if folder_index >= 1 || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x01c0 + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(save::Chip {
            id: raw.id as usize,
            code: num_traits::FromPrimitive::from_u8(raw.code)?,
        })
    }
}

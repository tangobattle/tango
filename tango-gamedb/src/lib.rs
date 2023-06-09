#[derive(Clone, Copy)]
pub enum Region {
    US,
    JP,
}

pub struct Game {
    pub family_and_variant: (&'static str, u8),
    pub rom_code_and_revision: (&'static [u8; 4], u8),
    pub crc32: u32,
    pub region: Region,
}

pub const AREJ_00: Game = Game {
    family_and_variant: ("exe1", 0),
    rom_code_and_revision: (b"AREJ", 0x00),
    crc32: 0xd9516e50,
    region: Region::JP,
};

pub const AREE_00: Game = Game {
    family_and_variant: ("bn1", 0),
    rom_code_and_revision: (b"AREE", 0x00),
    crc32: 0x1d347971,
    region: Region::US,
};

pub const AE2J_00_AC: Game = Game {
    family_and_variant: ("exe2", 0),
    rom_code_and_revision: (b"AE2J", 0x00),
    crc32: 0x46eed8d,
    region: Region::JP,
};

pub const AE2E_00: Game = Game {
    family_and_variant: ("bn2", 0),
    rom_code_and_revision: (b"AE2E", 0x00),
    crc32: 0x6d961f82,
    region: Region::US,
};

pub const GAMES: &[&Game] = &[&AREJ_00, &AREE_00, &AE2J_00_AC, &AE2E_00];

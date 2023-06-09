pub enum Region {
    US,
    JP,
}

pub struct Game {
    pub family_and_variant: (&'static str, u8),
    pub rom_code: &'static [u8; 4],
    pub revision: u8,
    pub crc32: u32,
    pub region: Region,
}

pub const GAMES: &[Game] = &[
    // BN1
    Game {
        family_and_variant: ("exe1", 0),
        rom_code: b"AREJ",
        revision: 0x00,
        crc32: 0xd9516e50,
        region: Region::JP,
    },
    Game {
        family_and_variant: ("bn1", 0),
        rom_code: b"AREE",
        revision: 0x00,
        crc32: 0x1d347971,
        region: Region::US,
    },
    // BN2
];

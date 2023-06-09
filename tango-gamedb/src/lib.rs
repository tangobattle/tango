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

// BN1
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

// BN2
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

// BN3
pub const A6BJ_01: Game = Game {
    family_and_variant: ("exe3", 0),
    rom_code_and_revision: (b"A6BJ", 0x01),
    crc32: 0xe48e6bc9,
    region: Region::JP,
};

pub const A3XJ_01: Game = Game {
    family_and_variant: ("exe3", 1),
    rom_code_and_revision: (b"A3XJ", 0x01),
    crc32: 0xfd57493b,
    region: Region::JP,
};

pub const A6BE_00: Game = Game {
    family_and_variant: ("bn3", 0),
    rom_code_and_revision: (b"A6BE", 0x00),
    crc32: 0x0be4410a,
    region: Region::US,
};

pub const A3XE_00: Game = Game {
    family_and_variant: ("bn3", 1),
    rom_code_and_revision: (b"A3XE", 0x01),
    crc32: 0xc0c780f9,
    region: Region::US,
};

// BN4

// BN5

// BN6

// EXE4.5

pub const GAMES: &[&Game] = &[
    &AREJ_00,
    &AREE_00,
    &AE2J_00_AC,
    &AE2E_00,
    &A6BJ_01,
    &A3XJ_01,
    &A6BE_00,
    &A3XE_00,
];

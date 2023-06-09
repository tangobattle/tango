#[derive(Clone, Copy, PartialEq)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq)]
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
    rom_code_and_revision: (b"A3XE", 0x00),
    crc32: 0xc0c780f9,
    region: Region::US,
};

// BN4
pub const B4WJ_01: Game = Game {
    family_and_variant: ("exe4", 0),
    rom_code_and_revision: (b"B4WJ", 0x01),
    crc32: 0xcf0e8b05,
    region: Region::JP,
};

pub const B4BJ_01: Game = Game {
    family_and_variant: ("exe4", 1),
    rom_code_and_revision: (b"B4BJ", 0x01),
    crc32: 0x709bbf07,
    region: Region::JP,
};

pub const B4WE_00: Game = Game {
    family_and_variant: ("bn4", 0),
    rom_code_and_revision: (b"B4WE", 0x00),
    crc32: 0x2120695c,
    region: Region::US,
};

pub const B4BE_00: Game = Game {
    family_and_variant: ("bn4", 1),
    rom_code_and_revision: (b"B4BE", 0x00),
    crc32: 0x758a46e9,
    region: Region::US,
};

// BN5
pub const BRBJ_00: Game = Game {
    family_and_variant: ("exe5", 0),
    rom_code_and_revision: (b"BRBJ", 0x00),
    crc32: 0xc73f23c0,
    region: Region::JP,
};

pub const BRKJ_00: Game = Game {
    family_and_variant: ("exe5", 1),
    rom_code_and_revision: (b"BRKJ", 0x00),
    crc32: 0x16842635,
    region: Region::JP,
};

pub const BRBE_00: Game = Game {
    family_and_variant: ("bn5", 0),
    rom_code_and_revision: (b"BRBE", 0x00),
    crc32: 0xa73e83a4,
    region: Region::US,
};

pub const BRKE_00: Game = Game {
    family_and_variant: ("bn5", 1),
    rom_code_and_revision: (b"BRKE", 0x00),
    crc32: 0xa552f683,
    region: Region::US,
};

// BN6
pub const BR5J_00: Game = Game {
    family_and_variant: ("exe6", 0),
    rom_code_and_revision: (b"BR5J", 0x00),
    crc32: 0x6285918a,
    region: Region::JP,
};

pub const BR6J_00: Game = Game {
    family_and_variant: ("exe6", 1),
    rom_code_and_revision: (b"BR6J", 0x00),
    crc32: 0x2dfb603e,
    region: Region::JP,
};

pub const BR5E_00: Game = Game {
    family_and_variant: ("bn6", 0),
    rom_code_and_revision: (b"BR5E", 0x00),
    crc32: 0x79452182,
    region: Region::US,
};

pub const BR6E_00: Game = Game {
    family_and_variant: ("bn6", 1),
    rom_code_and_revision: (b"BR6E", 0x00),
    crc32: 0xdee6f2a9,
    region: Region::US,
};

// EXE4.5
pub const BR4J_00: Game = Game {
    family_and_variant: ("exe45", 0),
    rom_code_and_revision: (b"BR4J", 0x00),
    crc32: 0xa646601b,
    region: Region::JP,
};

pub const GAMES: &[&Game] = &[
    &AREJ_00,
    &AREE_00,
    &AE2J_00_AC,
    &AE2E_00,
    &A6BJ_01,
    &A3XJ_01,
    &A6BE_00,
    &A3XE_00,
    &B4WJ_01,
    &B4BJ_01,
    &B4WE_00,
    &B4BE_00,
    &BRBJ_00,
    &BRKJ_00,
    &BRBE_00,
    &BRKE_00,
    &BR5J_00,
    &BR6J_00,
    &BR5E_00,
    &BR6E_00,
    &BR4J_00,
];

pub fn find_by_family_and_variant(family: &str, variant: u8) -> Option<&'static Game> {
    GAMES
        .iter()
        .find(|g| g.family_and_variant == (family, variant))
        .map(|v| *v)
}

pub fn find_by_rom_info(code: &[u8; 4], revision: u8) -> Option<&'static Game> {
    GAMES
        .iter()
        .find(|g| g.rom_code_and_revision == (code, revision))
        .map(|v| *v)
}

pub fn detect(rom: &[u8]) -> Option<&'static Game> {
    let code = rom.get(0xac..0xac + 4)?.try_into().unwrap();
    let revision = *rom.get(0xbc)?;
    let entry = GAMES.iter().find(|g| g.rom_code_and_revision == (code, revision))?;
    let crc32 = crc32fast::hash(rom);
    if crc32 != entry.crc32 {
        return None;
    }
    Some(*entry)
}

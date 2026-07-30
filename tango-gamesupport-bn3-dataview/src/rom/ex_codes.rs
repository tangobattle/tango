//! The EX/Mod-Code system is BN3's own — no other game has it — so its
//! model lives here, not in the shared dataview traits. See the ability
//! compiler notes for how the game rebuilds the 0x5770 ability array
//! from style + navicust + excode.

/// What an EX/Mod code grants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExCodeEffect {
    MaxHp(u16),
    SuperArmor,
    BreakBuster,
    BreakCharge,
    ShadowShoes,
    FloatShoes,
    AirShoes,
    UnderShirt,
    Block,
    Shield,
    Reflect,
    AntiDamage,
    MegaFolder(u8),
    GigaFolder(u8),
    FastGauge,
    SneakRun,
    Humor,
}

/// The drawback a code carries, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExCodeBug {
    Custom(u8),
    PoisonPanelStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExCode {
    pub code: u8,
    pub effect: ExCodeEffect,
    pub bug: Option<ExCodeBug>,
}

/// The code the save records (see `NavicustView`'s ex-code byte), or
/// `None` for a byte that isn't a defined code.
pub fn ex_code(code: u8) -> Option<ExCode> {
    EX_CODES.iter().find(|e| e.code == code).copied()
}

#[rustfmt::skip]
pub static EX_CODES: &[ExCode] = &[
    ExCode { code: 0x1e, effect: ExCodeEffect::MaxHp(100),     bug: None },
    ExCode { code: 0x1f, effect: ExCodeEffect::MaxHp(150),     bug: None },
    ExCode { code: 0x20, effect: ExCodeEffect::MaxHp(200),     bug: None },
    ExCode { code: 0x21, effect: ExCodeEffect::MaxHp(250),     bug: None },
    ExCode { code: 0x22, effect: ExCodeEffect::MaxHp(300),     bug: None },
    ExCode { code: 0x23, effect: ExCodeEffect::MaxHp(350),     bug: None },
    ExCode { code: 0x24, effect: ExCodeEffect::MaxHp(400),     bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x25, effect: ExCodeEffect::MaxHp(450),     bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x26, effect: ExCodeEffect::MaxHp(500),     bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x27, effect: ExCodeEffect::MaxHp(550),     bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x28, effect: ExCodeEffect::MaxHp(600),     bug: Some(ExCodeBug::Custom(2)) },
    ExCode { code: 0x29, effect: ExCodeEffect::MaxHp(650),     bug: Some(ExCodeBug::Custom(2)) },
    ExCode { code: 0x2a, effect: ExCodeEffect::MaxHp(700),     bug: Some(ExCodeBug::Custom(2)) },
    ExCode { code: 0x2b, effect: ExCodeEffect::SuperArmor,     bug: None },
    ExCode { code: 0x2c, effect: ExCodeEffect::BreakBuster,    bug: Some(ExCodeBug::Custom(2)) },
    ExCode { code: 0x2d, effect: ExCodeEffect::BreakCharge,    bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x2e, effect: ExCodeEffect::ShadowShoes,    bug: None },
    ExCode { code: 0x2f, effect: ExCodeEffect::FloatShoes,     bug: None },
    ExCode { code: 0x30, effect: ExCodeEffect::AirShoes,       bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x31, effect: ExCodeEffect::UnderShirt,     bug: None },
    ExCode { code: 0x32, effect: ExCodeEffect::Block,          bug: None },
    ExCode { code: 0x33, effect: ExCodeEffect::Shield,         bug: None },
    ExCode { code: 0x34, effect: ExCodeEffect::Reflect,        bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x35, effect: ExCodeEffect::AntiDamage,     bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x36, effect: ExCodeEffect::MegaFolder(1),  bug: None },
    ExCode { code: 0x37, effect: ExCodeEffect::MegaFolder(2),  bug: Some(ExCodeBug::Custom(1)) },
    ExCode { code: 0x38, effect: ExCodeEffect::FastGauge,      bug: Some(ExCodeBug::Custom(2)) },
    ExCode { code: 0x39, effect: ExCodeEffect::SneakRun,       bug: None },
    ExCode { code: 0x3a, effect: ExCodeEffect::Humor,          bug: None },
    ExCode { code: 0x3b, effect: ExCodeEffect::MaxHp(800),     bug: Some(ExCodeBug::PoisonPanelStep) },
    ExCode { code: 0x3c, effect: ExCodeEffect::MaxHp(900),     bug: Some(ExCodeBug::PoisonPanelStep) },
    ExCode { code: 0x3d, effect: ExCodeEffect::MaxHp(1000),    bug: Some(ExCodeBug::PoisonPanelStep) },
    ExCode { code: 0x3e, effect: ExCodeEffect::MegaFolder(3),  bug: Some(ExCodeBug::PoisonPanelStep) },
    ExCode { code: 0x3f, effect: ExCodeEffect::MegaFolder(4),  bug: Some(ExCodeBug::PoisonPanelStep) },
    ExCode { code: 0x40, effect: ExCodeEffect::MegaFolder(5),  bug: Some(ExCodeBug::PoisonPanelStep) },
    ExCode { code: 0x41, effect: ExCodeEffect::GigaFolder(1),  bug: Some(ExCodeBug::PoisonPanelStep) },
];

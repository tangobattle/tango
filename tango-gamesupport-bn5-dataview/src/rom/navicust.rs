//! NaviCust program effects for BN5, cataloged by part template id (the part
//! `id >> 2`, i.e. ignoring the four colour variants).
//!
//! Reverse-engineered from the effect-application code (Team ProtoMan BRBE): the
//! navicust compiler dispatches each installed program through a jump table at
//! `0x0813fb40` (indexed by program id), into a handler that calls
//! `set_stat(navi, field, value)` — `field` is the navi-stats byte offset. The
//! field layout matches BN6, so the shared abilities behave identically; BN5
//! adds SoulTime, the virus folders, Chivalry, AutoRun, SoulCleanse and the
//! (unobtainable) HubStyle bundle. Values are identical between Team ProtoMan
//! and Team Colonel.
//!
//! Bugs come from a second jump table at `0x08140064`, dispatched by effect
//! group × bugged-count; [`EffectGroup`] / [`NavicustBug`] /
//! [`navicust_group_bugs`] capture that, with bug names from the wiki
//! (therockmanexezone NaviCustomizer (MMBN5)).

/// A single mechanical effect a NaviCust program grants when installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavicustEffect {
    /// Max HP `+N`.
    MaxHp(u16),
    /// Mega-chip folder limit `+N`.
    MegaLimit(u8),
    /// Giga-chip folder limit `+N`.
    GigaLimit(u8),
    /// Custom screen `+N` chips.
    CustomGauge(u8),
    /// MegaBuster Attack `+N` (clamps at 4).
    Attack(u8),
    /// MegaBuster Speed `+N` (clamps at 4).
    Speed(u8),
    /// MegaBuster Charge `+N` (clamps at 4).
    Charge(u8),
    /// DoubleSoul duration `+N` turns (clamps at 6).
    SoulTime(u8),
    /// MegaBuster Attack/Speed/Charge set to 4 (the cap).
    AttackMax,
    SpeedMax,
    ChargeMax,
    /// Can't be pushed back (SprArmr).
    SuperArmor,
    /// Start each battle with a Barrier (FstBarr).
    FirstBarrier,
    /// `B`+Left guard moves.
    Shield,
    Reflect,
    AntiDamage,
    /// Immune to panel-type effects (FlotShoe).
    FloatShoes,
    /// Can move over holes (AirShoes).
    AirShoes,
    /// Survive a lethal hit on 1 HP (UnderSht).
    UnderShirt,
    /// No weak random encounters (SneakRun).
    SneakRun,
    /// Attracts Fire viruses (OilBody).
    OilBody,
    /// Attracts Aqua viruses (Fish).
    Fish,
    /// Attracts Elec viruses (Battery).
    Battery,
    /// Attracts Wood viruses (Jungle).
    Jungle,
    /// More chips dropped by enemies (Collect).
    Collect,
    /// Mystery data yields zenny (Millions).
    Millions,
    /// `L` button gag (Humor).
    Humor,
    /// `L` button macho pose (Chivalry).
    Chivalry,
    /// Run in the overworld without holding `B` (AutoRun).
    AutoRun,
    /// Recover HP after each battle (AutoHeal).
    AutoHeal,
    /// Prevents NaviCust bugs (BugStop).
    BugStop,
    /// Purifies the soul, undoing dark-chip use (SoulClen).
    SoulCleanse,
    /// VS-only support navis.
    Rush,
    Beat,
    Tango,
    /// Mega-virus folder (MegaVirs).
    MegaVirus,
    /// Giga-virus folder (GigaVirs).
    GigaVirus,
    /// The Hub Style bundle (HubBatc) — unobtainable in legit play; grants
    /// SuperArmor, Custom+1, Mega+1, FirstBarrier, Shield and the shoe set.
    HubStyle,
}

/// The effects of NaviCust part `id` (the colour variants `id >> 2` share an
/// effect), or `&[]` for the blank part and unknown ids.
pub fn navicust_part_effects(id: usize) -> &'static [NavicustEffect] {
    use NavicustEffect::*;
    match id >> 2 {
        1 => &[SuperArmor],                                    // SprArmr
        2 => &[CustomGauge(1)],                                // Custom1
        3 => &[CustomGauge(2)],                                // Custom2
        4 => &[MegaLimit(1)],                                  // MegFldr1
        5 => &[MegaLimit(2)],                                  // MegFldr2
        6 => &[GigaLimit(1)],                                  // GigFldr1
        7 => &[FirstBarrier],                                  // FstBarr
        8 => &[Shield],                                        // Shield
        9 => &[Reflect],                                       // Reflect
        10 => &[AntiDamage],                                   // AntiDmg
        11 => &[FloatShoes],                                   // FlotShoe
        12 => &[AirShoes],                                     // AirShoes
        13 => &[UnderShirt],                                   // UnderSht
        14 => &[SneakRun],                                     // SneakRun
        15 => &[OilBody],                                      // OilBody
        16 => &[Fish],                                         // Fish
        17 => &[Battery],                                      // Battery
        18 => &[Jungle],                                       // Jungle
        19 => &[Collect],                                      // Collect
        20 => &[Millions],                                     // Millions
        21 => &[Humor],                                        // Humor
        22 => &[Chivalry],                                     // Chivalry
        23 => &[AutoRun],                                      // AutoRun
        24 => &[AutoHeal],                                     // AutoHeal
        25 => &[Attack(3), Speed(3), Charge(3)],               // BustPack
        26 => &[SuperArmor, FloatShoes, AirShoes, UnderShirt], // BodyPack
        27 => &[HubStyle],                                     // HubBatc
        28 => &[BugStop],                                      // BugStop
        29 => &[SoulCleanse],                                  // SoulClen
        30 => &[Rush],                                         // Rush
        31 => &[Beat],                                         // Beat
        32 => &[Tango],                                        // Tango
        33 => &[MegaVirus],                                    // MegaVirs
        34 => &[GigaVirus],                                    // GigaVirs
        35 => &[Attack(1)],                                    // Attck+1
        36 => &[Speed(1)],                                     // Speed+1
        37 => &[Charge(1)],                                    // Charge+1
        38 => &[SoulTime(1)],                                  // SoulT+1
        39 => &[AttackMax],                                    // AttckMAX
        40 => &[SpeedMax],                                     // SpeedMAX
        41 => &[ChargeMax],                                    // ChargMAX
        42 => &[MaxHp(50)],                                    // HP+50
        43 => &[MaxHp(100)],                                   // HP+100
        44 => &[MaxHp(200)],                                   // HP+200
        45 => &[MaxHp(300)],                                   // HP+300
        46 => &[MaxHp(400)],                                   // HP+400
        47 => &[MaxHp(500)],                                   // HP+500
        _ => &[],                                              // 0 = None / unknown
    }
}

/// The coarse effect group of a NaviCust program (its part-data `_unk_05[0]`
/// byte), classified by what the program does. The group determines which bug a
/// malfunctioning program inflicts (see [`navicust_group_bugs`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectGroup {
    /// Battle abilities (SprArmr, FstBarr, Shield, Reflect, AntiDmg, UnderSht, BodyPack, SoulClen).
    BattleAbility = 1,
    /// `L`-button programs (Humor, Chivalry, SoulT+1).
    LButton = 2,
    /// Movement/shoe abilities (FlotShoe, AirShoes, AutoRun, AutoHeal).
    Movement = 3,
    /// Folder / custom-gauge limits (Custom, MegFldr, GigFldr).
    FolderCustom = 4,
    /// Encounter modifiers (SneakRun, OilBody, Fish, Battery, Jungle, MegaVirs, GigaVirs).
    Encounter = 5,
    /// Battle-reward modifiers (Collect, Millions).
    Drops = 6,
    /// Buster upgrades (BustPack, Attack/Speed/Charge +1/MAX).
    Buster = 7,
    /// VS-only support navis (Rush, Beat, Tango).
    SupportNavi = 8,
    /// HP-memory programs (HP+50 … HP+500).
    Hp = 9,
    /// The HubStyle bundle (HubBatc).
    HubStyle = 10,
}

/// A NaviCust bug: the malfunction inflicted when programs are bugged (placed
/// off their command line, off-colour, or out of bounds). Names follow the
/// in-game bug list; the parenthesised offsets are the navi-stats bug fields the
/// dispatch handlers write.
///
/// This is just the *kind* of bug. Its severity is carried separately as a level
/// (1–3) by [`navicust_group_bugs`], so the levels from several bugged groups
/// can be accumulated — see that function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavicustBug {
    /// HP drains during battle, faster at higher level (`0x16`).
    BattleHp,
    /// Flinch / knockback inflicts a Battle HP Bug.
    DamageHp,
    /// The MegaBuster may fire blanks or a ChargeShot; higher level worsens the
    /// odds (`0x14`/`0x15`).
    BusterBlank,
    /// The Custom screen deals HP damage each turn (20/40/80 at level 1/2/3)
    /// (`0x54`, accumulated).
    CustomDamage,
    /// The emotion window cycles randomly during battle (`0x24`).
    EmotionWindow,
    /// Random encounters appear more frequently (`0x28`).
    Encounter,
    /// Moving warps MegaMan to the edge of his field — "ProcessingBug" (`0x31`).
    Movement,
    /// A chance to crack the panel moved from: 2/4/8 in 8 at level 1/2/3
    /// (`0x12`/`0x13`).
    Panel,
    /// Battle rewards are Zenny only (`0x26`).
    Result,
    /// A battle-start status ailment, whose duration is set by the grid's colour
    /// count. Also inflicted by a bugged HubStyle (`0x1a`).
    Status,
}

pub fn navicust_group_bugs(group: EffectGroup) -> &'static [NavicustBug] {
    use EffectGroup as G;
    use NavicustBug as B;
    match group {
        G::BattleAbility => &[B::Movement],
        G::LButton => &[B::EmotionWindow],
        G::Movement => &[B::Panel],
        G::FolderCustom => &[B::CustomDamage],
        G::Encounter => &[B::Encounter],
        G::Drops => &[B::Result],
        G::Buster => &[B::BusterBlank],
        G::Hp => &[B::BattleHp, B::DamageHp],
        G::SupportNavi => &[],
        G::HubStyle => &[B::Status],
    }
}

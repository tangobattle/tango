//! NaviCust program effects for BN6, cataloged by part template id (the part
//! `id >> 2`, i.e. ignoring the four colour variants).
//!
//! Reverse-engineered from the effect-application code (Cybeast Gregar BR5E):
//! the navicust compiler dispatches each installed program through a jump table
//! at `0x0813e528` (indexed by program id), into a handler that calls
//! `set_stat(navi, field, value)` — where `field` is the byte offset into the
//! navi stats block ([`super::super::save`]'s `RawNaviStats`). All 47 handlers
//! were decoded; values are identical between Gregar and Falzar. Notable code
//! facts: buster Attack/Speed/Charge (`0x01/02/03`) `+1`/`+3` clamp at 4 and
//! `…MAX` sets 4; Mega/Giga (`0x0b/0x0c`) clamp 10, Custom (`0x0a`) clamp 8;
//! abilities set their own RawNaviStats flag; Shield/Reflect/AntiDmg all write
//! `b_left_ability` (`0x07`) = `0x3b`/`0x8b`/`0x3d`. Composite programs
//! (BustPack, BodyPack, FldrPak) call their constituent handlers, so they
//! expand to multiple effects here.
//!
//! Bugs (when a program is placed wrong) come from a second jump table at
//! `0x0813e9f8`, dispatched by effect group × bugged-count; [`EffectGroup`] /
//! [`NavicustBug`] / [`navicust_group_bugs`] capture that, with bug names taken
//! from the wiki (therockmanexezone NaviCustomizer (MMBN6)).

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
    /// MegaBuster Attack `+N` (the stat clamps at 4).
    Attack(u8),
    /// MegaBuster Speed `+N` (clamps at 4).
    Speed(u8),
    /// MegaBuster Charge `+N` (clamps at 4).
    Charge(u8),
    /// MegaBuster Attack/Speed/Charge set to 4 (the cap).
    AttackMax,
    SpeedMax,
    ChargeMax,
    /// Can't be pushed back (SuprArmr).
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
    /// Custom screen: shuffle the chip selection once (ChpShufl).
    ChipShuffle,
    /// Custom screen: selecting a chip counts as 10 (NumbrOpn).
    NumberOpen,
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
    /// `L` button recites a poem (Poem).
    Poem,
    /// `B`+move slides without stopping (SlipRunr).
    SlipRunner,
    /// Recover HP after each battle (AutoHeal).
    AutoHeal,
    /// Prevents NaviCust bugs (BugStop).
    BugStop,
    /// VS-only support navis.
    Rush,
    Beat,
    Tango,
}

/// The effects of NaviCust part `id` (the colour variants `id >> 2` share an
/// effect), or `&[]` for the blank part and unknown ids.
pub fn navicust_part_effects(id: usize) -> &'static [NavicustEffect] {
    use NavicustEffect::*;
    match id >> 2 {
        1 => &[SuperArmor],                                    // SuprArmr
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
        14 => &[ChipShuffle],                                  // ChpShufl
        15 => &[NumberOpen],                                   // NumbrOpn
        16 => &[SneakRun],                                     // SneakRun
        17 => &[OilBody],                                      // OilBody
        18 => &[Fish],                                         // Fish
        19 => &[Battery],                                      // Battery
        20 => &[Jungle],                                       // Jungle
        21 => &[Collect],                                      // Collect
        22 => &[Millions],                                     // Millions
        23 => &[Humor],                                        // Humor
        24 => &[Poem],                                         // Poem
        25 => &[SlipRunner],                                   // SlipRunr
        26 => &[AutoHeal],                                     // AutoHeal
        27 => &[Attack(3), Speed(3), Charge(3)],               // BustPack
        28 => &[SuperArmor, UnderShirt, FloatShoes, AirShoes], // BodyPack
        29 => &[CustomGauge(1), MegaLimit(1)],                 // FldrPak1
        30 => &[CustomGauge(2), MegaLimit(2)],                 // FldrPak2
        31 => &[BugStop],                                      // BugStop
        32 => &[Rush],                                         // Rush
        33 => &[Beat],                                         // Beat
        34 => &[Tango],                                        // Tango
        35 => &[Attack(1)],                                    // Attack+1
        36 => &[Speed(1)],                                     // Speed+1
        37 => &[Charge(1)],                                    // Charge+1
        38 => &[AttackMax],                                    // AttckMAX
        39 => &[SpeedMax],                                     // SpeedMAX
        40 => &[ChargeMax],                                    // ChargMAX
        41 => &[MaxHp(50)],                                    // HP+50
        42 => &[MaxHp(100)],                                   // HP+100
        43 => &[MaxHp(200)],                                   // HP+200
        44 => &[MaxHp(300)],                                   // HP+300
        45 => &[MaxHp(400)],                                   // HP+400
        46 => &[MaxHp(500)],                                   // HP+500
        _ => &[],                                              // 0 = None / unknown
    }
}

/// The coarse effect group of a NaviCust program (its part-data `_unk_05[0]`
/// byte), classified by what the program does. The group determines which bug a
/// malfunctioning program inflicts (see [`navicust_group_bugs`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectGroup {
    /// Battle abilities (SuprArmr, FstBarr, Shield, Reflect, AntiDmg, UnderSht, BodyPack).
    BattleAbility = 1,
    /// `L`-button programs (Humor, Poem).
    LButton = 2,
    /// Movement/shoe abilities (FlotShoe, AirShoes, SlipRunr, AutoHeal).
    Movement = 3,
    /// Folder / custom-gauge limits (Custom, MegFldr, GigFldr, ChpShufl, NumbrOpn, FldrPak).
    FolderCustom = 4,
    /// Encounter modifiers (SneakRun, OilBody, Fish, Battery, Jungle).
    Encounter = 5,
    /// Battle-reward modifiers (Collect, Millions).
    Drops = 6,
    /// Buster upgrades (BustPack, Attack/Speed/Charge +1/MAX).
    Buster = 7,
    /// VS-only support navis (Rush, Beat, Tango).
    SupportNavi = 8,
    /// HP-memory programs (HP+50 … HP+500).
    Hp = 9,
}

/// A NaviCust bug: the malfunction inflicted when programs are bugged (placed
/// off their command line, off-colour, or out of bounds). Names follow the
/// in-game bug list; the parenthesised offsets are the `RawNaviStats` bug fields
/// the dispatch handlers write.
///
/// This is just the *kind* of bug. Its severity is carried separately as a level
/// (1–3) by [`navicust_group_bugs`], so the levels from several bugged groups
/// can be accumulated — see that function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavicustBug {
    /// HP drains during battle, faster at higher level (`0x18`).
    BattleHp,
    /// Flinch / knockback / pull-in inflicts a Battle HP Bug (`0x16`).
    DamageHp,
    /// The MegaBuster may fire blanks or a ChargeShot; higher level worsens the
    /// odds (`0x14` = 6/10/13 per level, `0x15` = level).
    BusterBlank,
    /// The Custom screen loses a slot every turn, starting on turn 4/3/2 at level
    /// 1/2/3 (TurnsUntilCustBug `0x63`).
    Custom,
    /// The emotion window cycles randomly during battle (EmotionBug `0x24`).
    EmotionWindow,
    /// Random encounters appear more frequently (`0x28`).
    Encounter,
    /// Moving warps MegaMan to the edge of his field — internally "ProcessingBug"
    /// (`0x31`).
    Movement,
    /// A chance to crack the panel moved from: 2/3/4 in 8 at level 1/2/3
    /// (`0x12`/`0x13`).
    Panel,
    /// Battle rewards are Zenny only (`0x26`).
    Result,
    /// A battle-start status ailment (confused/blind/flashing/invincible) whose
    /// duration is set by the number of colours on the grid. Triggered by the
    /// grid's colour count rather than a program group, so it is never returned
    /// by [`navicust_group_bugs`].
    Status,
}

pub fn navicust_group_bugs(group: EffectGroup) -> &'static [NavicustBug] {
    use EffectGroup as G;
    use NavicustBug as B;

    match group {
        G::BattleAbility => &[B::Movement],
        G::LButton => &[B::EmotionWindow],
        G::Movement => &[B::Panel],
        G::FolderCustom => &[B::Custom],
        G::Encounter => &[B::Encounter],
        G::Drops => &[B::Result],
        G::Buster => &[B::BusterBlank],
        G::Hp => &[B::BattleHp, B::DamageHp],
        G::SupportNavi => &[],
    }
}

//! NaviCust program effects for BN4, cataloged by part template id (the part
//! `id >> 2`, i.e. ignoring the four colour variants).
//!
//! Reverse-engineered from the effect-application code (Red Sun B4WE): the
//! navicust compiler dispatches each installed program through a jump table at
//! `0x08041a54` — entry `i` handles program `i + 1` (the blank NONE program is a
//! no-op and isn't in the table) — into a handler that calls
//! `set_stat(field, value)` (`0x0800d78a`), where `field` is the navi-stats byte
//! offset. Decoded field offsets: super_armor `0x01`, float_shoes `0x02`,
//! air_shoes `0x03`, under_shirt `0x04`, buster attack/speed/charge `0x05/06/07`
//! (clamp 4), weapon `0x0a` (Heat 2 / Aqua 3 / Elec 4 / Wood 5 / Invisible 6),
//! weapon_level `0x0b` (clamp 2), b_left_ability `0x0c` (Shield `0x25` / Reflect
//! `0x26` / AntiMagic `0x27`), custom_gauge `0x12` (clamp 8), mega_limit `0x13`
//! and giga_limit `0x14` (clamp 10), sneak_run `0x16`, element-attract `0x17`,
//! support_navi `0x18`, collect `0x19`, humor `0x1c`, bug_stop `0x1d`,
//! soul_cleanse `0x1e`, first_barrier `0x21`; HP is added through a direct
//! pointer write. Composite programs call their constituent handlers, so they
//! expand to multiple effects here (notably BustPack also raises weapon_level).
//! Values are identical between Red Sun and Blue Moon.
//!
//! Bugs (when a program is placed wrong) come from a second jump table at
//! `0x08042b28`, dispatched by effect group × bugged-count; [`EffectGroup`] /
//! [`NavicustBug`] / [`navicust_group_bugs`] capture that, with bug names taken
//! from the wiki (therockmanexezone NaviCustomizer (MMBN4)).

/// A single mechanical effect a NaviCust program grants when installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavicustEffect {
    /// Max HP `+N`.
    MaxHp(u16),
    /// Mega-chip folder limit `+N` (clamps at 10).
    MegaLimit(u8),
    /// Giga-chip folder limit `+N` (clamps at 10).
    GigaLimit(u8),
    /// Custom screen `+N` chips (clamps at 8).
    CustomGauge(u8),
    /// MegaBuster Attack `+N` (clamps at 4).
    Attack(u8),
    /// MegaBuster Speed `+N` (clamps at 4).
    Speed(u8),
    /// MegaBuster Charge `+N` (clamps at 4).
    Charge(u8),
    /// MegaBuster Attack/Speed/Charge set to 4 (the cap).
    AttackMax,
    SpeedMax,
    ChargeMax,
    /// NaviCustomizer weapon level `+N` (clamps at 2).
    WeaponLevel(u8),
    /// Can't be pushed back (SprArmr).
    SuperArmor,
    /// Start each battle with a Barrier (FstBarr).
    FirstBarrier,
    /// `B`+Left guard moves.
    Shield,
    Reflect,
    /// `B`+Left hurls a star back (AntiMagc).
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
    /// `L` button gag (Humor).
    Humor,
    /// Turns random encounters in the area Fire (HeatWepn) — weapon slot `0x0a`.
    HeatWeapon,
    /// Turns random encounters in the area Aqua (AquaWepn).
    AquaWeapon,
    /// Turns random encounters in the area Elec (ElecWepn).
    ElecWeapon,
    /// Turns random encounters in the area Wood (WoodWepn).
    WoodWeapon,
    /// Invisible in the overworld (Invisibl) — weapon slot `0x0a` = 6.
    Invisible,
    /// Prevents NaviCust bugs (BugStop).
    BugStop,
    /// Purifies the soul, undoing dark-chip use (SoulClen).
    SoulCleanse,
    /// VS-only support navis.
    Rush,
    Beat,
    Tango,
    /// The Hub Style bundle (HubBatc) — grants SuperArmor, Custom+1, Mega+1,
    /// FirstBarrier, Shield and the shoe set plus an `0x49` b_left_ability.
    HubStyle,
}

/// The effects of NaviCust part `id` (the colour variants `id >> 2` share an
/// effect), or `&[]` for the blank part and unknown ids.
pub fn navicust_part_effects(id: usize) -> &'static [NavicustEffect] {
    use NavicustEffect::*;
    match id >> 2 {
        1 => &[SuperArmor],                                      // SprArmr
        2 => &[CustomGauge(1)],                                  // Custom1
        3 => &[CustomGauge(2)],                                  // Custom2
        4 => &[MegaLimit(1)],                                    // MegFldr1
        5 => &[MegaLimit(2)],                                    // MegFldr2
        6 => &[GigaLimit(1)],                                    // GigFldr1
        7 => &[FirstBarrier],                                    // FstBarr
        8 => &[Shield],                                          // Shield
        9 => &[Reflect],                                         // Reflect
        10 => &[AntiDamage],                                     // AntiMagc
        11 => &[FloatShoes],                                     // FlotShoe
        12 => &[AirShoes],                                       // AirShoes
        13 => &[UnderShirt],                                     // UnderSht
        14 => &[SneakRun],                                       // SneakRun
        15 => &[OilBody],                                        // OilBody
        16 => &[Fish],                                           // Fish
        17 => &[Battery],                                        // Battery
        18 => &[Jungle],                                         // Jungle
        19 => &[Collect],                                        // Collect
        20 => &[Humor],                                          // Humor
        21 => &[Attack(3), Speed(3), Charge(3), WeaponLevel(1)], // BustPack
        22 => &[SuperArmor, FloatShoes, AirShoes, UnderShirt],   // BodyPack
        23 => &[HubStyle],                                       // HubBatc
        24 => &[BugStop],                                        // BugStop
        25 => &[SoulCleanse],                                    // SoulClen
        26 => &[Rush],                                           // Rush
        27 => &[Beat],                                           // Beat
        28 => &[Tango],                                          // Tango
        29 => &[HeatWeapon],                                     // HeatWepn
        30 => &[AquaWeapon],                                     // AquaWepn
        31 => &[ElecWeapon],                                     // ElecWepn
        32 => &[WoodWeapon],                                     // WoodWepn
        33 => &[Invisible],                                      // Invisibl
        34 => &[Attack(1)],                                      // Attack+1
        35 => &[Speed(1)],                                       // Speed+1
        36 => &[Charge(1)],                                      // Charge+1
        37 => &[AttackMax],                                      // AttckMAX
        38 => &[SpeedMax],                                       // SpeedMAX
        39 => &[ChargeMax],                                      // ChargMAX
        40 => &[WeaponLevel(1)],                                 // WeapLV+1
        41 => &[MaxHp(50)],                                      // HP+50
        42 => &[MaxHp(100)],                                     // HP+100
        43 => &[MaxHp(200)],                                     // HP+200
        44 => &[MaxHp(300)],                                     // HP+300
        45 => &[MaxHp(400)],                                     // HP+400
        46 => &[MaxHp(500)],                                     // HP+500
        _ => &[],                                                // 0 = None / unknown
    }
}

/// The coarse effect group of a NaviCust program (its part-data `effect_group`
/// byte), classified by what the program does. The group determines which bug a
/// malfunctioning program inflicts (see [`navicust_group_bugs`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectGroup {
    /// Battle abilities (SprArmr, FstBarr, Shield, Reflect, AntiMagc, UnderSht, Humor, BodyPack).
    BattleAbility = 1,
    /// HP-memory programs (HP+50 … HP+500).
    Hp = 2,
    /// Folder / custom-gauge limits (Custom, MegFldr, GigFldr).
    FolderCustom = 3,
    /// Movement/shoe abilities (FlotShoe, AirShoes).
    Movement = 4,
    /// Encounter modifiers (SneakRun, OilBody, Fish, Battery, Jungle).
    Encounter = 5,
    /// Buster upgrades (BustPack, Attack/Speed/Charge +1/MAX).
    Buster = 6,
    /// VS-only support navis (Rush, Beat, Tango).
    SupportNavi = 7,
    /// Weapon-level program (WeapLV+1).
    WeaponLevel = 8,
    /// Battle-reward modifiers (Collect).
    Drops = 9,
    /// The Hub Style bundle (HubBatc).
    Bundle = 10,
}

/// A NaviCust bug: the malfunction inflicted when programs are bugged (placed
/// off their command line, off-colour, or out of bounds). Names follow the
/// in-game bug list; the parenthesised offsets are the navi-stats bug fields the
/// dispatch handlers write.
///
/// This is just the *kind* of bug. Its severity is carried separately as a level
/// (1–3) by [`navicust_group_bugs`], so the levels from several bugged groups can
/// be accumulated — see that function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NavicustBug {
    /// HP drains during battle, faster at higher level (`0x0f`).
    BattleHp,
    /// Battle rewards are degraded (`0x16`).
    BattleResult,
    /// The MegaBuster misfires (`0x18`).
    Buster,
    /// MegaMan acts on his own / mis-inputs (`0x0e`).
    Player,
    /// The Custom screen drains HP each turn (`0x1b`).
    CustomHp,
    /// Random encounters appear more frequently (`0x08`).
    Encounter,
    /// A chance to crack the panel moved from (`0x15`).
    Panel,
    /// The active support navi is corrupted (`0x0a`).
    Support,
    /// A grid with more than four colours inflicts Player and Custom-HP bugs
    /// whose level is the colour count over four. Driven by the grid rather than
    /// a program group, so it is never returned by [`navicust_group_bugs`].
    Color,
}

/// The bug type(s) inflicted when programs of `group` are bugged.
/// Reverse-engineered from the bug-dispatch jump table at `0x08042b28` (B4WE) —
/// each group's handler writes a bug-status field, with the magnitude scaled by
/// the bugged count — and named per the in-game bug list. The weapon-level
/// program, the Hub Style bundle and the group-0 programs (BugStop, SoulClen,
/// the element weapons, Invisible) don't bug. [`NavicustBug::Color`] is
/// grid-colour-driven and is not returned here.
///
/// This returns only the bug *kinds*; their severity is the bug level — the
/// number of bugged programs in the group, capped at 3 — which the caller pairs
/// in. Summing those levels across every bugged group gives the navi's full bug
/// state, e.g.:
///
/// ```ignore
/// let mut bugs = std::collections::HashMap::<NavicustBug, u8>::new();
/// for (group, count) in bugged_groups {
///     let level = count.clamp(1, 3);
///     for &bug in navicust_group_bugs(group) {
///         *bugs.entry(bug).or_default() += level;
///     }
/// }
/// ```
pub fn navicust_group_bugs(group: EffectGroup) -> &'static [NavicustBug] {
    use EffectGroup as G;
    use NavicustBug as B;
    match group {
        G::BattleAbility => &[B::Player],
        G::Hp => &[B::BattleHp],
        G::FolderCustom => &[B::CustomHp],
        G::Movement => &[B::Panel],
        G::Encounter => &[B::Encounter],
        G::Buster => &[B::Buster],
        G::SupportNavi => &[B::Support],
        G::Drops => &[B::BattleResult],
        G::WeaponLevel => &[],
        G::Bundle => &[],
    }
}

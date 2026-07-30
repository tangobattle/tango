//! NaviCust program effects for BN3, cataloged by part template id (the part
//! `id >> 2`, i.e. ignoring the four colour variants).
//!
//! Reverse-engineered from the effect-application code (White A6BE): the navicust
//! compiler dispatches each installed program through a jump table at
//! `0x0803c7fc` — entry `i` handles program `i + 1` (the blank NONE program is a
//! no-op and isn't in the table) — into a handler that calls `set_stat(field,
//! value)`, where `field` is the navi-stats byte offset. Decoded field offsets:
//! super_armor `0x01`, shoes `0x02` (Shadow 1 / Float 2), air_shoes `0x03`,
//! under_shirt `0x04`, break_buster `0x06`, break_charge `0x0e`, buster
//! attack/speed/charge `0x08/09/0a` (clamp 4), power_attack_level `0x0d`,
//! b_left_ability `0x0f` (Block 2 / Shield 4 / Reflect 6 / AntiDmg 8),
//! reg_memory `0x12`, custom_gauge `0x13`, mega_limit `0x14`, giga_limit `0x15`,
//! panel-set `0x17` (Green `0x36` / Ice `0x37` / Lava `0x38` / Sand `0x3a` /
//! Metal `0x35` / Holy `0x19`), fast_gauge `0x18`, sneak_run `0x1a`,
//! element-attract `0x1b` (Oil 2 / Fish 3 / Battery 1 / Jungle 4), support_navi
//! `0x1c` (Rush 1 / Beat 2 / Tango 3), collect `0x1d`, black_mind `0x21`,
//! humor `0x22`, bug_stop `0x23`, energy_change `0x24`, alpha `0x25`, press
//! `0x28`, dark_license `0x20`; HP is added through a direct pointer write.
//! BustrMAX maxes all three buster stats, and HubBatc calls its constituent
//! handlers. Values are identical between White and Blue.
//!
//! Bugs (when a program is placed wrong) come from a second jump table at
//! `0x0803c3e8`, dispatched by effect group × bugged-count (groups 1–14);
//! [`EffectGroup`] / [`NavicustBug`] / [`navicust_group_bugs`] capture that, with
//! bug names taken from the wiki (therockmanexezone NaviCustomizer (MMBN3)).

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
    /// Regular-memory capacity `+N` MB (Reg+5).
    RegMemory(u8),
    /// MegaBuster Attack `+N` (clamps at 4).
    Attack(u8),
    /// MegaBuster Speed `+N` (clamps at 4).
    Speed(u8),
    /// MegaBuster Charge `+N` (clamps at 4).
    Charge(u8),
    /// MegaBuster Attack/Speed/Charge all maxed (BustrMAX).
    BusterMax,
    /// PowerAttack (B-button charge) level `+N` (WeapLV+1).
    WeaponLevel(u8),
    /// Can't be pushed back (SprArmor).
    SuperArmor,
    /// The MegaBuster pierces guards (BrakBust).
    BreakBuster,
    /// PowerAttacks pierce guards (BrakChrg).
    BreakCharge,
    /// `B`+Left halves damage (Block).
    Block,
    /// `B`+Left negates damage (Shield).
    Shield,
    /// `B`+Left returns damage (Reflect).
    Reflect,
    /// `B`+Left hurls a star back when hit (AntiDmg).
    AntiDamage,
    /// Immune to panel-type effects (FlotShoe).
    FloatShoes,
    /// Walk safely over cracked/broken panels (ShdwShoe).
    ShadowShoes,
    /// Can move over holes (AirShoes).
    AirShoes,
    /// Survive a lethal hit on 1 HP (UnderSht).
    UnderShirt,
    /// Turn MegaMan's panels to Grass (SetGreen).
    SetGreen,
    /// Turn MegaMan's panels to Ice (SetIce).
    SetIce,
    /// Turn MegaMan's panels to Lava (SetLava).
    SetLava,
    /// Turn MegaMan's panels to Sand (SetSand).
    SetSand,
    /// Turn MegaMan's panels to Metal (SetMetal).
    SetMetal,
    /// Turn MegaMan's panels to Holy (SetHoly).
    SetHoly,
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
    /// The custom gauge fills faster (FstGauge).
    FastGauge,
    /// Shrink to fit through tight overworld gaps (Press).
    Press,
    /// Fire/Aqua chips restore energy instead (EngyChng).
    EnergyChange,
    /// The hidden Alpha appears in a hallway (Alpha).
    Alpha,
    /// `L` button gag (Humor).
    Humor,
    /// Prevents NaviCust bugs (BugStop).
    BugStop,
    /// Dons an evil disguise (BlckMind).
    BlackMind,
    /// Use Hole-requiring chips without a Hole (DarkLcns).
    DarkLicense,
    /// VS-only support navis.
    Rush,
    Beat,
    Tango,
    /// The Hub Style bundle (HubBatc) — SuperArmor, BreakBuster, BreakCharge,
    /// Custom+1, Mega+1, Shield, FloatShoes, UnderShirt and AirShoes.
    HubStyle,
}

/// The effects of NaviCust part `id` (the colour variants `id >> 2` share an
/// effect), or `&[]` for the blank part and unknown ids.
pub fn navicust_part_effects(id: usize) -> &'static [NavicustEffect] {
    use NavicustEffect::*;
    match id >> 2 {
        1 => &[SuperArmor],      // SprArmor
        2 => &[BreakBuster],     // BrakBust
        3 => &[BreakCharge],     // BrakChrg
        4 => &[SetGreen],        // SetGreen
        5 => &[SetIce],          // SetIce
        6 => &[SetLava],         // SetLava
        7 => &[SetSand],         // SetSand
        8 => &[SetMetal],        // SetMetal
        9 => &[SetHoly],         // SetHoly
        10 => &[CustomGauge(1)], // Custom1
        11 => &[CustomGauge(2)], // Custom2
        12 => &[MegaLimit(1)],   // MegFldr1
        13 => &[MegaLimit(2)],   // MegFldr2
        14 => &[Block],          // Block
        15 => &[Shield],         // Shield
        16 => &[Reflect],        // Reflect
        17 => &[ShadowShoes],    // ShdwShoe
        18 => &[FloatShoes],     // FlotShoe
        19 => &[AntiDamage],     // AntiDmg
        20 => &[Press],          // Press
        21 => &[EnergyChange],   // EngyChng
        22 => &[Alpha],          // Alpha
        23 => &[SneakRun],       // SneakRun
        24 => &[OilBody],        // OilBody
        25 => &[Fish],           // Fish
        26 => &[Battery],        // Battery
        27 => &[Jungle],         // Jungle
        28 => &[Collect],        // Collect
        29 => &[AirShoes],       // AirShoes
        30 => &[UnderShirt],     // UnderSht
        31 => &[FastGauge],      // FstGauge
        32 => &[Rush],           // Rush
        33 => &[Beat],           // Beat
        34 => &[Tango],          // Tango
        35 => &[WeaponLevel(1)], // WeapLV+1
        36 => &[MaxHp(100)],     // HP+100
        37 => &[MaxHp(200)],     // HP+200
        38 => &[MaxHp(300)],     // HP+300
        39 => &[MaxHp(500)],     // HP+500
        40 => &[RegMemory(5)],   // Reg+5
        41 => &[Attack(1)],      // Atk+1
        42 => &[Speed(1)],       // Speed+1
        43 => &[Charge(1)],      // Charge+1
        44 => &[BugStop],        // BugStop
        45 => &[Humor],          // Humor
        46 => &[BlackMind],      // BlckMind
        47 => &[BusterMax],      // BustrMAX
        48 => &[GigaLimit(1)],   // GigFldr1
        49 => &[HubStyle],       // HubBatc
        50 => &[DarkLicense],    // DarkLcns
        _ => &[],                // 0 = None / unknown
    }
}

/// The coarse effect group of a NaviCust program (its part-data effect-group
/// byte), classified by what the program does. The group determines which bug a
/// malfunctioning program inflicts (see [`navicust_group_bugs`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectGroup {
    /// Battle abilities and shoes (SprArmor, Block, Shield, Reflect, ShdwShoe, FlotShoe, AntiDmg, AirShoes, UnderSht, Humor, BlckMind).
    BattleAbility = 1,
    /// HP memory and overworld utility (HP+N, Press, EngyChng, Alpha).
    Hp = 2,
    /// Folder / custom-gauge / regular memory (Custom, MegFldr, Reg+5).
    FolderCustom = 3,
    /// Panel-set programs (SetGreen … SetHoly).
    PanelSet = 4,
    /// The fast-gauge program (FstGauge).
    CustomGauge = 5,
    /// Encounter modifiers (SneakRun, OilBody, Fish, Battery, Jungle).
    Encounter = 6,
    /// Buster upgrades (BrakBust, Atk/Speed/Charge +1).
    Buster = 7,
    /// VS-only support navis (Rush, Beat, Tango).
    SupportNavi = 8,
    /// PowerAttack programs (BrakChrg, WeapLV+1).
    PowerAttack = 9,
    /// Battle-reward modifiers (Collect).
    Drops = 10,
    /// The buster-max program (BustrMAX).
    BusterMax = 11,
    /// The giga-folder program (GigFldr1).
    GigaFolder = 12,
    /// The Hub Style bundle (HubBatc).
    Bundle = 13,
    /// The dark-license program (DarkLcns).
    DarkLicense = 14,
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
    /// Battle HP Drain Bug — HP drains steadily during battle (`0x11`).
    BattleHpDrain,
    /// Battle Panel Change Bug — MegaMan's panels start as a fixed type, worse at
    /// higher level (`0x17`).
    PanelChange,
    /// Battle Result Bug — only Zenny drops from battles (`0x1d`).
    BattleResult,
    /// Custom Screen HP Drain Bug — HP drains while on the custom screen (`0x16`).
    CustomHpDrain,
    /// Custom Gauge Bug — the custom gauge is permanently slowed (`0x18`).
    CustomGauge,
    /// Player Movement Bug — MegaMan is locked into a movement state (`0x10`).
    PlayerMovement,
    /// Modified Shot Bug — the B-button PowerAttack is replaced (`0x0c`).
    ModifiedShot,
    /// Raised Encounter Rate Bug — random encounters appear more often (`0x19`).
    EncounterRate,
    /// Support Bug — the support navi is disabled (`0x1c`).
    Support,
    /// Buster Bug — the MegaBuster and ChargeShot may fire blanks (`0x07`).
    Buster,
    /// Auto Bug (BustrMAX) — every selected chip is used immediately and Zeta
    /// PAs fire constantly (`0x1e`).
    AutoChipUse,
    /// Auto Bug (GigFldr1) — the panel MegaMan steps off becomes a Swamp panel
    /// (`0x1f`).
    SwampPanel,
    /// Auto Bug (HubBatc) — MegaMan's max HP is halved (`0x2b`).
    HalveHp,
    /// Auto Bug (DarkLcns) — one slot is lost from the custom screen (`0x13`).
    LoseCustomSlot,
}

/// The bug type(s) inflicted when programs of `group` are bugged.
/// Reverse-engineered from the bug-dispatch jump table at `0x0803c3e8` (A6BE) —
/// each group's handler writes a bug-status field, with the magnitude scaled by
/// the bugged count — and named per the in-game bug list. The blank part and
/// BugStop (group `0`) never bug.
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
        G::BattleAbility => &[B::PlayerMovement],
        G::Hp => &[B::BattleHpDrain],
        G::FolderCustom => &[B::CustomHpDrain],
        G::PanelSet => &[B::PanelChange],
        G::CustomGauge => &[B::CustomGauge],
        G::Encounter => &[B::EncounterRate],
        G::Buster => &[B::Buster],
        G::SupportNavi => &[B::Support],
        G::PowerAttack => &[B::ModifiedShot],
        G::Drops => &[B::BattleResult],
        G::BusterMax => &[B::AutoChipUse],
        G::GigaFolder => &[B::SwampPanel],
        G::Bundle => &[B::HalveHp],
        G::DarkLicense => &[B::LoseCustomSlot],
    }
}

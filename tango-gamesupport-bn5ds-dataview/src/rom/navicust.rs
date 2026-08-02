//! NaviCust program effects, cataloged by part template id (the part
//! `id >> 2`, i.e. ignoring the four colour variants).
//!
//! These are the GBA game's, unchanged: the cart's part table is
//! byte-identical to BN5's for all 192 entries (see [`super::Offsets`]),
//! so a program's id means the same thing here and the effects it
//! grants are the ones reverse-engineered out of the GBA build — see
//! `tango-gamesupport-bn5-dataview`'s copy of this table for where they
//! come from, and for the bug catalogue this one leaves out (nothing
//! reads it).
//!
//! Only what the save layer needs is here: the effects that move a
//! navi's max HP and the folder's Mega/Giga limits.
//!
//! The port adds four templates of its own on the end of the table
//! (48-51), all of them about the battle's Navi Change panel; they are
//! listed so the account stays complete.

/// A single mechanical effect a NaviCust program grants when installed.
/// Only the ones this crate reads are named; everything else a program
/// does is [`Other`](NavicustEffect::Other).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavicustEffect {
    /// Max HP `+N`.
    MaxHp(u16),
    /// Mega-chip folder limit `+N`.
    MegaLimit(u8),
    /// Giga-chip folder limit `+N`.
    GigaLimit(u8),
    /// A program whose effect is real but reaches nothing this crate
    /// reads — a buster upgrade, a shoe, an encounter modifier. Kept so
    /// the table stays a full account of which ids do something.
    Other,
}

/// The effects of NaviCust part `id` (the colour variants `id >> 2`
/// share an effect), or `&[]` for the blank part and unknown ids.
pub fn navicust_part_effects(id: usize) -> &'static [NavicustEffect] {
    use NavicustEffect::*;
    match id >> 2 {
        1 => &[Other],                       // SprArmr
        2 => &[Other],                       // Custom1
        3 => &[Other],                       // Custom2
        4 => &[MegaLimit(1)],                // MegFldr1
        5 => &[MegaLimit(2)],                // MegFldr2
        6 => &[GigaLimit(1)],                // GigFldr1
        7..=34 => &[Other],                  // FstBarr … GigaVirs
        35..=41 => &[Other],                 // Attck+1 … ChargMAX
        42 => &[MaxHp(50)],                  // HP+50
        43 => &[MaxHp(100)],                 // HP+100
        44 => &[MaxHp(200)],                 // HP+200
        45 => &[MaxHp(300)],                 // HP+300
        46 => &[MaxHp(400)],                 // HP+400
        47 => &[MaxHp(500)],                 // HP+500
        // The port's own four, past where the GBA game's table stops:
        // two that buy Navi Changes in a battle, one that buys support
        // from the team, and one that runs. None of them moves HP or a
        // folder limit.
        48..=51 => &[Other],                 // NavChg+1, NavChg+2, Spport, RUN!
        _ => &[],                            // 0 = None / unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HP programs are the last block of the table, and the folder
    /// limits sit where the GBA game puts them.
    #[test]
    fn the_table_reads_the_gba_games_ids() {
        // Every colour variant of HP+100 grants the same 100.
        for id in 43 * 4..44 * 4 {
            assert_eq!(navicust_part_effects(id), &[NavicustEffect::MaxHp(100)], "id {id}");
        }
        assert_eq!(navicust_part_effects(4 * 4), &[NavicustEffect::MegaLimit(1)]);
        assert_eq!(navicust_part_effects(6 * 4), &[NavicustEffect::GigaLimit(1)]);
        // The blank part grants nothing.
        assert_eq!(navicust_part_effects(0), &[]);
    }
}

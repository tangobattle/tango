//! The shared chip-use tracker: the per-tick edge detector game
//! pollers embed. What a game's chip RAM MEANS stays in the game — its
//! poller reads the raw pieces into a [`LoadedChip`]; the readings
//! never leave the poller, only the use events it reports into the
//! [`EventSink`](tango_match::telemetry::EventSink). (Games whose
//! battle system is not hand-shaped at all — BCC's acting-chip turns,
//! vanilla exe45's dealt queue — keep their own trackers in their own
//! crates.)

use tango_match::telemetry::EventSink;

/// One tick's reading of a player's loaded chip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadedChip {
    /// The chip id the game reports loaded (the one to fire next).
    pub id: u16,
    /// The game's own fire cursor: how many of this hand's chips have
    /// fired. It's what makes back-to-back duplicate picks readable as
    /// distinct fires.
    pub fires: u16,
}

/// Chip-use detection for the hand-cursor contract every mainline
/// family follows: the game keeps a per-player hand of picked chips and
/// a counter of how many have fired (bn1's console-local stack,
/// bn2/bn3's picked-minus-remaining, the fired-count blocks of
/// bn4/bn5/bn6, bn5ds and the exe45 PvP patch). The reading
/// is the chip loaded next with the counter as [`LoadedChip::fires`],
/// `None` when the hand is spent — so a fire IS the counter stepping up
/// (the chip used is the reading that departed), a spent hand's last
/// fire is the reading clearing, and a re-commit (counter reset, new
/// ids) is silent. No emission while the player's own custom screen is
/// open (the game may rewrite the block mid-pick) or when the owner is
/// at 0 HP (the KO frame clears the loser's block — teardown, not a
/// use).
#[derive(Clone, Default)]
pub struct HandChipTracker {
    round: u32,
    prev: Option<LoadedChip>,
}

impl HandChipTracker {
    /// One tick's reading for the tracked player, plus that player's
    /// own custom flag and HP for the suppression guards.
    pub fn tick(
        &mut self,
        round: u32,
        reading: Option<LoadedChip>,
        custom_open: bool,
        own_hp: u16,
        player: usize,
        events: &EventSink,
    ) {
        if self.round != round {
            *self = Self { round, prev: None };
        }
        if reading == self.prev {
            return;
        }
        if !custom_open {
            match (self.prev, reading) {
                (Some(p), Some(c)) if c.fires > p.fires => {
                    events.chip_used(player, p.id);
                }
                (Some(p), None) if own_hp != 0 => {
                    events.chip_used(player, p.id);
                }
                _ => {}
            }
        }
        self.prev = reading;
    }
}

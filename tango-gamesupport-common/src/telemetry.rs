//! Shared chip-use trackers: the per-tick edge detectors game pollers
//! embed. What a game's chip RAM MEANS stays in the game — its poller
//! reads the raw pieces into a [`LoadedChip`] and picks the tracker
//! whose contract they follow; the readings never leave the poller,
//! only the use events it reports into the
//! [`EventSink`](tango_match::telemetry::EventSink).

use tango_match::telemetry::EventSink;

/// One tick's reading of a player's loaded chip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoadedChip {
    /// The chip id the game reports loaded (the one to fire next).
    pub id: u16,
    /// The game's own fire counter — whatever moves exactly when a chip
    /// fires: the hand cursor on the counter games, bn5ds's remaining
    /// count, 0 always on a game with none (bn5's bare cell). It's what
    /// makes back-to-back duplicate picks readable as distinct fires.
    pub fires: u16,
}

/// Chip-use detection for the hand-cursor contract: the game keeps a
/// per-player hand of picked chips and a counter of how many have fired
/// (bn1's console-local stack, bn2/bn3's picked-minus-remaining,
/// bn4/bn6's and the exe45 PvP patch's fired-count blocks). The reading
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

/// Chip-use detection for the loaded-cell contract (bn5, and bn5ds with
/// its hand-count [`fires`](LoadedChip::fires)): the cell holds the
/// chip loaded next and a departure is that chip being used — EXCEPT
/// the first departure of each custom cycle, which is the new selection
/// landing on top of whatever was left. The cell's counter (where the
/// game has one at all) doesn't step monotonically, so the load must be
/// told apart by position: opening the player's own custom screen arms
/// a pending load, and the next transition consumes it. Transitions
/// before the round's first custom cycle are init writes, not uses; a
/// departure from a player at 0 HP is the KO frame's cell clear.
#[derive(Clone, Default)]
pub struct LoadedCellTracker {
    round: u32,
    prev: Option<LoadedChip>,
    prev_custom: bool,
    /// A custom cycle opened and its selection hasn't landed yet.
    load_pending: bool,
    /// A custom cycle has opened this round at all — nothing before the
    /// first one can be a use.
    any_cycle: bool,
}

impl LoadedCellTracker {
    /// One tick's reading for the tracked player, plus that player's
    /// own custom flag and HP.
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
            *self = Self {
                round,
                ..Default::default()
            };
        }
        if custom_open && !self.prev_custom {
            self.load_pending = true;
            self.any_cycle = true;
        }
        self.prev_custom = custom_open;
        if reading != self.prev {
            if self.load_pending {
                self.load_pending = false;
            } else if let (true, Some(p)) = (self.any_cycle, self.prev) {
                if own_hp != 0 {
                    events.chip_used(player, p.id);
                }
            }
            self.prev = reading;
        }
    }
}

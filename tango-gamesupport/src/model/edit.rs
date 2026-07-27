//! The save editors' staged-edit types ([`ChipEdit`] & friends) and
//! their in-memory appliers: resolve against the ROM assets, write
//! through the dataview's mutable views, and rebuild any derived
//! mirrors (anti-cheat folder/library, materialized auto battle data)
//! so they stay in sync. No disk I/O — the commit path only checksums
//! and writes.
//!
//! [`apply`] keeps everything the *model* derives in step by itself. A
//! frontend's own derived state — anything it baked out of the assets
//! for drawing — it cannot know about, so `apply` reports what it
//! invalidated and leaves the re-deriving to the caller. See
//! [`Invalidation`].

use crate::model::SaveModel;

/// A single folder edit staged by the folder editor. Applied to the
/// save save in memory; not persisted to disk until the user hits
/// Save (the host's save-edit commit).
#[derive(Debug, Clone)]
pub enum ChipEdit {
    /// Add chip `chip_id` with `code` to the first empty folder slot.
    AddChip {
        chip_id: usize,
        code: tango_dataview::save::ChipCode,
    },
    /// Empty `slot`.
    RemoveChip { slot: usize },
    /// Reorder: move the chip at `from` to `to` (an ordered move that shifts
    /// the chips in between). Both slots must be filled — the editor never
    /// drags an empty slot or drops into a gap. REG/TAG slot pointers follow
    /// the moved chips.
    MoveChip { from: usize, to: usize },
    /// Empty every folder slot (and clear REG/TAG).
    ClearFolder,
    /// Toggle `slot` as the folder's Regular chip (clear if already set).
    ToggleRegular { slot: usize },
    /// Set (or clear, with `None`) the folder's Tag chip pair.
    SetTags(Option<[usize; 2]>),
    /// Write `slot` directly: install the chip (replacing whatever occupied
    /// the slot), or empty it with `None` — with **no** compaction and no
    /// REG/TAG bookkeeping. For slot-addressed decks (BCC's program deck
    /// board), where a gap is a legal state; the BN folder editors stick to
    /// the compacting `AddChip`/`RemoveChip` pair above.
    SetChip {
        slot: usize,
        chip: Option<tango_dataview::save::Chip>,
    },
}

/// A single navicust edit staged by the navicust editor. Applied to the
/// save save in memory; not persisted to disk until the user hits Save.
#[derive(Debug, Clone)]
pub enum NavicustEdit {
    /// Place a part into the first empty navicust slot.
    AddPart(tango_dataview::save::NavicustPart),
    /// Empty navicust slot `slot`.
    RemovePart { slot: usize },
    /// Remove every installed part.
    ClearAll,
}

/// A staged navi-selection edit. Applied to the save save in memory;
/// not persisted to disk until the user hits Save.
#[derive(Debug, Clone)]
pub enum NaviEdit {
    /// Set the equipped navi to this index.
    SetNavi(usize),
}

/// A single BN5/BN6 patch-card edit staged by the editor. Applied to the
/// save save in memory; not persisted to disk until the user hits Save.
#[derive(Debug, Clone)]
pub enum PatchCard56Edit {
    /// Register patch card `id` (append to the list, enabled).
    AddCard { id: usize },
    /// Unregister the patch card in `slot` (shift the rest up).
    RemoveCard { slot: usize },
    /// Reorder: move the card at `from` to `to` (an ordered move that shifts
    /// the cards in between). The registered list is dense, so both ends are
    /// always valid.
    MoveCard { from: usize, to: usize },
    /// Unregister every patch card.
    ClearAll,
}

/// A single auto-battle-data edit staged by the editor. Applied to the
/// save save in memory; not persisted to disk until the user hits
/// Save. The deck is derived from per-chip use counts, so these set
/// those counts; the applier rebuilds the materialized deck after each
/// so the preview shows the change live.
#[derive(Debug, Clone)]
pub enum AutoBattleDataEdit {
    /// Set chip `id`'s primary use count.
    SetUseCount { id: usize, count: usize },
    /// Set chip `id`'s secondary use count (Standard chips only).
    SetSecondaryUseCount { id: usize, count: usize },
    /// Zero every chip's use counts, emptying the deck.
    ClearAll,
}

/// A game's own staged edit — the extension point for save sections
/// whose model lives in the game's crates rather than here (BN4's
/// slot-based Mod Cards). The game's UI crate defines the edit type and
/// its application; it reaches the concrete save through
/// [`tango_dataview::save::AsAny`]. Applications follow the same
/// contract as the shared appliers: mutate the in-memory save (keeping
/// any anti-cheat mirror in sync), no disk I/O.
pub trait GameEdit: std::fmt::Debug + Send + Sync {
    #[must_use = "an edit can invalidate frontend-derived art; see Invalidation"]
    fn apply(&self, save: &mut SaveModel) -> Invalidation;
}

/// One staged edit to the save save, unifying the per-editor edit
/// types so hosts can route every editor through a single effect.
#[derive(Debug, Clone)]
pub enum Edit {
    Chips(ChipEdit),
    Navicust(NavicustEdit),
    Navi(NaviEdit),
    PatchCard56s(PatchCard56Edit),
    /// A game's own edit (BN4's Mod Cards, or anything else without a
    /// shared model) — see [`GameEdit`]. `Arc` so `Edit` stays `Clone`.
    Game(std::sync::Arc<dyn GameEdit>),
    AutoBattleData(AutoBattleDataEdit),
}

/// What an applied edit invalidated in state the *frontend* derived from
/// this save, and which only it can rebuild.
///
/// The model's own derived caches — the anti-cheat folder/library
/// mirror, the materialized navicust and auto-battle-data WRAM — are
/// kept in step by the appliers themselves and never appear here. This
/// is only about what a frontend baked for drawing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Invalidation {
    /// The navicust's contents or its very existence changed, so any
    /// picture of the grid drawn from this save is stale. Note that the
    /// navicust *editor*'s own edits don't set this: it paints from the
    /// live view and only needs a re-bake once, at commit.
    pub navicust_render: bool,
}

impl Invalidation {
    pub fn navicust_render() -> Self {
        Self { navicust_render: true }
    }
}

//! The BN5 Double Team DS save.
//!
//! A cartridge dump is not one save: the game's file select offers two
//! in-game files, and the flash carries each as its own mirrored pair
//! of blocks. So the dump parses to a [`SaveSet`], which hands out one
//! [`Save`] per file — each a save in its own right, with its own
//! folders to read and edit. A `Save` keeps the whole dump behind it
//! (that is what gets written back to the cartridge) but only ever
//! reads and writes its own file's block.
//!
//! Which file the *cartridge* plays is the one the game itself calls
//! most recently saved — [`SaveSet::current`], off the generation
//! counters in the blocks' footers. Everything above reads it there:
//! the editor opens on it, a session boots it, the priming walk steers
//! the game's own file select to it, and a recording gets it for free
//! by storing the same bytes. Picking the other file is therefore an
//! edit to the cartridge rather than a note beside it — see
//! [`Save::make_current`].
//!
//! Recognition is settled: the game stamps its own format tag into
//! every block it formats, and finding that tag intact is what puts a
//! dump in the save picker. The interior is mapped as far as the GBA
//! game's editor reaches: the chip folders and pack, the
//! equipped-folder cluster, the NaviCust (grid, parts and colour bar),
//! the navi records the HP comes out of, the auto-battle data, the
//! GBA-slot [`Cross`] byte, and — read out of the ARM9's own save code
//! — the flash checksums, so an edited block can be made acceptable to
//! the game again.
//!
//! What the GBA game has and this cart hasn't: patch cards, which
//! Double Team dropped, so there is no view for them. The rest of the
//! layout was found through the ARM9's own section table (the save
//! object at US `0x021701d4`, filled at `0x020214bc`, whose fields are
//! the sections' addresses) plus the GBA game's own shapes.

use tango_gamesupport_common_dataview::save::Error;

/// The flash the cart carries: 256 KiB. That is what the emulator
/// mounts and hands back, so it is the length a dump is canonicalized
/// to — but not the length one has to arrive at, see [`SaveSet::parse`].
pub const SIZE: usize = 0x4_0000;

/// What a byte of flash reads as before anything is written to it, and
/// so what a dump that stops short of [`SIZE`] is padded out with.
const ERASED: u8 = 0xff;

/// The game divides the flash into blocks of this size and writes a
/// save as a mirrored pair of them.
pub const BLOCK_SIZE: usize = 0xa000;

/// Where in each block the game stamps [`MAGIC`].
pub const MAGIC_OFFSET: usize = 0x9f08;

/// The format tag the game writes at the tail of every block it
/// formats. The string is baked into both the US and JP ROMs, so one
/// check covers bn5ds and exe5ds alike.
///
/// The head of a block is live data, not a header — fingerprinting it
/// rejected real saves whose dumper left different bytes there.
pub const MAGIC: &[u8; 16] = b"EXE DS SAVE 0006";

/// Which file-select slot a block's contents belong to (0 or 1), at
/// the tail of the footer line `00 00 <slot> 01` that follows
/// [`MAGIC`]. The mount code in the ARM9 walks every block and keeps,
/// per file, the intact block with the highest generation counter.
pub const FILE_SLOT_OFFSET: usize = 0x9f1a;

/// The block's save-generation counter (u32 LE), bumped on every save.
/// A file alternates between two mirrored pairs of blocks, so the pair
/// with the highest counter holds the file's current contents — and
/// the highest counter overall belongs to whichever file was saved
/// most recently. The first-save format stamps every block with the
/// same baked starting value (0x3b7 observed), and an untouched file
/// keeps it, so only real saves separate the counters.
pub const GENERATION_OFFSET: usize = 0x9f1c;

/// The save image: the first 0x8440 bytes of a block are the data the
/// game loads and checksums; from there to the footer is fill.
pub const SAVE_IMAGE_SIZE: usize = 0x8440;

/// The save image's interior checksum: a u32 byte-sum of the whole
/// image with these four bytes excluded. Verified at load, separately
/// from the footer pairs — a save whose data changed without it is
/// rejected and the file select boots the new-game path (probed live,
/// July 2026). A byte-sum is permutation-invariant, which is how chip
/// *reorders* shipped without rebuilding it: rearranged bytes keep
/// their sum. Sits inside the image, so cs2 covers it and it must be
/// rebuilt first.
pub const INTERIOR_CHECKSUM_OFFSET: usize = 0x9c;

/// The checksum pairs at each block's tail, as four u16 LE:
/// `[cs1, !cs1, cs2, !cs2]`, each value paired with `0x10000 - value`.
/// `cs2` covers the save image; `cs1` covers the footer from +0x9f04
/// through +0x9f20 — the cs2 pair, the format tag, the file-slot line
/// and the generation counter — so cs1 must be recomputed after cs2
/// lands. Read out of the ARM9's own save code (US build: writer at
/// 0x02020b68, verifier at 0x02020bfc, the sum itself at 0x02020c90).
pub const CHECKSUM_OFFSET: usize = 0x9f00;

/// The stretch `cs1` sums: from the cs2 pair through the end of the
/// generation counter.
const FOOTER_SUM_START: usize = 0x9f04;
const FOOTER_SUM_END: usize = 0x9f20;

/// The chip folders: 3 folders of 30 chips each, one u16 LE per chip in
/// the same encoding the GBA game uses (id in bits 0..9, code in bits
/// 9..16). Verified byte-identical across a generation's mirrored pair.
pub const FOLDER_OFFSET: usize = 0x498;

/// Folders per save. The array at [`FOLDER_OFFSET`] holds exactly three
/// before the entries stop decoding, matching the GBA game.
pub const NUM_FOLDERS: usize = 3;

/// The chip pack: one 12-byte entry per chip id — a count byte per
/// code slot (the stat table's `codes[4]` order, which is the pack
/// API's "variant"), then a u16 LE per slot holding that code's
/// acquisition key. The 320 entries fill this section of the ARM9's
/// own section table exactly; ids past the table — Program Advances
/// and the DS navi chips — have no pack slot, as on GBA.
pub const PACK_OFFSET: usize = 0x54c;
pub const NUM_PACK_CHIPS: usize = 320;
const PACK_ENTRY_SIZE: usize = 12;

/// The acquisition keys count *down* from this as codes are obtained —
/// a maxed save observed 648 of them running 0x7fff..0x7d78, handed out
/// in library order rather than by id. 0 means the code was never
/// owned, which is how a code slot the chip doesn't even have reads.
const PACK_KEY_FIRST: u16 = 0x7fff;

/// Which folder is equipped, as an index into [`FOLDER_OFFSET`]'s
/// array. Found by the GBA game's own geometry: there the equipped
/// byte sits 0x11 before the navi-stats array with the per-folder
/// Regular-chip bytes right behind it, and the same cluster sits at
/// the same distance from [`NAVI_STATS_OFFSET`] here — confirmed
/// against a cart known to have its second folder selected.
pub const EQUIPPED_FOLDER_OFFSET: usize = 0x2e99;

/// The per-folder Regular chip: one byte per folder, the chip's index
/// into that folder, with 0xff (anything past 29) meaning none — the
/// GBA game's own encoding.
pub const REGULAR_CHIP_OFFSET: usize = 0x2e9a;

/// The navi stats array: 13 slots of 0x60 bytes (slot 0 the player,
/// 1.. the team link navis, as on GBA), each starting u16 LE base max
/// HP, current HP, effective max HP. It is the anchor
/// [`EQUIPPED_FOLDER_OFFSET`] was found against, and what
/// [`NaviView`](tango_gamesupport_common_dataview::save::NaviView)
/// reports HP out of.
pub const NAVI_STATS_OFFSET: usize = 0x2eaa;

/// The navi record array proper, as the game's own accessor walks it:
/// record `i` at `NAVI_RECORD_OFFSET + i * 0x60`, record 0 being the
/// player's. [`NAVI_STATS_OFFSET`] is the HP triple 0x3e into record 0
/// — the same array reached from the other end.
///
/// Read out of the ARM9: the save object at US `0x021701d4` holds this
/// block's address at `+0x74`, and the accessor at `0x02007784` indexes
/// it (through an identity remap table) by `0x60`.
const NAVI_RECORD_OFFSET: usize = 0x2e6c;

/// How long a navi record is, and how far into one its HP triple sits —
/// [`NAVI_STATS_OFFSET`] is record 0's.
const NAVI_RECORD_SIZE: usize = 0x60;
const NAVI_STATS_INTO_RECORD: usize = NAVI_STATS_OFFSET - NAVI_RECORD_OFFSET;

/// How many navis the record array holds: MegaMan and both teams' six.
pub const NUM_NAVIS: usize = 13;

/// MegaMan's karma, a u16 at record 0 `+0x44` — two bytes past the HP
/// triple. 0 is fully dark, [`KARMA_MAX`] fully light.
///
/// Read out of the ARM9: the alignment-tier getter at US `0x020081c4`
/// reads this halfword through the record accessor and banks it against
/// 1000/500/470 (light at the cap, darker tiers below), and the game's
/// own adjustments clamp it to 0..=1000 (US `0x02008104`). The GBA game
/// keeps the same field at the same distance into its own record 0,
/// which is where its bundled dark/light template saves differ.
const KARMA_INTO_RECORD: usize = 0x44;

/// Where the karma clamp stops: fully light.
pub const KARMA_MAX: u16 = 1000;

/// Karma's anti-tamper mirror: a u32 the game keeps equal to
/// `karma ^ key`, with the key another u32 the save carries. The GBA
/// game writes and verifies the same pair (US Protoman `0x080064d8` /
/// `0x080064f2`: read record 0 `+0x44`, XOR the key, store into its
/// last save section), and this cart's played files hold exactly that
/// relation at these offsets — so a karma write has to land in both
/// places or the file reads as tampered.
const KARMA_MIRROR_OFFSET: usize = 0x5a3c;
const KARMA_KEY_OFFSET: usize = 0x3ac4;

/// How many times using Dark Chips has cost the file a point of max HP:
/// a u16 at `+0x16` of the image's second section, the same distance
/// into it the GBA game keeps its own counter. The ARM9's penalty
/// routine (US `0x02008124`) bumps it after a dark battle while it is
/// under 499 and docks base max HP by one alongside — which is why a
/// fully-played dark file's MegaMan reads 1000 minus this.
pub const DARK_HP_LOSSES_OFFSET: usize = 0x7e;

/// Which GBA-slot cross the player brings, at record 0 `+0x4c` — the
/// byte the game's own file select writes when it finds a cartridge in
/// the DS's GBA slot and the player accepts it. See [`Cross`] for the
/// values.
pub const CROSS_OFFSET: usize = NAVI_RECORD_OFFSET + 0x4c;

/// The folder's Regular memory, in MB, at record 0 `+9`. The GBA game
/// keeps this byte 0x24 before its equipped-folder byte and so does the
/// cart: the whole cluster the GBA game reads from `0x52a8` is record
/// 0 here, at the same distances into it.
pub const REGULAR_MEMORY_OFFSET: usize = NAVI_RECORD_OFFSET + 0x09;

/// Which navi the player is: 0 MegaMan, 1.. a team navi, in the roster
/// order the navi records use. Read out of the ARM9 — the getter at US
/// `0x0208d7a4` reads byte 1 of the save image's first section, and the
/// setter at `+0x14` writes it — and it is the byte the GBA game keeps
/// at its own `0x2941`.
///
/// It is not a battle loadout, though the GBA game's is: this cart
/// hands you your team **in** a battle (the touch screen's NAVI CHANGE
/// panel), and the byte is who the field is currently being played as,
/// which the Liberation missions move. A save carrying a nonzero one
/// boots into that navi's world, not into a net battle — verified by
/// poking it and watching the priming walk land on the PET screen — so
/// nothing here writes it, and a save that has one is treated the way
/// BN5 treats a link navi: no NaviCust of MegaMan's to edit.
pub const NAVI_OFFSET: usize = 0x0001;

/// The team the player brings into a battle: the two navis the touch
/// screen's NAVI CHANGE panel offers, in the order it shows them, one
/// u32 each.
///
/// Read out of the ARM9: the battle's team loader (US `0x02097b84`)
/// asks `0x02097dd4` for the pair, which reads each slot through the
/// getter at `0x02094e4c` — `save_image + 0x8004`, indexed by slot at
/// `+0xc` — and hands the id to the navi-record accessor. A slot holds
/// the navi's id biased by [`TEAM_NAVI_BIAS`], and 0 for an empty one.
///
/// The ids are only half of it: the save load cross-checks each slot
/// against the [`TEAM_MIRROR_OFFSET`] bits and quietly drops any navi
/// whose bit is off, so an edit has to keep both in step — see there.
pub const TEAM_NAVI_OFFSET: usize = 0x8010;
pub const NUM_TEAM_SLOTS: usize = 2;
const TEAM_NAVI_BIAS: u32 = 0x254;

/// The team's mirror in the story-flag bitfield: one bit per navi,
/// MSB-first from [`TEAM_MIRROR_OFFSET`]'s bit `4 + navi` — navi 1 at
/// `0xea & 0x04` through navi 11 at `0xeb & 0x01`, navi 12 spilling
/// into `0xec & 0x80`. The game's own Navi Change machine keeps these
/// bits exactly equal to the set of navis in the two slots, and the
/// save load *enforces* it: a slot whose navi's bit is off reads back
/// as empty (the sanitize happens inside the load frame, which is why
/// no slot poke without the bit ever survived to the battle).
///
/// Found by transplant bisection between a save whose team worked and
/// one whose identical-looking team didn't: one byte, `image + 0xeb`,
/// flipped the battle's NAVI CHANGE panel on, and the bit layout then
/// decoded from five known team/value pairs. Verified by prediction —
/// setting the computed bits for navis this cart never fielded (and
/// even the *other team's* GyroMan) puts them on the panel, working.
///
/// The surrounding bits are other story flags (`0xea` carries real ones
/// on played carts), so writes must mask exactly the twelve navi bits.
pub const TEAM_MIRROR_OFFSET: usize = 0xea;

/// Navi `id`'s mirror bit, as `(byte offset, mask)`.
fn team_mirror_bit(navi: usize) -> (usize, u8) {
    story_flag(TEAM_MIRROR_FLAG_BASE + navi)
}

/// The story-flag bitfield: flag `N` is bit `0x80 >> (N % 8)` of byte
/// `0xa0 + N / 8` — read out of the game's own flag test (US
/// `0x2063d24`: the save object's `+0x44` section pointer is
/// `image + 0xa0`, then the byte/mask split above). The team system's
/// flags live here as two twelve-flag runs:
///
/// - `0x254 + navi`: the [`TEAM_MIRROR_OFFSET`] bits — and the team
///   slot words are these very flag numbers, which is what the `0x254`
///   bias *is*.
/// - `0x268 + navi`: the navi is recruited — what the game's own Navi
///   Change machine offers. Found by searching the flag space for a
///   twelve-flag run splitting exactly by team across both files of
///   two carts, then verified live: clearing ShadowMan's (`0x270`) on
///   a played cart makes the machine's picker skip him.
pub const STORY_FLAGS_OFFSET: usize = 0xa0;
const TEAM_MIRROR_FLAG_BASE: usize = 0x254;
const TEAM_RECRUIT_FLAG_BASE: usize = 0x268;

/// Where the story-rank run starts. The game counts how many of the
/// nine flags from here are set, stopping at the first clear, and uses
/// the count to index everything that grows as the story does — the
/// team navis' chip attack among them. Read out of the counter at US
/// `0x0203c10c`, which tests `(group 3, index 0..8)` through the flag
/// getter at `0x02063d20`; that getter is `N = (group << 8) | index`
/// over this same bitfield, which is what fixes the run at 0x300.
const STORY_RANK_FLAG_BASE: usize = 0x300;
const STORY_RANKS: usize = 9;

/// Story flag `N`, as `(byte offset, mask)`.
fn story_flag(n: usize) -> (usize, u8) {
    (STORY_FLAGS_OFFSET + n / 8, 0x80 >> (n % 8))
}

/// What the PARTY CUSTOMIZER has given a team navi, all four of it in
/// that navi's own record: `+1` the ATTACK its card shows (0-based, so
/// a card reading ATTACK 1 is a zero here), `+0x56` the chip attack it
/// adds, `+0x59` whether the member gives support, `+0x5a` the max HP
/// it adds. The HP lands in the record's *effective* max HP as well —
/// [`NAVI_STATS_OFFSET`]'s third half-word, which is what the screens
/// outside the customizer read a member's HP off.
///
/// Read out of the customizer itself: driving one headless and diffing
/// the whole save image across a session turns up exactly these (plus
/// the play clock, the checksums, and the loadout at
/// [`PARTYCUST_LOADOUT_OFFSET`]), and the game's own apply and remove
/// jump tables (US `0x021e0560` / `0x021e02d0`) write the same fields.
const PARTYCUST_ATTACK_INTO_RECORD: usize = 0x01;
const PARTYCUST_CHIP_ATTACK_INTO_RECORD: usize = 0x56;
const PARTYCUST_SUPPORT_INTO_RECORD: usize = 0x59;
const PARTYCUST_MAX_HP_INTO_RECORD: usize = 0x5a;
const PARTYCUST_EFFECTIVE_HP_INTO_RECORD: usize = NAVI_STATS_INTO_RECORD + 2 * std::mem::size_of::<u16>();

/// The customizer's committed loadouts, at the very end of the save
/// image: one entry per party slot, [`PARTYCUST_LOADOUT_SIZE`] bytes
/// apiece, so the pair runs out to the image's last byte.
///
/// ```text
///   +0x00        the navi the entry was customized for, 0 for none
///   +0x02..0x0c  the item ids of the programs equipped, 0-padded —
///                ten, as many as the widest gauge can hold
///   +0x0c        the ATTACK they add up to
///   +0x0d        the chip attack they add up to
/// ```
///
/// The record fields above are what a battle reads; this is what the
/// customizer's own panel redraws its list and gauge from. A save
/// carrying one without the other shows boosted stats over an empty
/// gauge — which is what writing the record alone and booting the panel
/// does — so both are written together.
///
/// Found by committing known programs in a driven session and diffing
/// the image: `RUN!`, the last entry of the panel's list, is what
/// writes it. The stride is the one the panel's own per-member array
/// uses (US `0x021e0450` and friends index it by `member * 0xe`), and
/// two entries of it reach the image's end exactly.
pub const PARTYCUST_LOADOUT_OFFSET: usize = 0x8424;
const PARTYCUST_LOADOUT_SIZE: usize = 0x0e;
const PARTYCUST_LOADOUT_NAVI: usize = 0x00;
const PARTYCUST_LOADOUT_PROGRAMS: usize = 0x02;
const PARTYCUST_LOADOUT_ATTACK: usize = 0x0c;
const PARTYCUST_LOADOUT_CHIP_ATTACK: usize = 0x0d;

/// How many copies of one party program a member may equip.
///
/// The cart has no such rule of its own — a member's gauge is its only
/// limit, and a session never spends what the file stocks — so this is
/// the editor's, the same cap it puts on copies of one NaviCust part
/// (`MAX_COPIES_PER_PART`, which lives behind the editor model this
/// crate does not depend on).
pub const MAX_COPIES_PER_PARTY_PROGRAM: usize = 9;

/// How many programs one entry can name — as many as the widest gauge
/// in the cart's own table holds, since nothing costs less than a
/// block.
pub const MAX_PARTY_PROGRAMS_EQUIPPED: usize = PARTYCUST_LOADOUT_ATTACK - PARTYCUST_LOADOUT_PROGRAMS;

/// The item counts: one byte per item id, the game's own section table
/// giving it as 0x190 bytes here. The subchips the PET stocks start at
/// id 0xa0 — a played cart's counts there match its Sub Chip menu
/// exactly — and the party programs at 0xb0, where they match the
/// customizer's own xN badges.
pub const ITEM_COUNTS_OFFSET: usize = 0x144c;
const NUM_ITEMS: usize = 0x190;

/// The materialized NaviCust grid: 5x5 cells, each holding a part
/// slot + 1 (0 for an empty cell), then seven bytes the section pads
/// with. The game's own section table gives the section as 0x20 bytes
/// at this offset, immediately before the parts themselves.
pub const NAVICUST_GRID_OFFSET: usize = 0x24dc;
const NAVICUST_GRID_SECTION_SIZE: usize = 0x20;

/// The NaviCust parts: 25 slots of 8 bytes — id, a byte the game keeps
/// zero, column, row, rotation, compressed flag, and two more zeroes —
/// the GBA game's own entry. The section table gives it as 0xc8 bytes,
/// which is exactly the 25.
pub const NAVICUST_PARTS_OFFSET: usize = 0x24fc;
const NUM_NAVICUST_SLOTS: usize = 25;
const NAVICUST_PART_SIZE: usize = 8;

/// The NaviCust colour bar: six bytes holding the distinct part colours
/// in placement order, zero-padded — the shape and encoding the GBA
/// game uses. Found by computing what the bar for a played cart's grid
/// must read and finding that run in the save image.
pub const NAVICUST_COLOR_BAR_OFFSET: usize = 0x6910;
const NAVICUST_COLOR_BAR_LEN: usize = 6;

/// The auto-battle data: the 42-slot deck the game materializes, then
/// the two use-count arrays it ranks chips by. Both arrays are u16 per
/// chip id and
/// [`NUM_AUTO_BATTLE_DATA_CHIPS`](crate::NUM_AUTO_BATTLE_DATA_CHIPS)
/// long — found through the ARM9's own "count one use" helpers (US
/// `0x0209952c` bumps the first, `0x02099518` the second), which is
/// also what tells the two apart.
pub const AUTO_BATTLE_DATA_OFFSET: usize = 0x334c;
pub const CHIP_USE_COUNT_OFFSET: usize = 0x4944;
pub const SECONDARY_CHIP_USE_COUNT_OFFSET: usize = 0x4c24;

/// How many slots the materialized deck holds.
const NUM_AUTO_BATTLE_DATA_SLOTS: usize = 42;

/// Where the deck's eight combo slots sit in it. The GBA game leaves
/// them empty and the shared materializer does too, but this cart
/// writes real markers there (`0x8000 | n` on a played cart), so a
/// rebuild leaves them alone rather than blanking something the game
/// put there.
const AUTO_BATTLE_DATA_COMBO_SLOTS: std::ops::Range<usize> = 33..41;

/// What the PARTY CUSTOMIZER has given a team navi — the three numbers
/// its card shows over the customizer's own gauge, and whether the
/// member gives support. Where each lives is
/// [`PARTYCUST_ATTACK_INTO_RECORD`] and friends.
///
/// The same shape says what a single program grants, since equipping one
/// is exactly adding its bonus (see
/// [`PartyProgram::bonus`](crate::rom::PartyProgram::bonus)).
///
/// `attack` is the game's own byte, which is 0-based: a card reading
/// ATTACK 1 is a zero here.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct PartycustBonus {
    pub max_hp: u16,
    pub attack: u8,
    pub chip_attack: u16,
    pub support: bool,
}

impl PartycustBonus {
    /// This bonus with one more program's equipped over it, or `None` when
    /// a field would run past what its record holds.
    fn plus(self, other: Self) -> Option<Self> {
        Some(Self {
            max_hp: self.max_hp.checked_add(other.max_hp)?,
            attack: self.attack.checked_add(other.attack)?,
            chip_attack: self.chip_attack.checked_add(other.chip_attack)?,
            support: self.support || other.support,
        })
    }
}

/// The party, as the game's own PARTY STATUS card and its CUSTOM panel
/// read it together: who each of the two slots fields, and what the
/// PARTY CUSTOMIZER has put on them.
///
/// A slot's programs are the game's own list
/// ([`PARTYCUST_LOADOUT_OFFSET`]), not a guess at one: the panel
/// records the item id of everything it commits. An entry naming a navi
/// the slot no longer fields is stale — the game rebuilds it whenever
/// the member changes — and reads as nothing equipped.
///
/// One type over a shared or exclusive borrow of the save, the shape
/// [`NavicustView`] has: the reading half is every method here, the
/// writing half the ones that need [`DerefMut`](std::ops::DerefMut).
/// Whatever a slot's programs cost comes off the cart, so the methods
/// that price them take the cart rather than the view holding one.
///
/// The gauge is the only thing that limits a loadout — see
/// [`can_add_party_program`](PartyView::can_add_party_program).
pub struct PartyView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> PartyView<S> {
    /// The navi in slot `slot`, or `None` for an empty slot (and for a
    /// slot past the two the panel has).
    pub fn navi(&self, slot: usize) -> Option<usize> {
        if slot >= NUM_TEAM_SLOTS {
            return None;
        }
        let raw = u32::from_le_bytes(
            self.save.active()[TEAM_NAVI_OFFSET + slot * std::mem::size_of::<u32>()..][..4]
                .try_into()
                .unwrap(),
        );
        raw.checked_sub(TEAM_NAVI_BIAS).map(|navi| navi as usize)
    }

    /// The navis a slot may be offered: the ones this file has
    /// recruited, read from the same flags the game's own Navi Change
    /// machine reads (`TEAM_RECRUIT_FLAG_BASE`; see
    /// [`STORY_FLAGS_OFFSET`]). A file early in its story is offered
    /// the teammates it has met, exactly as the machine would offer
    /// them.
    ///
    /// The battle would take more: with the mirror bit set it fields
    /// any of the twelve — verified live with GyroMan fighting for a
    /// Team Colonel file — and [`set_navi`](PartyView::set_navi) stays
    /// willing so a damaged pair can always be repaired. But the
    /// recruit flags are the game's own account of who this file may
    /// field, so they are the editor's too.
    pub fn choices(&self) -> Vec<usize> {
        (1..NUM_NAVIS)
            .filter(|&navi| {
                let (at, mask) = story_flag(TEAM_RECRUIT_FLAG_BASE + navi);
                self.save.active()[at] & mask != 0
            })
            .collect()
    }

    /// How many blocks slot `slot`'s gauge holds. Zero for an empty
    /// slot, which is nothing to customize.
    pub fn capacity(&self, slot: usize, assets: &crate::rom::Assets) -> u8 {
        self.navi(slot).map(|navi| assets.partycust_capacity(navi)).unwrap_or(0)
    }

    /// The programs slot `slot` has equipped, in the order the panel
    /// put them on, as indexes into the cart's own table.
    pub fn programs(&self, slot: usize, assets: &crate::rom::Assets) -> Vec<usize> {
        let Some(navi) = self.navi(slot) else { return Vec::new() };
        let entry = self.save.partycust_loadout(slot);
        if entry[PARTYCUST_LOADOUT_NAVI] as usize != navi {
            return Vec::new();
        }
        entry[PARTYCUST_LOADOUT_PROGRAMS..PARTYCUST_LOADOUT_ATTACK]
            .iter()
            .take_while(|&&item| item != 0)
            .filter_map(|&item| {
                (0..crate::NUM_PARTY_PROGRAMS)
                    .find(|&index| assets.party_program(index).map(|p| p.item_id()) == Some(item as usize))
            })
            .collect()
    }

    /// How much of slot `slot`'s gauge those fill.
    pub fn cost(&self, slot: usize, assets: &crate::rom::Assets) -> u8 {
        self.programs(slot, assets)
            .into_iter()
            .map(|index| cost_of(index, assets))
            .sum()
    }

    /// Whether one more of `program` would go on slot `slot`: the gauge
    /// has to have room for it, and the member may not already be
    /// carrying [`MAX_COPIES_PER_PARTY_PROGRAM`] of it.
    ///
    /// What the file stocks does not come into it. The counts at
    /// [`ITEM_COUNTS_OFFSET`] are what the player owns, and a
    /// customizer session never spends them — equipping a program
    /// leaves them untouched, so there is nothing here for a stock
    /// check to conserve.
    pub fn can_add_party_program(&self, slot: usize, assets: &crate::rom::Assets, program: usize) -> bool {
        if program >= crate::NUM_PARTY_PROGRAMS {
            return false;
        }
        let already = self
            .programs(slot, assets)
            .into_iter()
            .filter(|&index| index == program)
            .count();
        already < MAX_COPIES_PER_PARTY_PROGRAM
            && self.cost(slot, assets) + cost_of(program, assets) <= self.capacity(slot, assets)
    }
}

impl<S: std::ops::DerefMut<Target = Save>> PartyView<S> {
    /// Put `navi` in slot `slot`, or empty it with `None`. Refuses a
    /// slot the panel hasn't got and a navi outside the record array
    /// (0, MegaMan, included — he is who the panel changes *from*).
    ///
    /// Keeps the [`TEAM_MIRROR_OFFSET`] bits equal to the slots the way
    /// the game's own machine does, so the load's cross-check passes,
    /// and leaves the pair packed the way the machine compacts it: a
    /// slot that changes members loses its customizer loadout, and the
    /// navi that left is stripped back to no bonus.
    pub fn set_navi(&mut self, slot: usize, navi: Option<usize>) -> bool {
        if !self.save.set_team_navi(slot, navi) {
            return false;
        }
        self.save.pack_team();
        true
    }

    /// Dress slot `slot` in exactly `programs`, the way a session that
    /// ended on the panel's `RUN!` leaves it. `false` (no write) for a
    /// slot with no member, and for a set longer than the widest gauge.
    pub fn set_party_programs(
        &mut self,
        slot: usize,
        programs: impl IntoIterator<Item = usize>,
        assets: &crate::rom::Assets,
    ) -> bool {
        self.save.set_party_programs(slot, programs, assets)
    }

    /// Put one more of `program` on slot `slot`, at the end of the list
    /// where the panel puts one. `false` (no write) when
    /// [`can_add_party_program`](PartyView::can_add_party_program) would refuse it.
    pub fn add_party_program(&mut self, slot: usize, assets: &crate::rom::Assets, program: usize) -> bool {
        if !self.can_add_party_program(slot, assets, program) {
            return false;
        }
        let mut programs = self.programs(slot, assets);
        programs.push(program);
        self.save.set_party_programs(slot, programs, assets)
    }

    /// Take the program in position `at` back off. `false` (no write)
    /// past what the slot has equipped.
    pub fn remove_party_program(&mut self, slot: usize, assets: &crate::rom::Assets, at: usize) -> bool {
        let mut programs = self.programs(slot, assets);
        if at >= programs.len() {
            return false;
        }
        programs.remove(at);
        self.save.set_party_programs(slot, programs, assets)
    }

    /// Take everything off slot `slot`, the customizer's own clear.
    pub fn clear_party_programs(&mut self, slot: usize, assets: &crate::rom::Assets) -> bool {
        self.save.set_party_programs(slot, [], assets)
    }
}

/// What one of `program` costs a gauge.
fn cost_of(program: usize, assets: &crate::rom::Assets) -> u8 {
    assets.party_program(program).map(|program| program.cost()).unwrap_or(0)
}

/// The MegaMan a save brings to a battle: plain, or one of the two
/// crosses the game unlocks from a cartridge in the GBA slot.
///
/// This is the game's own byte, in the game's own encoding — writing it
/// is what its file select does after asking. Slot 2 is never emulated
/// here: what the cartridge would have bought is what the player picks,
/// and PvP re-asserts it (see the crate's `pvp` module) because the
/// file select clears the byte on every boot, cartridge or not.
///
/// BassCross is two values because the game keeps two, chosen by the
/// save's own team ([`TEAM_OFFSET`]) rather than by the player: a Team
/// ProtoMan save's is [`Cross::BassProto`] and a Team Colonel save's
/// [`Cross::BassColonel`]. [`Cross::bass_for`] is that rule, so a pick
/// lands the value the game would have written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Cross {
    /// No cross: plain MegaMan, which is every save that has not been
    /// through the prompt.
    None = 0,
    /// BassCross MegaMan on a Team ProtoMan save.
    BassProto = 1,
    /// BassCross MegaMan on a Team Colonel save.
    BassColonel = 2,
    /// SolCross MegaMan, from the Boktai cartridge (Boktai 2 in the US
    /// build, Boktai 3 in the JP one — one value either way).
    Sol = 3,
}

/// Which of the cartridge's two teams a save plays, at this offset in
/// the save image: 0 Team ProtoMan, 1 Team Colonel. The game reads it
/// through the accessor at US `0x02001d74` to pick which BassCross
/// value to write.
pub const TEAM_OFFSET: usize = 0x0b;

impl Cross {
    /// The byte, as the save stores it.
    pub fn raw(self) -> u8 {
        self as u8
    }

    /// A stored byte read back, or [`Cross::None`] for anything the
    /// game does not write.
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Cross::BassProto,
            2 => Cross::BassColonel,
            3 => Cross::Sol,
            _ => Cross::None,
        }
    }

    /// BassCross as it would be written on a save whose [`TEAM_OFFSET`]
    /// byte is `team` — the game's own choice between the two values.
    pub fn bass_for(team: u8) -> Self {
        if team == 0 {
            Cross::BassProto
        } else {
            Cross::BassColonel
        }
    }

    /// Whether this is a BassCross, either team's.
    pub fn is_bass(self) -> bool {
        matches!(self, Cross::BassProto | Cross::BassColonel)
    }
}

/// The game's own checksum (ARM9 0x02020c90): the sum of each u16
/// XORed with the byte count still remaining at that point. The
/// length key is why no plain range sum ever matched the stored
/// values.
fn checksum(buf: &[u8]) -> u16 {
    let mut remaining = buf.len() as u16;
    let mut sum = 0u16;
    for pair in buf.chunks_exact(2) {
        sum = sum.wrapping_add(u16::from_le_bytes([pair[0], pair[1]]) ^ remaining);
        remaining = remaining.wrapping_sub(2);
    }
    sum
}

/// The generation counter stamped in `block`'s footer.
fn generation(data: &[u8], block: usize) -> u32 {
    u32::from_le_bytes(
        data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4]
            .try_into()
            .unwrap(),
    )
}

/// Whether `block` carries the game's format tag — the one thing that
/// tells a written block from erased flash.
fn formatted(data: &[u8], block: usize) -> bool {
    &data[block * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()] == MAGIC
}

/// Recompute `block`'s checksums the way the game does: the interior
/// byte-sum first (it lives inside the image), then cs2 over the save
/// image that now contains it, then cs1 over the footer that now
/// contains cs2.
fn rebuild_block(data: &mut [u8], block: usize) {
    let base = block * BLOCK_SIZE;

    let image = &data[base..][..SAVE_IMAGE_SIZE];
    let interior = image.iter().map(|&v| v as u32).sum::<u32>().wrapping_sub(
        image[INTERIOR_CHECKSUM_OFFSET..][..4]
            .iter()
            .map(|&v| v as u32)
            .sum::<u32>(),
    );
    data[base + INTERIOR_CHECKSUM_OFFSET..][..4].copy_from_slice(&interior.to_le_bytes());

    let cs2 = checksum(&data[base..][..SAVE_IMAGE_SIZE]);
    data[base + CHECKSUM_OFFSET + 4..][..2].copy_from_slice(&cs2.to_le_bytes());
    data[base + CHECKSUM_OFFSET + 6..][..2].copy_from_slice(&0u16.wrapping_sub(cs2).to_le_bytes());

    let cs1 = checksum(&data[base + FOOTER_SUM_START..base + FOOTER_SUM_END]);
    data[base + CHECKSUM_OFFSET..][..2].copy_from_slice(&cs1.to_le_bytes());
    data[base + CHECKSUM_OFFSET + 2..][..2].copy_from_slice(&0u16.wrapping_sub(cs1).to_le_bytes());
}

/// Every save on one cartridge: the whole flash dump, and which block
/// holds the live copy of each in-game file. Handing out a [`Save`] per
/// file is what makes the two files independently readable and
/// editable — see the module docs.
#[derive(Clone)]
pub struct SaveSet {
    data: Vec<u8>,
    /// `(file-select slot, live block)` per file the cart holds, in
    /// slot order.
    files: Vec<(u8, usize)>,
}

impl SaveSet {
    /// Accept `data` if it is a dump of this game's flash: no longer
    /// than the chip, with the game's own format tag in at least one
    /// block.
    ///
    /// The game falls back to a generation's twin copy when one block
    /// is damaged, so a single intact tag is as much as it requires
    /// too.
    ///
    /// A dump of any length is accepted and canonicalized to [`SIZE`],
    /// which is the length the emulator mounts and hands back.
    ///
    /// Short of the chip it is padded with erased flash. Dumpers trim
    /// the erased tail, or size the chip by what the game touched, and
    /// either way the short file carries every block the game wrote;
    /// the padding lands where flash the cart never wrote would be, and
    /// reads back as unformatted blocks — which is exactly what the
    /// game's own mount makes of them.
    ///
    /// Past the chip it is cut off. A dump longer than 256 KiB is a
    /// container, not a bigger cartridge: DS save managers routinely
    /// write a fixed 512 KiB whatever the cart holds, and every byte of
    /// such a file past the chip is flash that does not exist. Taking
    /// the front of it is not a guess — it is what `CartRetail` does
    /// with the same file, copying `min(file, chip)` into an erased
    /// chip and dropping the rest, and what the GBA saves here have
    /// always done by slicing the range they need out of a dump.
    /// Length was never what identified a save anyway: that is the
    /// format tag below, and a file that does not carry it is refused
    /// at any size.
    ///
    /// So the save Tango reads, the SRAM it hands the console and the
    /// image the replay records are the same bytes whatever length the
    /// file arrived at.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut data = data.to_vec();
        data.resize(SIZE, ERASED);

        // A file's live block is its intact block with the highest
        // generation counter; the game default-formats every block on
        // first save (an unused file carries baked defaults, tag and
        // all), so tag presence can't tell what's in use — only real
        // saves move the counter. A generation's two mirrored copies
        // are identical on an intact cart, so the tie between them is
        // broken toward the first block just to be deterministic.
        let mut files: Vec<(u8, usize)> = Vec::new();
        for block in (0..SIZE / BLOCK_SIZE).filter(|&block| formatted(&data, block)) {
            let slot = data[block * BLOCK_SIZE + FILE_SLOT_OFFSET];
            match files.iter_mut().find(|(s, _)| *s == slot) {
                Some(file) if generation(&data, block) > generation(&data, file.1) => file.1 = block,
                Some(_) => {}
                None => files.push((slot, block)),
            }
        }
        if files.is_empty() {
            return Err(Error::InvalidGameName(data[MAGIC_OFFSET..][..MAGIC.len()].to_vec()));
        }
        files.sort_unstable();

        Ok(SaveSet { data, files })
    }

    /// The file-select slots this cart holds, in slot order. Both exist
    /// on any cart the game formatted; a damaged dump may carry one.
    pub fn slots(&self) -> Vec<u8> {
        self.files.iter().map(|&(slot, _)| slot).collect()
    }

    /// File `slot`'s save, or `None` if this cart has no such file.
    pub fn save(&self, slot: u8) -> Option<Save> {
        let &(_, block) = self.files.iter().find(|(s, _)| *s == slot)?;
        Some(Save {
            data: self.data.clone(),
            block,
            slot,
            slots: self.slots(),
        })
    }

    /// The file saved most recently — **the** save this cartridge
    /// carries, as far as everything above is concerned: what the
    /// editor opens on, what a session plays, and what the priming walk
    /// steers the game's own file select to. Which file that is lives
    /// in the cartridge's own bytes (the generation counters the game
    /// stamps), so the choice needs nothing riding beside them — see
    /// [`Save::make_current`], which is how the editor changes it.
    pub fn current(&self) -> Save {
        let &(slot, _) = self
            .files
            .iter()
            .max_by_key(|&&(_, block)| generation(&self.data, block))
            .expect("a parsed SaveSet holds at least one file");
        self.save(slot).expect("the slot came from this set")
    }
}

/// One in-game file, handed out by [`SaveSet`]. The whole dump rides
/// along because that is what gets written back to the cartridge, but
/// every read and write goes through this file's own block.
#[derive(Clone)]
pub struct Save {
    data: Vec<u8>,
    block: usize,
    slot: u8,
    slots: Vec<u8>,
}

impl Save {
    /// Which of the game's file-select slots this save is.
    pub fn slot(&self) -> u8 {
        self.slot
    }

    /// Every file-select slot the cart behind this save holds — what a
    /// frontend's file picker offers. Carried along so drawing the
    /// picker doesn't mean re-parsing the dump every frame.
    pub fn slots(&self) -> &[u8] {
        &self.slots
    }

    /// The block this file's views read from.
    fn active(&self) -> &[u8] {
        &self.data[self.block * BLOCK_SIZE..][..BLOCK_SIZE]
    }

    fn active_mut(&mut self) -> &mut [u8] {
        &mut self.data[self.block * BLOCK_SIZE..][..BLOCK_SIZE]
    }

    /// Which team this file plays (see [`TEAM_OFFSET`]).
    pub fn team(&self) -> u8 {
        self.active()[TEAM_OFFSET]
    }

    /// The GBA-slot cross this file brings (see [`Cross`]).
    pub fn cross(&self) -> Cross {
        Cross::from_raw(self.active()[CROSS_OFFSET])
    }

    /// Which navi this file is being played as (see [`NAVI_OFFSET`]).
    pub fn navi(&self) -> usize {
        self.active()[NAVI_OFFSET] as usize
    }

    /// The navi in team slot `slot`, or `None` for an empty slot (and
    /// for a slot past the two the panel has).
    fn team_navi(&self, slot: usize) -> Option<usize> {
        if slot >= NUM_TEAM_SLOTS {
            return None;
        }
        let raw = u32::from_le_bytes(
            self.active()[TEAM_NAVI_OFFSET + slot * std::mem::size_of::<u32>()..][..4]
                .try_into()
                .unwrap(),
        );
        raw.checked_sub(TEAM_NAVI_BIAS).map(|navi| navi as usize)
    }

    /// Put `navi` in team slot `slot`, or empty it with `None`.
    /// Refuses a slot the panel hasn't got and a navi outside the
    /// record array (0, MegaMan, included — he is who the panel changes
    /// *from*).
    ///
    /// Keeps the [`TEAM_MIRROR_OFFSET`] bits equal to the slots the way
    /// the game's own machine does, so the load's cross-check passes.
    /// Keeping the list packed is [`pack_team`](Save::pack_team)'s job.
    ///
    /// A slot that changes members loses its customizer loadout and the
    /// navi that left is stripped back to no bonus, which is what the
    /// game's own change does: the programs come off.
    fn set_team_navi(&mut self, slot: usize, navi: Option<usize>) -> bool {
        if slot >= NUM_TEAM_SLOTS {
            return false;
        }
        let raw = match navi {
            Some(navi) if (1..NUM_NAVIS).contains(&navi) => navi as u32 + TEAM_NAVI_BIAS,
            Some(_) => return false,
            None => 0,
        };
        let left = self.team_navi(slot).filter(|&left| Some(left) != navi);
        self.active_mut()[TEAM_NAVI_OFFSET + slot * std::mem::size_of::<u32>()..][..4]
            .copy_from_slice(&raw.to_le_bytes());
        self.sync_team_mirror();
        if let Some(left) = left {
            self.clear_partycust_loadout(slot);
            // Unless the navi only moved between the two slots, in
            // which case its loadout moves with it (see `pack_team`).
            if !(0..NUM_TEAM_SLOTS).any(|slot| self.team_navi(slot) == Some(left)) {
                self.write_partycust_bonus(left, PartycustBonus::default());
            }
        }
        true
    }

    /// Rewrite the twelve mirror bits to exactly the navis the slots
    /// hold, leaving the story flags around them alone.
    fn sync_team_mirror(&mut self) {
        let held: Vec<usize> = (0..NUM_TEAM_SLOTS).filter_map(|slot| self.team_navi(slot)).collect();
        for navi in 1..NUM_NAVIS {
            let (at, mask) = team_mirror_bit(navi);
            if held.contains(&navi) {
                self.active_mut()[at] |= mask;
            } else {
                self.active_mut()[at] &= !mask;
            }
        }
    }

    /// Close any gap in the team, so the slots read as the game keeps
    /// them: filled from the first, empties at the end.
    ///
    /// The team is a packed list, not two independent slots. Watching
    /// the game's own Navi Change machine settles it: clearing the
    /// first of two navis moves the second down into it, and a save
    /// whose id sits in the second slot with the first empty is one the
    /// battle refuses — it draws MegaMan and NO DATA instead of the
    /// team. Packing that same save's ids is enough to make the battle
    /// show the navi again, which is why every edit ends here.
    /// A navi's customizer loadout moves down with it, since it belongs
    /// to the member rather than to the slot.
    fn pack_team(&mut self) {
        let held: Vec<(usize, [u8; PARTYCUST_LOADOUT_SIZE])> = (0..NUM_TEAM_SLOTS)
            .filter_map(|slot| Some((self.team_navi(slot)?, self.partycust_loadout(slot))))
            .collect();
        for slot in 0..NUM_TEAM_SLOTS {
            self.set_team_navi(slot, held.get(slot).map(|&(navi, _)| navi));
            let entry = held.get(slot).map(|&(_, entry)| entry).unwrap_or_default();
            self.partycust_loadout_mut(slot).copy_from_slice(&entry);
        }
    }

    /// Navi `id`'s effective max HP, off its record — the number the
    /// game's own Navi Change screens put on the navi's card.
    pub fn navi_hp(&self, id: usize) -> u16 {
        self.navi_stats(id).map(|[_, _, effective]| effective).unwrap_or(0)
    }

    /// Set navi `id`'s HP: base and current take `hp`, and the
    /// effective figure re-folds whatever the PARTY CUSTOMIZER adds,
    /// the way [`write_partycust_bonus`](Save::write_partycust_bonus)
    /// keeps it. Refuses an id past the roster.
    pub fn set_navi_hp(&mut self, id: usize, hp: u16) -> bool {
        if id >= NUM_NAVIS {
            return false;
        }
        let effective = hp.saturating_add(self.partycust_bonus(id).max_hp);
        let stats = NAVI_RECORD_OFFSET + id * NAVI_RECORD_SIZE + NAVI_STATS_INTO_RECORD;
        for (at, value) in [(0, hp), (2, hp), (4, effective)] {
            self.active_mut()[stats + at..][..2].copy_from_slice(&value.to_le_bytes());
        }
        true
    }

    /// MegaMan's karma — see [`KARMA_INTO_RECORD`].
    pub fn karma(&self) -> u16 {
        u16::from_le_bytes(
            self.active()[NAVI_RECORD_OFFSET + KARMA_INTO_RECORD..][..2]
                .try_into()
                .unwrap(),
        )
    }

    /// Set MegaMan's karma, clamped the way the game keeps it, and
    /// bring the anti-tamper mirror along — see [`KARMA_MIRROR_OFFSET`].
    pub fn set_karma(&mut self, karma: u16) {
        let karma = karma.min(KARMA_MAX);
        let key = u32::from_le_bytes(self.active()[KARMA_KEY_OFFSET..][..4].try_into().unwrap());
        self.active_mut()[NAVI_RECORD_OFFSET + KARMA_INTO_RECORD..][..2].copy_from_slice(&karma.to_le_bytes());
        self.active_mut()[KARMA_MIRROR_OFFSET..][..4].copy_from_slice(&(karma as u32 ^ key).to_le_bytes());
    }

    /// How much max HP Dark Chip use has cost this file — see
    /// [`DARK_HP_LOSSES_OFFSET`]. The HP itself is the records'
    /// business; this is only the counter the game stops docking at.
    pub fn dark_hp_losses(&self) -> u16 {
        u16::from_le_bytes(self.active()[DARK_HP_LOSSES_OFFSET..][..2].try_into().unwrap())
    }

    /// Set the Dark Chip HP-loss counter.
    pub fn set_dark_hp_losses(&mut self, losses: u16) {
        self.active_mut()[DARK_HP_LOSSES_OFFSET..][..2].copy_from_slice(&losses.to_le_bytes());
    }

    /// What the PARTY CUSTOMIZER has given navi `id`. All zeroes for a
    /// navi nobody has customized, which is every navi on a fresh file.
    pub fn partycust_bonus(&self, id: usize) -> PartycustBonus {
        let byte = |into| {
            self.active()
                .get(NAVI_RECORD_OFFSET + id * NAVI_RECORD_SIZE + into)
                .copied()
                .unwrap_or(0)
        };
        let half = |into: usize| {
            self.active()
                .get(NAVI_RECORD_OFFSET + id * NAVI_RECORD_SIZE + into..)
                .and_then(|raw| raw.get(..2))
                .map(|raw| u16::from_le_bytes(raw.try_into().unwrap()))
                .unwrap_or(0)
        };
        PartycustBonus {
            max_hp: half(PARTYCUST_MAX_HP_INTO_RECORD),
            attack: byte(PARTYCUST_ATTACK_INTO_RECORD),
            chip_attack: half(PARTYCUST_CHIP_ATTACK_INTO_RECORD),
            support: byte(PARTYCUST_SUPPORT_INTO_RECORD) != 0,
        }
    }

    /// Dress party slot `slot` in `programs`, the way a session that
    /// ended on the panel's `RUN!` leaves it: the member's record takes
    /// the sum of what they grant, and the slot's own entry takes the
    /// programs themselves. An empty set strips the member bare.
    ///
    /// Refuses a slot with no member, and a set longer than an entry
    /// holds. Whether it fits the member's gauge is
    /// [`PartyView::can_add_party_program`]'s to say, and what the
    /// editor asks before calling.
    fn set_party_programs(
        &mut self,
        slot: usize,
        programs: impl IntoIterator<Item = usize>,
        assets: &crate::rom::Assets,
    ) -> bool {
        let Some(navi) = self.team_navi(slot) else { return false };
        let mut bonus = PartycustBonus::default();
        let mut items = [0u8; MAX_PARTY_PROGRAMS_EQUIPPED];
        for (at, index) in programs.into_iter().enumerate() {
            let Some(program) = assets.party_program(index) else { return false };
            let Some(equipped) = bonus.plus(program.bonus()) else { return false };
            let Some(item) = items.get_mut(at) else { return false };
            *item = program.item_id() as u8;
            bonus = equipped;
        }

        self.write_partycust_bonus(navi, bonus);
        let entry = self.partycust_loadout_mut(slot);
        entry.fill(0);
        entry[PARTYCUST_LOADOUT_NAVI] = navi as u8;
        entry[PARTYCUST_LOADOUT_PROGRAMS..PARTYCUST_LOADOUT_ATTACK].copy_from_slice(&items);
        entry[PARTYCUST_LOADOUT_ATTACK] = bonus.attack;
        entry[PARTYCUST_LOADOUT_CHIP_ATTACK] = bonus.chip_attack.min(u8::MAX as u16) as u8;
        true
    }

    /// Write navi `id`'s four record fields, and fold the HP into the
    /// effective max HP the rest of the game reads.
    fn write_partycust_bonus(&mut self, id: usize, bonus: PartycustBonus) {
        if id >= NUM_NAVIS {
            return;
        }
        let base = self.navi_stats(id).map(|[base, _, _]| base).unwrap_or(0);
        let record = NAVI_RECORD_OFFSET + id * NAVI_RECORD_SIZE;
        self.active_mut()[record + PARTYCUST_ATTACK_INTO_RECORD] = bonus.attack;
        self.active_mut()[record + PARTYCUST_SUPPORT_INTO_RECORD] = bonus.support as u8;
        self.active_mut()[record + PARTYCUST_CHIP_ATTACK_INTO_RECORD..][..2]
            .copy_from_slice(&bonus.chip_attack.to_le_bytes());
        self.active_mut()[record + PARTYCUST_MAX_HP_INTO_RECORD..][..2]
            .copy_from_slice(&bonus.max_hp.to_le_bytes());
        self.active_mut()[record + PARTYCUST_EFFECTIVE_HP_INTO_RECORD..][..2]
            .copy_from_slice(&base.saturating_add(bonus.max_hp).to_le_bytes());
    }

    /// Party slot `slot`'s customizer entry (see
    /// [`PARTYCUST_LOADOUT_OFFSET`]), all zeroes past the two slots.
    fn partycust_loadout(&self, slot: usize) -> [u8; PARTYCUST_LOADOUT_SIZE] {
        if slot >= NUM_TEAM_SLOTS {
            return [0; PARTYCUST_LOADOUT_SIZE];
        }
        self.active()[PARTYCUST_LOADOUT_OFFSET + slot * PARTYCUST_LOADOUT_SIZE..][..PARTYCUST_LOADOUT_SIZE]
            .try_into()
            .unwrap()
    }

    fn partycust_loadout_mut(&mut self, slot: usize) -> &mut [u8] {
        let slot = slot.min(NUM_TEAM_SLOTS - 1);
        &mut self.active_mut()[PARTYCUST_LOADOUT_OFFSET + slot * PARTYCUST_LOADOUT_SIZE..][..PARTYCUST_LOADOUT_SIZE]
    }

    /// Empty party slot `slot`'s customizer entry — what the game does
    /// when the member changes.
    fn clear_partycust_loadout(&mut self, slot: usize) {
        if slot < NUM_TEAM_SLOTS {
            self.partycust_loadout_mut(slot).fill(0);
        }
    }

    /// The party this file brings: who each slot fields and what the
    /// customizer has put on them.
    pub fn view_party(&self) -> PartyView<&Save> {
        PartyView { save: self }
    }

    /// The same, with the panel's own edits on it.
    pub fn view_party_mut(&mut self) -> PartyView<&mut Save> {
        PartyView { save: self }
    }

    /// How far through the story this file is, 0..=9 — see
    /// [`STORY_RANK_FLAG_BASE`].
    pub fn story_rank(&self) -> u8 {
        (0..STORY_RANKS)
            .take_while(|&n| {
                let (at, mask) = story_flag(STORY_RANK_FLAG_BASE + n);
                self.active().get(at).is_some_and(|byte| byte & mask != 0)
            })
            .count() as u8
    }

    /// Navi `id`'s chip attack, the way its PARTY STATUS card reads it:
    /// what the navi brings at this file's [`story_rank`](Save::story_rank),
    /// plus whatever `P.Chp` programs the customizer has added.
    pub fn navi_chip_attack(&self, id: usize, assets: &crate::rom::Assets) -> u16 {
        assets
            .navi_chip_attack(id, self.story_rank())
            .saturating_add(self.partycust_bonus(id).chip_attack)
    }

    /// How many of item `id` the file stocks — see
    /// [`ITEM_COUNTS_OFFSET`].
    pub fn item_count(&self, id: usize) -> u8 {
        if id >= NUM_ITEMS {
            return 0;
        }
        self.active().get(ITEM_COUNTS_OFFSET + id).copied().unwrap_or(0)
    }

    /// Navi `id`'s stats block: base max HP, current HP, effective max
    /// HP, as three u16 LE. `None` past the roster.
    fn navi_stats(&self, id: usize) -> Option<[u16; 3]> {
        let raw = self
            .active()
            .get(NAVI_RECORD_OFFSET + id * NAVI_RECORD_SIZE + NAVI_STATS_INTO_RECORD..)?
            .get(..3 * std::mem::size_of::<u16>())?;
        Some(std::array::from_fn(|i| {
            u16::from_le_bytes(raw[i * 2..][..2].try_into().unwrap())
        }))
    }

    /// Set the cross this file brings, writing the game's own byte.
    /// Checksums are not rebuilt here — the editor commits through
    /// [`rebuild_checksum`](tango_gamesupport_common_dataview::save::Save::rebuild_checksum)
    /// like every other edit.
    pub fn set_cross(&mut self, cross: Cross) {
        self.active_mut()[CROSS_OFFSET] = cross.raw();
    }

    /// Make this file the cartridge's most recently saved one, so
    /// [`SaveSet::current`] hands it out — which is how the pick
    /// reaches a session, a recording and the game's own file select
    /// without anything riding beside the bytes.
    ///
    /// The game alternates a file between two mirrored pairs of blocks
    /// and calls the highest counter current, so being current is a
    /// matter of degree rather than a flag: this stamps the file's live
    /// pair one past every counter on the cartridge. Both blocks of the
    /// pair, because the game reads whichever of them it reaches first
    /// — the same reason `rebuild_checksum` mirrors.
    ///
    /// A no-op when this file already leads.
    pub fn make_current(&mut self) {
        let blocks = self.data.len() / BLOCK_SIZE;
        let highest = (0..blocks)
            .filter(|&b| formatted(&self.data, b))
            .map(|b| generation(&self.data, b))
            .max()
            .unwrap_or(0);
        if generation(&self.data, self.block) == highest {
            return;
        }
        let next = highest.wrapping_add(1);
        for block in [self.block, self.block ^ 1] {
            let base = block * BLOCK_SIZE + GENERATION_OFFSET;
            self.data[base..][..4].copy_from_slice(&next.to_le_bytes());
        }
    }
}

impl tango_gamesupport_common_dataview::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    /// The NaviCust, unless the file is being played as a team navi —
    /// the customizer is MegaMan's, exactly as it is on GBA (see
    /// [`NAVI_OFFSET`] for what a nonzero navi means on this cart).
    fn view_navicust(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NavicustView + '_>> {
        if self.navi() != 0 {
            return None;
        }
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NavicustViewMut + '_>> {
        if self.navi() != 0 {
            return None;
        }
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_auto_battle_data(
        &self,
    ) -> Option<Box<dyn tango_gamesupport_common_dataview::save::AutoBattleDataView + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn view_auto_battle_data_mut(
        &mut self,
    ) -> Option<Box<dyn tango_gamesupport_common_dataview::save::AutoBattleDataViewMut + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// The whole dump doubles as the "wram": the cart's ROM assets are
    /// read from the cart image, not derived from a save — the trait
    /// just wants the save's raw bytes.
    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.data)
    }

    /// Recompute this file's block checksum pairs, and bring the
    /// generation's twin copy along so whichever of the mirrored pair
    /// the game reads first loads.
    fn rebuild_checksum(&mut self) {
        rebuild_block(&mut self.data, self.block);
        // Blocks pair up as (0,1), (2,3), (4,5); the twin is the other
        // half of this block's pair.
        let base = self.block * BLOCK_SIZE;
        let twin = (self.block ^ 1) * BLOCK_SIZE;
        self.data.copy_within(base..base + BLOCK_SIZE, twin);
    }

}

pub struct ChipsView<S> {
    save: S,
}

/// One folder chip slot, as the save stores it — the GBA game's packed
/// id/code halfword unchanged.
#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    #[bitfield(name = "id", ty = "u16", bits = "0..=8")]
    #[bitfield(name = "code", ty = "u16", bits = "9..=15")]
    id_and_code: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::ChipsView for ChipsView<S> {
    fn num_folders(&self) -> usize {
        NUM_FOLDERS
    }

    fn equipped_folder_index(&self) -> usize {
        let index = self.save.active()[EQUIPPED_FOLDER_OFFSET] as usize;
        // A value past the folder count would be a misread of the
        // save, not a folder; show the first folder rather than an
        // empty grid.
        if index >= NUM_FOLDERS {
            return 0;
        }
        index
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        let index = self.save.active()[REGULAR_CHIP_OFFSET + folder_index];
        Some(if index >= 30 { None } else { Some(index as usize) })
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<tango_gamesupport_common_dataview::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= self.folder_size() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.active()[FOLDER_OFFSET
                + (folder_index * self.folder_size() + chip_index) * std::mem::size_of::<RawChip>()..]
                [..std::mem::size_of::<RawChip>()],
        );

        Some(tango_gamesupport_common_dataview::save::Chip {
            id: raw.id() as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code())?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= NUM_PACK_CHIPS || variant >= 4 {
            return None;
        }
        Some(self.save.active()[PACK_OFFSET + id * PACK_ENTRY_SIZE + variant] as usize)
    }
}

impl<S: std::ops::Deref<Target = Save>> ChipsView<S> {
    /// The lowest acquisition key in the pack, or [`PACK_KEY_FIRST`] + 1
    /// when nothing is owned yet, so the first grant lands on
    /// [`PACK_KEY_FIRST`] itself.
    fn lowest_pack_key(&self) -> u16 {
        let block = self.save.active();
        (0..NUM_PACK_CHIPS)
            .flat_map(|id| {
                let entry = PACK_OFFSET + id * PACK_ENTRY_SIZE;
                (0..4).map(move |variant| entry + 4 + variant * 2)
            })
            .map(|off| u16::from_le_bytes(block[off..][..2].try_into().unwrap()))
            .filter(|&key| key != 0)
            .min()
            .unwrap_or(PACK_KEY_FIRST + 1)
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        if folder_index >= NUM_FOLDERS {
            return false;
        }
        self.save.active_mut()[EQUIPPED_FOLDER_OFFSET] = folder_index as u8;
        true
    }

    fn set_chip(
        &mut self,
        folder_index: usize,
        chip_index: usize,
        chip: tango_gamesupport_common_dataview::save::Chip,
    ) -> bool {
        if folder_index >= NUM_FOLDERS || chip_index >= 30 || chip.id > 0x1ff {
            return false;
        }
        self.save.active_mut()
            [FOLDER_OFFSET + (folder_index * 30 + chip_index) * std::mem::size_of::<RawChip>()..]
            [..std::mem::size_of::<RawChip>()]
            .copy_from_slice(bytemuck::bytes_of(&{
                let mut raw = RawChip::default();
                raw.set_id(chip.id as u16);
                raw.set_code(chip.code as u16);
                raw
            }));
        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= NUM_FOLDERS || chip_index >= 30 {
            return false;
        }
        // 0xffff reads back as an invalid code, so `chip()` returns
        // None — i.e. an empty slot.
        self.save.active_mut()
            [FOLDER_OFFSET + (folder_index * 30 + chip_index) * std::mem::size_of::<RawChip>()..]
            [..std::mem::size_of::<RawChip>()]
            .fill(0xff);
        true
    }

    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: Option<usize>) -> bool {
        if folder_index >= NUM_FOLDERS {
            return false;
        }
        // 0xff (out of the 0..30 range) reads back as "no regular".
        let raw = match chip_index {
            Some(i) if i < 30 => i as u8,
            None => 0xff,
            Some(_) => return false,
        };
        self.save.active_mut()[REGULAR_CHIP_OFFSET + folder_index] = raw;
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= NUM_PACK_CHIPS || variant >= 4 || count > 0xff {
            return false;
        }
        let entry = PACK_OFFSET + id * PACK_ENTRY_SIZE;
        self.save.active_mut()[entry + variant] = count as u8;

        // Keep the slot's acquisition key in the shape the game keeps
        // it: zero while never owned, and — for a code being granted
        // for the first time — the next key below every one already
        // handed out, which is how the game's own descending counter
        // would have issued it.
        let key_offset = entry + 4 + variant * 2;
        let key = u16::from_le_bytes(self.save.active()[key_offset..][..2].try_into().unwrap());
        let new_key = if count == 0 {
            0
        } else if key == 0 {
            self.lowest_pack_key().saturating_sub(1)
        } else {
            key
        };
        self.save.active_mut()[key_offset..][..2].copy_from_slice(&new_key.to_le_bytes());
        true
    }

    /// Nothing to rebuild: no GBA-style anti-cheat mirror is known on
    /// the DS cart — the load path verifies the two checksum pairs and
    /// nothing else that has been mapped.
    fn rebuild_anticheat(&mut self) {}
}

pub struct NavicustView<S> {
    save: S,
}

/// One NaviCust part slot, as the save stores it — the GBA game's entry
/// unchanged.
#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawNavicustPart {
    id: u8,
    _unk_01: u8,
    col: u8,
    row: u8,
    rot: u8,
    compressed: u8,
    _unk_06: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == NAVICUST_PART_SIZE);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NavicustView for NavicustView<S> {
    fn count(&self) -> usize {
        NUM_NAVICUST_SLOTS
    }

    fn size(&self) -> [usize; 2] {
        [NAVICUST_SIZE, NAVICUST_SIZE]
    }

    fn navicust_part(&self, i: usize) -> Option<tango_gamesupport_common_dataview::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }
        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.active()[NAVICUST_PARTS_OFFSET + i * NAVICUST_PART_SIZE..][..NAVICUST_PART_SIZE],
        );
        if raw.id == 0 {
            return None;
        }
        Some(tango_gamesupport_common_dataview::save::NavicustPart {
            id: raw.id as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: raw.compressed != 0,
        })
    }

    fn materialized(&self) -> tango_gamesupport_common_dataview::navicust::MaterializedNavicust {
        tango_gamesupport_common_dataview::navicust::materialized_from_wram(
            &self.save.active()[NAVICUST_GRID_OFFSET..][..NAVICUST_SIZE * NAVICUST_SIZE],
            [NAVICUST_SIZE, NAVICUST_SIZE],
        )
    }

    fn navicust_color_bar(&self) -> Vec<Option<tango_gamesupport_common_dataview::rom::NavicustPartColor>> {
        self.save.active()[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN]
            .iter()
            .map(|&raw| crate::rom::navicust_part_color(raw))
            .collect()
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::NavicustViewMut
    for NavicustView<S>
{
    fn set_navicust_part(
        &mut self,
        i: usize,
        part: Option<tango_gamesupport_common_dataview::save::NavicustPart>,
    ) -> bool {
        if i >= NUM_NAVICUST_SLOTS {
            return false;
        }
        let raw = match part {
            Some(part) => {
                if part.id >= crate::NUM_NAVICUST_PARTS {
                    return false;
                }
                RawNavicustPart {
                    id: part.id as u8,
                    col: part.col,
                    row: part.row,
                    rot: part.rot,
                    compressed: u8::from(part.compressed),
                    ..Default::default()
                }
            }
            // An all-zero part (id 0) reads back as an empty slot.
            None => RawNavicustPart::default(),
        };
        self.save.active_mut()[NAVICUST_PARTS_OFFSET + i * NAVICUST_PART_SIZE..][..NAVICUST_PART_SIZE]
            .copy_from_slice(bytemuck::bytes_of(&raw));
        true
    }

    fn clear_materialized(&mut self) {
        self.save.active_mut()[NAVICUST_GRID_OFFSET..][..NAVICUST_GRID_SECTION_SIZE].fill(0);
        self.save.active_mut()[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN].fill(0);
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common_dataview::rom::Assets) {
        let materialized = tango_gamesupport_common_dataview::navicust::materialize(
            &*self,
            [NAVICUST_SIZE, NAVICUST_SIZE],
            assets,
        );
        let mut grid = [0u8; NAVICUST_GRID_SECTION_SIZE];
        for (cell, slot) in grid.iter_mut().zip(materialized) {
            // Cells hold the slot + 1, so 0 stays "empty".
            *cell = slot.map(|slot| slot as u8 + 1).unwrap_or(0);
        }
        self.save.active_mut()[NAVICUST_GRID_OFFSET..][..NAVICUST_GRID_SECTION_SIZE].copy_from_slice(&grid);

        // The colour bar: the distinct part colours in placement order.
        let bar = tango_gamesupport_common_dataview::navicust::materialize_color_bar(&*self, assets);
        let mut bytes = [0u8; NAVICUST_COLOR_BAR_LEN];
        for (slot, color) in bar.iter().flatten().enumerate().take(NAVICUST_COLOR_BAR_LEN) {
            bytes[slot] = tango_gamesupport_common_dataview::navicust::color_to_raw(
                color,
                crate::rom::navicust_part_color,
            );
        }
        self.save.active_mut()[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN].copy_from_slice(&bytes);
    }
}

/// How wide and tall the grid is.
const NAVICUST_SIZE: usize = 5;

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NaviView for NaviView<S> {
    fn navi(&self) -> usize {
        self.save.navi()
    }

    /// The HP the navi brings. MegaMan's own is what HP Memories bought
    /// plus what his NaviCust adds — the same sum the GBA game makes,
    /// minus the patch cards this cart has none of. A team navi reports
    /// the effective figure its own record carries.
    fn max_hp(&self, _assets: &dyn tango_gamesupport_common_dataview::rom::Assets) -> u16 {
        let navi = self.navi();
        let Some([base, _current, effective]) = self.save.navi_stats(navi) else {
            return 0;
        };
        if navi != 0 {
            return effective;
        }

        let mut max_hp = base;
        if let Some(navicust) = tango_gamesupport_common_dataview::save::Save::view_navicust(&*self.save) {
            for part in placed_parts(&*navicust) {
                for effect in crate::rom::navicust::navicust_part_effects(part) {
                    if let crate::rom::navicust::NavicustEffect::MaxHp(n) = effect {
                        max_hp += *n;
                    }
                }
            }
        }
        max_hp
    }

    /// What a folder may hold. The class caps come off the NaviCust's
    /// command line the way the GBA game reads them, Regular memory off
    /// the folder cluster, and the Dark cap and per-chip copy rule are
    /// the game's own constants — with no patch cards on this cart to
    /// move any of them.
    fn folder_limits(
        &self,
        _assets: &dyn tango_gamesupport_common_dataview::rom::Assets,
    ) -> tango_gamesupport_common_dataview::save::FolderLimits {
        let mut mega: isize = BASE_MEGA_LIMIT;
        let mut giga: usize = BASE_GIGA_LIMIT;

        if let Some(navicust) = tango_gamesupport_common_dataview::save::Save::view_navicust(&*self.save) {
            for part in command_line_parts(&*navicust) {
                for effect in crate::rom::navicust::navicust_part_effects(part) {
                    match effect {
                        crate::rom::navicust::NavicustEffect::MegaLimit(n) => mega += *n as isize,
                        crate::rom::navicust::NavicustEffect::GigaLimit(n) => giga += *n as usize,
                        _ => {}
                    }
                }
            }
        }

        tango_gamesupport_common_dataview::save::FolderLimits {
            mega_limit: Some(mega.clamp(0, MAX_CLASS_LIMIT as isize) as usize),
            giga_limit: Some(giga.clamp(0, MAX_CLASS_LIMIT)),
            dark_limit: Some(DARK_LIMIT),
            reg_memory: Some(self.save.active()[REGULAR_MEMORY_OFFSET]),
            max_copies: |chip| {
                if chip.dark() {
                    return 1;
                }
                match chip.class() {
                    tango_gamesupport_common_dataview::rom::ChipClass::Mega
                    | tango_gamesupport_common_dataview::rom::ChipClass::Giga => 1,
                    tango_gamesupport_common_dataview::rom::ChipClass::Standard => 4,
                    _ => 0,
                }
            },
            ..Default::default()
        }
    }
}

/// What a folder may hold before the NaviCust moves it, and the ceiling
/// either class cap is clamped to — the GBA game's numbers, which the
/// cart keeps.
const BASE_MEGA_LIMIT: isize = 5;
const BASE_GIGA_LIMIT: usize = 1;
const MAX_CLASS_LIMIT: usize = 10;
const DARK_LIMIT: usize = 3;

/// The part ids actually on the grid, each counted once however many
/// cells it covers.
fn placed_parts(navicust: &dyn tango_gamesupport_common_dataview::save::NavicustView) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    navicust
        .materialized()
        .into_iter()
        .flatten()
        .filter(|slot| seen.insert(*slot))
        .filter_map(|slot| navicust.navicust_part(slot).map(|part| part.id))
        .collect()
}

/// The same, restricted to the command line — the row a program has to
/// sit on for its folder effect to count.
fn command_line_parts(navicust: &dyn tango_gamesupport_common_dataview::save::NavicustView) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    navicust
        .materialized()
        .row(NAVICUST_COMMAND_LINE)
        .iter()
        .flatten()
        .copied()
        .filter(|slot| seen.insert(*slot))
        .filter_map(|slot| navicust.navicust_part(slot).map(|part| part.id))
        .collect()
}

/// Which row of the grid is the command line. The cart's own layout
/// (see [`crate::rom`]) says the same; this is the save layer's copy so
/// the folder limits don't have to be handed the cart to read it.
const NAVICUST_COMMAND_LINE: usize = 2;

pub struct AutoBattleDataView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> AutoBattleDataView<S> {
    fn count_at(&self, base: usize, id: usize) -> Option<usize> {
        if id >= crate::NUM_AUTO_BATTLE_DATA_CHIPS {
            return None;
        }
        let raw = self.save.active().get(base + id * std::mem::size_of::<u16>()..)?;
        Some(u16::from_le_bytes(raw.get(..2)?.try_into().unwrap()) as usize)
    }
}

impl<S: std::ops::DerefMut<Target = Save>> AutoBattleDataView<S> {
    fn set_count_at(&mut self, base: usize, id: usize, count: usize) -> bool {
        if id >= crate::NUM_AUTO_BATTLE_DATA_CHIPS || count > u16::MAX as usize {
            return false;
        }
        self.save.active_mut()[base + id * std::mem::size_of::<u16>()..][..2]
            .copy_from_slice(&(count as u16).to_le_bytes());
        true
    }

    /// Write the deck the shared materializer built, leaving the combo
    /// slots as the game left them (see
    /// [`AUTO_BATTLE_DATA_COMBO_SLOTS`]).
    fn set_materialized(
        &mut self,
        materialized: &tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData,
    ) {
        for (slot, chip) in materialized.as_slice().iter().enumerate() {
            if AUTO_BATTLE_DATA_COMBO_SLOTS.contains(&slot) {
                continue;
            }
            let raw = chip.map(|chip| chip as u16).unwrap_or(0xffff);
            self.save.active_mut()[AUTO_BATTLE_DATA_OFFSET + slot * std::mem::size_of::<u16>()..][..2]
                .copy_from_slice(&raw.to_le_bytes());
        }
    }
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::AutoBattleDataView
    for AutoBattleDataView<S>
{
    fn chip_use_count(&self, id: usize) -> Option<usize> {
        self.count_at(CHIP_USE_COUNT_OFFSET, id)
    }

    fn secondary_chip_use_count(&self, id: usize) -> Option<usize> {
        self.count_at(SECONDARY_CHIP_USE_COUNT_OFFSET, id)
    }

    fn materialized(&self) -> tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData {
        tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData::from_wram(
            &self.save.active()[AUTO_BATTLE_DATA_OFFSET..]
                [..NUM_AUTO_BATTLE_DATA_SLOTS * std::mem::size_of::<u16>()],
        )
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::AutoBattleDataViewMut
    for AutoBattleDataView<S>
{
    fn set_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        self.set_count_at(CHIP_USE_COUNT_OFFSET, id, count)
    }

    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        self.set_count_at(SECONDARY_CHIP_USE_COUNT_OFFSET, id, count)
    }

    fn clear_materialized(&mut self) {
        self.set_materialized(
            &tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData::empty(),
        );
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common_dataview::rom::Assets) {
        let materialized =
            tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData::materialize(
                &*self, assets,
            );
        self.set_materialized(&materialized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_gamesupport_common_dataview::save::{ChipCode, ChipsViewMut, NavicustPart, Save as _};

    fn chip_bytes(id: u16, code: u16) -> [u8; 2] {
        (id | (code << 9)).to_le_bytes()
    }

    fn chips_mut(save: &mut Save) -> Box<dyn ChipsViewMut + '_> {
        save.view_chips_mut().expect("the folder is editable")
    }

    fn plausible() -> Vec<u8> {
        let mut data = vec![0u8; SIZE];
        for block in 0..SIZE / BLOCK_SIZE {
            data[block * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()].copy_from_slice(MAGIC);
        }
        data
    }

    #[test]
    fn accepts_a_dump_of_this_game() {
        assert!(SaveSet::parse(&plausible()).is_ok());
    }

    #[test]
    fn accepts_a_dump_with_a_single_intact_block() {
        let mut data = vec![0u8; SIZE];
        data[5 * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()].copy_from_slice(MAGIC);
        assert!(SaveSet::parse(&data).is_ok());
    }

    /// A dump whose erased tail was trimmed off is padded back out, and
    /// the blocks that fell in the trim read as the unformatted flash
    /// they are.
    #[test]
    fn accepts_a_dump_short_of_the_flash() {
        use tango_gamesupport_common_dataview::save::Save as _;
        let mut full = vec![ERASED; SIZE];
        for block in 0..2 {
            full[block * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()].copy_from_slice(MAGIC);
            full[block * BLOCK_SIZE + FILE_SLOT_OFFSET] = 0;
        }
        // Everything past the two written blocks is erased flash, so
        // cutting it off loses nothing.
        let set = SaveSet::parse(&full[..2 * BLOCK_SIZE]).unwrap();
        assert_eq!(set.slots(), vec![0]);
        assert_eq!(set.current().to_sram_dump(), full);
    }

    /// The shape a DS save manager writes: the cart's flash, then as
    /// much erased padding again. The cart is in there; the rest is
    /// address space the chip does not answer for.
    #[test]
    fn accepts_a_dump_longer_than_the_flash() {
        use tango_gamesupport_common_dataview::save::Save as _;
        let full = plausible();
        let mut padded = full.clone();
        padded.resize(SIZE * 2, ERASED);
        let set = SaveSet::parse(&padded).unwrap();
        assert_eq!(set.slots(), SaveSet::parse(&full).unwrap().slots());
        assert_eq!(set.current().to_sram_dump(), full);
    }

    #[test]
    fn rejects_another_games_dump_of_the_same_size() {
        assert!(SaveSet::parse(&vec![0u8; SIZE]).is_err());
    }

    #[test]
    fn round_trips_its_bytes() {
        let data = plausible();
        let save = SaveSet::parse(&data).unwrap().current();
        assert_eq!(save.to_sram_dump(), data);
    }

    #[test]
    fn reads_the_folder() {
        let mut data = plausible();
        data[FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(193, 12));
        // Last slot of the last folder.
        data[FOLDER_OFFSET + (2 * 30 + 29) * 2..][..2].copy_from_slice(&chip_bytes(207, 26));

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(
            chips.chip(0, 0),
            Some(tango_gamesupport_common_dataview::save::Chip {
                id: 193,
                code: ChipCode::M,
            })
        );
        assert_eq!(
            chips.chip(2, 29),
            Some(tango_gamesupport_common_dataview::save::Chip {
                id: 207,
                code: ChipCode::Star,
            })
        );
        assert_eq!(chips.chip(3, 0), None);
        assert_eq!(chips.chip(0, 30), None);
    }

    #[test]
    fn reads_the_newest_generation() {
        let mut data = plausible();
        // Blocks 0/1 hold generation 5, blocks 2/3 generation 6: the
        // game alternates pairs, so the higher counter is current.
        data[0 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&5u32.to_le_bytes());
        data[1 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&5u32.to_le_bytes());
        data[2 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&6u32.to_le_bytes());
        data[3 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&6u32.to_le_bytes());
        data[0 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(1, 0));
        data[2 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(2, 0));

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(chips.chip(0, 0).unwrap().id, 2);
    }

    #[test]
    fn skips_a_damaged_block_no_matter_its_counter() {
        let mut data = plausible();
        // The highest counter sits in a block whose tag is gone; its
        // intact twin (same generation) is what should be read.
        data[2 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&9u32.to_le_bytes());
        data[2 * BLOCK_SIZE + MAGIC_OFFSET] = 0;
        data[3 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&9u32.to_le_bytes());
        data[3 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(7, 3));

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(chips.chip(0, 0).unwrap().id, 7);
    }

    #[test]
    fn reads_the_equipped_folder_and_regular_chips() {
        let mut data = plausible();
        data[EQUIPPED_FOLDER_OFFSET] = 1;
        data[REGULAR_CHIP_OFFSET] = 0xff;
        data[REGULAR_CHIP_OFFSET + 1] = 7;

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(chips.equipped_folder_index(), 1);
        assert_eq!(chips.regular_chip_index(0), Some(None));
        assert_eq!(chips.regular_chip_index(1), Some(Some(7)));
    }

    #[test]
    fn an_equipped_index_past_the_folders_reads_as_the_first() {
        let mut data = plausible();
        data[EQUIPPED_FOLDER_OFFSET] = 9;

        let save = SaveSet::parse(&data).unwrap().current();
        assert_eq!(save.view_chips().unwrap().equipped_folder_index(), 0);
    }

    #[test]
    fn checksum_matches_the_games_algorithm() {
        // By hand: (0x1234 ^ 4) + (0xabcd ^ 2).
        assert_eq!(checksum(&[0x34, 0x12, 0xcd, 0xab]), 0x1230u16.wrapping_add(0xabcf));
    }

    /// Put `part` in slot `slot` of the grid's `cells`, so the views
    /// that read the materialized grid see it placed.
    fn place(data: &mut [u8], slot: usize, id: u8, cells: &[usize]) {
        data[NAVICUST_PARTS_OFFSET + slot * NAVICUST_PART_SIZE] = id;
        for &cell in cells {
            data[NAVICUST_GRID_OFFSET + cell] = slot as u8 + 1;
        }
    }

    #[test]
    fn reads_and_writes_navicust_parts() {
        let save = SaveSet::parse(&plausible()).unwrap().current();
        let mut save = save;
        {
            let mut navicust = save.view_navicust_mut().expect("the navicust is editable");
            assert!(navicust.set_navicust_part(
                3,
                Some(NavicustPart {
                    id: 188,
                    col: 1,
                    row: 4,
                    rot: 2,
                    compressed: true,
                })
            ));
            // Past the cart's table, and past the slots.
            assert!(!navicust.set_navicust_part(
                3,
                Some(NavicustPart {
                    id: crate::NUM_NAVICUST_PARTS,
                    col: 0,
                    row: 0,
                    rot: 0,
                    compressed: false,
                })
            ));
            assert!(!navicust.set_navicust_part(NUM_NAVICUST_SLOTS, None));
        }
        let navicust = save.view_navicust().unwrap();
        assert_eq!(navicust.count(), NUM_NAVICUST_SLOTS);
        assert_eq!(navicust.size(), [NAVICUST_SIZE, NAVICUST_SIZE]);
        let part = navicust.navicust_part(3).unwrap();
        assert_eq!((part.id, part.col, part.row, part.rot, part.compressed), (188, 1, 4, 2, true));
        // An id of 0 is an empty slot, not part 0.
        assert!(navicust.navicust_part(0).is_none());
    }

    /// MegaMan's HP is what the record says plus what the NaviCust
    /// adds; a team navi reports its own record's effective figure.
    #[test]
    fn the_navi_reports_hp_the_navicust_moves() {
        let mut data = plausible();
        data[NAVI_STATS_OFFSET..][..2].copy_from_slice(&1000u16.to_le_bytes());
        // HP+400 (template 46) on the grid, and HP+100 (43) in a slot
        // the grid doesn't show — an unplaced part grants nothing.
        place(&mut data, 0, 46 * 4, &[0, 1]);
        place(&mut data, 1, 43 * 4, &[]);

        let mut save = SaveSet::parse(&data).unwrap().current();
        let assets = crate::rom::Assets::new(&crate::rom::A5TE_00, crate::rom::EN_CHARSET, vec![]);
        assert_eq!(save.view_navi().unwrap().max_hp(&assets), 1400);

        // A file being played as a team navi reports that navi's own
        // effective HP, and hands out no navicust to edit.
        save.active_mut()[NAVI_OFFSET] = 2;
        let stats = NAVI_RECORD_OFFSET + 2 * NAVI_RECORD_SIZE + NAVI_STATS_INTO_RECORD;
        save.active_mut()[stats + 4..][..2].copy_from_slice(&777u16.to_le_bytes());
        assert_eq!(save.view_navi().unwrap().max_hp(&assets), 777);
        assert!(save.view_navicust().is_none());
        assert!(save.view_navicust_mut().is_none());
    }

    /// Karma reads off record 0, and a write keeps the anti-tamper
    /// mirror equal to `karma ^ key` — the relation the game verifies.
    #[test]
    fn karma_round_trips_with_its_mirror() {
        let mut data = plausible();
        data[KARMA_KEY_OFFSET..][..4].copy_from_slice(&0x080e_ea40u32.to_le_bytes());
        data[NAVI_RECORD_OFFSET + KARMA_INTO_RECORD..][..2].copy_from_slice(&1000u16.to_le_bytes());
        data[KARMA_MIRROR_OFFSET..][..4].copy_from_slice(&(0x080e_ea40u32 ^ 1000).to_le_bytes());

        let mut save = SaveSet::parse(&data).unwrap().current();
        assert_eq!(save.karma(), 1000);

        save.set_karma(0);
        assert_eq!(save.karma(), 0);
        let mirror = u32::from_le_bytes(save.active()[KARMA_MIRROR_OFFSET..][..4].try_into().unwrap());
        assert_eq!(mirror, 0x080e_ea40);

        // The clamp is the game's own.
        save.set_karma(u16::MAX);
        assert_eq!(save.karma(), KARMA_MAX);
        let mirror = u32::from_le_bytes(save.active()[KARMA_MIRROR_OFFSET..][..4].try_into().unwrap());
        assert_eq!(mirror, 0x080e_ea40 ^ KARMA_MAX as u32);
    }

    #[test]
    fn dark_hp_losses_round_trip() {
        let mut save = SaveSet::parse(&plausible()).unwrap().current();
        assert_eq!(save.dark_hp_losses(), 0);
        save.set_dark_hp_losses(3);
        assert_eq!(save.dark_hp_losses(), 3);
    }

    /// Setting a navi's HP lands in all three record fields, with the
    /// customizer's grant folded back into the effective figure.
    #[test]
    fn set_navi_hp_refolds_the_partycust_grant() {
        let mut save = SaveSet::parse(&plausible()).unwrap().current();
        assert!(save.set_navi_hp(0, 997));
        let stats = NAVI_RECORD_OFFSET + NAVI_STATS_INTO_RECORD;
        for at in [0, 2, 4] {
            let read = u16::from_le_bytes(save.active()[stats + at..][..2].try_into().unwrap());
            assert_eq!(read, 997);
        }

        let record = NAVI_RECORD_OFFSET + 2 * NAVI_RECORD_SIZE;
        save.active_mut()[record + PARTYCUST_MAX_HP_INTO_RECORD..][..2].copy_from_slice(&100u16.to_le_bytes());
        assert!(save.set_navi_hp(2, 500));
        assert_eq!(save.navi_hp(2), 600);
        assert!(!save.set_navi_hp(NUM_NAVIS, 500));
    }

    /// The folder's limits: the class caps move with the command line's
    /// own programs, Regular memory comes off the folder cluster.
    #[test]
    fn folder_limits_follow_the_command_line() {
        let mut data = plausible();
        data[REGULAR_MEMORY_OFFSET] = 60;
        // MegFldr2 (+2) on the command line, GigFldr1 (+1) off it.
        place(&mut data, 0, 5 * 4, &[NAVICUST_COMMAND_LINE * NAVICUST_SIZE]);
        place(&mut data, 1, 6 * 4, &[0]);

        let save = SaveSet::parse(&data).unwrap().current();
        let assets = crate::rom::Assets::new(&crate::rom::A5TE_00, crate::rom::EN_CHARSET, vec![]);
        let limits = save.view_navi().unwrap().folder_limits(&assets);
        assert_eq!(limits.mega_limit, Some(BASE_MEGA_LIMIT as usize + 2));
        assert_eq!(limits.giga_limit, Some(BASE_GIGA_LIMIT));
        assert_eq!(limits.dark_limit, Some(DARK_LIMIT));
        assert_eq!(limits.reg_memory, Some(60));
    }

    /// The battle's two NAVI CHANGE slots read and write as navi ids,
    /// and an empty slot reads as nothing.
    #[test]
    fn the_team_slots_round_trip() {
        let mut save = SaveSet::parse(&plausible()).unwrap().current();
        assert_eq!(save.view_party().navi(0), None, "an unwritten slot is empty");

        // Story flags share the mirror's bytes; they must survive edits.
        save.active_mut()[TEAM_MIRROR_OFFSET] = 0x48;

        assert!(save.view_party_mut().set_navi(0, Some(7)));
        assert!(save.view_party_mut().set_navi(1, Some(10)));
        assert_eq!(save.view_party().navi(0), Some(7));
        assert_eq!(save.view_party().navi(1), Some(10));
        // The game's own bias, so the console reads back what we wrote.
        assert_eq!(
            &save.active()[TEAM_NAVI_OFFSET..][..4],
            &(7 + TEAM_NAVI_BIAS).to_le_bytes()
        );
        // The mirror holds exactly the fielded pair's bits (navi 7 =
        // 0xeb & 0x10, navi 10 = 0xeb & 0x02), the load's cross-check.
        assert_eq!(save.active()[TEAM_MIRROR_OFFSET + 1], 0x12);
        // The story flags beside them are untouched.
        assert_eq!(save.active()[TEAM_MIRROR_OFFSET], 0x48);

        assert!(save.view_party_mut().set_navi(1, None));
        assert_eq!(save.view_party().navi(1), None);
        // Emptying the slot takes its navi's mirror bit with it.
        assert_eq!(save.active()[TEAM_MIRROR_OFFSET + 1], 0x10);

        // A gap is a team the battle refuses; packing closes it.
        assert!(save.view_party_mut().set_navi(0, None));
        assert!(save.view_party_mut().set_navi(1, Some(12)));
        save.pack_team();
        assert_eq!(save.view_party().navi(0), Some(12));
        assert_eq!(save.view_party().navi(1), None);

        // Past the panel's slots, and past the record array.
        assert!(!save.view_party_mut().set_navi(NUM_TEAM_SLOTS, Some(1)));
        assert!(!save.view_party_mut().set_navi(0, Some(NUM_NAVIS)));
        assert_eq!(save.view_party().navi(NUM_TEAM_SLOTS), None);
    }

    /// The picker offers the file's own team; the write layer takes
    /// any of the twelve, and the mirror bits span all three bytes
    /// without stepping on each other.
    #[test]
    fn any_navi_may_be_fielded() {
        let mut data = plausible();
        data[NAVI_RECORD_OFFSET + 9 * NAVI_RECORD_SIZE + PARTYCUST_ATTACK_INTO_RECORD] = 3;

        let mut save = SaveSet::parse(&data).unwrap().current();
        // The offer list is the recruit flags (0x268 + navi): nothing
        // recruited, nothing offered; set a few and exactly those come
        // back — flag 0x269 is 0xed & 0x40, flag 0x274 is 0xee & 0x08.
        assert_eq!(save.view_party().choices(), Vec::<usize>::new());
        save.active_mut()[0xed] |= 0x40;
        save.active_mut()[0xee] |= 0x88;
        assert_eq!(save.view_party().choices(), vec![1, 8, 12]);
        // MegaMan is who the panel changes *from*, never a slot's navi.
        assert!(!save.view_party_mut().set_navi(0, Some(0)));

        // The mirror's first and last bits: navi 1 in 0xea, navi 12 in
        // 0xec — the ends of the twelve-bit run.
        assert!(save.view_party_mut().set_navi(0, Some(1)));
        assert!(save.view_party_mut().set_navi(1, Some(12)));
        assert_eq!(save.active()[TEAM_MIRROR_OFFSET] & 0x07, 0x04);
        assert_eq!(save.active()[TEAM_MIRROR_OFFSET + 2] & 0x80, 0x80);
        // From the back: the pair packs, so emptying the first slot
        // would move the second up into it rather than empty the pair.
        assert!(save.view_party_mut().set_navi(1, None));
        assert!(save.view_party_mut().set_navi(0, None));
        assert_eq!(save.active()[TEAM_MIRROR_OFFSET] & 0x07, 0);
        assert_eq!(save.active()[TEAM_MIRROR_OFFSET + 2] & 0x80, 0);

        // The customizer's ATTACK is not a gate on being fielded.
        assert_eq!(save.partycust_bonus(9).attack, 3);
        assert_eq!(save.partycust_bonus(8).attack, 0);
    }

    /// A cart carrying just the party program tables, in the overlay
    /// [`crate::rom::A5TE_00`] names at the addresses it gives — the
    /// reads a customizer write makes, without a real cartridge on
    /// hand. The rows are the US cart's own (see
    /// [`crate::rom::Offsets::party_programs`]).
    fn party_program_cart() -> crate::rom::Assets {
        let mut rom = vec![0u8; 0x1_5000];
        // Enough header for the mapper: where the ARM9 image loads —
        // and, because the tables resolve through the filesystem the
        // way everything does now, where the overlay table and the FAT
        // are.
        rom[0x20..][..4].copy_from_slice(&0x4000u32.to_le_bytes());
        rom[0x28..][..4].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[0x2c..][..4].copy_from_slice(&0x0016_0d78u32.to_le_bytes());
        rom[0x48..][..4].copy_from_slice(&0x4400u32.to_le_bytes());
        rom[0x4c..][..4].copy_from_slice(&(414u32 * 8).to_le_bytes());
        rom[0x50..][..4].copy_from_slice(&0x1000u32.to_le_bytes());
        rom[0x54..][..4].copy_from_slice(&(414u32 * 0x20).to_le_bytes());
        // Overlay 413, the US cart's own shape: loads at 0x021ddbe0,
        // stored plain as file 413, whose FAT range holds the tables at
        // the same spots into the overlay as the real cart's.
        let entry = 0x1000 + 413 * 0x20;
        rom[entry..][..4].copy_from_slice(&413u32.to_le_bytes());
        rom[entry + 0x04..][..4].copy_from_slice(&0x021d_dbe0u32.to_le_bytes());
        rom[entry + 0x08..][..4].copy_from_slice(&0xfcc0u32.to_le_bytes());
        rom[entry + 0x18..][..4].copy_from_slice(&413u32.to_le_bytes());
        rom[0x4400 + 413 * 8..][..4].copy_from_slice(&0x6000u32.to_le_bytes());
        rom[0x4400 + 413 * 8 + 4..][..4].copy_from_slice(&(0x6000u32 + 0xeddc).to_le_bytes());
        rom[0x6000 + 0xed78..][..12].copy_from_slice(&[6, 8, 6, 8, 5, 10, 8, 6, 10, 6, 5, 8]);
        rom[0x6000 + 0xed9c..][..13].copy_from_slice(&[
            0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc,
        ]);
        rom[0x6000 + 0xedac..][..13].copy_from_slice(&[1, 2, 3, 5, 1, 2, 3, 3, 4, 5, 4, 7, 1]);
        rom[0x6000 + 0xedbc..][..13].copy_from_slice(&[3, 3, 3, 3, 4, 4, 4, 2, 2, 2, 5, 5, 5]);
        crate::rom::Assets::new(&crate::rom::A5TE_00, crate::rom::EN_CHARSET, rom)
    }

    /// What a member equips adds up into its record, the slot's own
    /// entry names the programs, and stripping them puts both back.
    ///
    /// The costs a real session needs come off the cart, as do the
    /// bonuses and item ids a plain write needs — all of them the
    /// program table's.
    #[test]
    fn equipped_party_programs_write_the_record_and_the_loadout() {
        let assets = party_program_cart();
        let mut save = SaveSet::parse(&plausible()).unwrap().current();
        assert!(save.view_party_mut().set_navi(0, Some(8)));
        assert_eq!(save.partycust_bonus(8), PartycustBonus::default());
        assert_eq!(save.view_party().programs(0, &assets), Vec::<usize>::new());

        // P.HP+300, P.Atk+2, P.Chp+40 and P.Spport, equipped together.
        assert!(save.view_party_mut().set_party_programs(0, [3, 5, 8, 12], &assets));
        assert_eq!(
            save.partycust_bonus(8),
            PartycustBonus {
                max_hp: 300,
                attack: 2,
                chip_attack: 40,
                support: true,
            }
        );
        assert_eq!(save.view_party().programs(0, &assets), vec![3, 5, 8, 12]);
        // The record is the game's own: HP and chip attack as u16 LE,
        // ATTACK and support as bytes, and the HP folded into the
        // effective max HP every other screen reads.
        let record = NAVI_RECORD_OFFSET + 8 * NAVI_RECORD_SIZE;
        assert_eq!(&save.active()[record + PARTYCUST_MAX_HP_INTO_RECORD..][..2], &[0x2c, 0x01]);
        assert_eq!(save.active()[record + PARTYCUST_SUPPORT_INTO_RECORD], 1);
        assert_eq!(save.navi_hp(8), 300);
        // And the slot's entry is the game's: the member, then the item
        // ids of what it equips.
        let entry = save.partycust_loadout(0);
        assert_eq!(entry[PARTYCUST_LOADOUT_NAVI], 8);
        assert_eq!(&entry[PARTYCUST_LOADOUT_PROGRAMS..][..5], &[0xb3, 0xb5, 0xb8, 0xbc, 0]);
        assert_eq!(entry[PARTYCUST_LOADOUT_ATTACK], 2);
        assert_eq!(entry[PARTYCUST_LOADOUT_CHIP_ATTACK], 40);

        // A battle pack is all three stats at once.
        assert!(save.view_party_mut().set_party_programs(0, [10], &assets));
        assert_eq!(
            save.partycust_bonus(8),
            PartycustBonus {
                max_hp: 50,
                attack: 1,
                chip_attack: 30,
                support: false,
            }
        );

        assert!(save.view_party_mut().set_party_programs(0, [], &assets));
        assert_eq!(save.partycust_bonus(8), PartycustBonus::default());
        assert_eq!(save.view_party().programs(0, &assets), Vec::<usize>::new());

        // No member is nothing to customize, and an entry longer than
        // the widest gauge is not one the game could have written.
        assert!(!save.view_party_mut().set_party_programs(1, [0], &assets));
        assert!(!save.view_party_mut().set_party_programs(0, [crate::NUM_PARTY_PROGRAMS], &assets));
        assert!(!save.view_party_mut().set_party_programs(0, [0; MAX_PARTY_PROGRAMS_EQUIPPED + 1], &assets));
    }

    /// Changing a member takes its programs back off, and packing the
    /// pair carries a member's loadout down with it.
    #[test]
    fn a_party_change_takes_the_members_programs_back_off() {
        let assets = party_program_cart();
        let mut save = SaveSet::parse(&plausible()).unwrap().current();
        assert!(save.view_party_mut().set_navi(0, Some(8)));
        assert!(save.view_party_mut().set_navi(1, Some(11)));
        assert!(save.view_party_mut().set_party_programs(1, [3], &assets));
        assert_eq!(save.partycust_bonus(11).max_hp, 300);

        // Emptying the first slot packs the second down into it, and
        // its loadout comes along.
        assert!(save.view_party_mut().set_navi(0, None));
        save.pack_team();
        assert_eq!(save.view_party().navi(0), Some(11));
        assert_eq!(save.view_party().programs(0, &assets), vec![3]);
        assert_eq!(save.partycust_bonus(11).max_hp, 300);

        // Swapping the member out is the game's own change: the
        // programs come off and the navi keeps nothing.
        assert!(save.view_party_mut().set_navi(0, Some(9)));
        assert_eq!(save.partycust_bonus(11), PartycustBonus::default());
        assert_eq!(save.view_party().programs(0, &assets), Vec::<usize>::new());
    }

    #[test]
    fn auto_battle_data_counts_round_trip() {
        let mut save = SaveSet::parse(&plausible()).unwrap().current();
        {
            let mut abd = save.view_auto_battle_data_mut().unwrap();
            assert!(abd.set_chip_use_count(5, 300));
            assert!(abd.set_secondary_chip_use_count(5, 7));
            // Past the arrays the cart keeps.
            assert!(!abd.set_chip_use_count(crate::NUM_AUTO_BATTLE_DATA_CHIPS, 1));
        }
        let abd = save.view_auto_battle_data().unwrap();
        assert_eq!(abd.chip_use_count(5), Some(300));
        assert_eq!(abd.secondary_chip_use_count(5), Some(7));
        assert_eq!(abd.chip_use_count(crate::NUM_AUTO_BATTLE_DATA_CHIPS), None);
    }

    /// Rebuilding the deck leaves the cart's own combo markers alone —
    /// the shared materializer has nothing to say about those slots.
    #[test]
    fn rebuilding_the_deck_keeps_the_combo_slots() {
        let mut data = plausible();
        let combo = AUTO_BATTLE_DATA_OFFSET + AUTO_BATTLE_DATA_COMBO_SLOTS.start * 2;
        data[combo..][..2].copy_from_slice(&0x8000u16.to_le_bytes());
        data[AUTO_BATTLE_DATA_OFFSET..][..2].copy_from_slice(&123u16.to_le_bytes());

        let mut save = SaveSet::parse(&data).unwrap().current();
        {
            let mut abd = save.view_auto_battle_data_mut().unwrap();
            abd.clear_materialized();
        }
        let block = save.active();
        assert_eq!(&block[combo..][..2], &0x8000u16.to_le_bytes(), "a combo slot was blanked");
        assert_eq!(
            &block[AUTO_BATTLE_DATA_OFFSET..][..2],
            &0xffffu16.to_le_bytes(),
            "an ordinary slot was not cleared"
        );
    }

    /// A cart with both files: file 0 in blocks 0-3 (generation 9 the
    /// live one), file 1 in blocks 4/5, each holding a distinct chip in
    /// folder 0 slot 0 so the tests can tell them apart.
    fn two_files() -> Vec<u8> {
        let mut data = plausible();
        for block in 0..2 {
            data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&9u32.to_le_bytes());
        }
        for block in 2..4 {
            data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&8u32.to_le_bytes());
        }
        for block in 4..6 {
            data[block * BLOCK_SIZE + FILE_SLOT_OFFSET] = 1;
            data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&3u32.to_le_bytes());
        }
        data[0 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(1, 0));
        data[2 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(99, 0));
        data[4 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(2, 0));
        data
    }

    #[test]
    fn hands_out_a_save_per_file() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();
        assert_eq!(set.slots(), vec![0, 1]);

        // Each file reads its own live block — file 0's older
        // generation (chip 99) is not what it hands out.
        let file0 = set.save(0).unwrap();
        let file1 = set.save(1).unwrap();
        assert_eq!(file0.slot(), 0);
        assert_eq!(file1.slot(), 1);
        assert_eq!(file0.view_chips().unwrap().chip(0, 0).unwrap().id, 1);
        assert_eq!(file1.view_chips().unwrap().chip(0, 0).unwrap().id, 2);

        // Every save knows the whole cart's files, for a file picker.
        assert_eq!(file0.slots(), &[0, 1]);
        assert_eq!(file1.slots(), &[0, 1]);

        // The most recently saved file is what the editor opens on, and
        // handing out a file never touches the dump's bytes.
        assert_eq!(set.current().slot(), 0);
        assert_eq!(file1.to_sram_dump(), data);
        assert!(set.save(9).is_none());
    }

    #[test]
    fn a_session_boots_the_dump_untouched() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();
        // Reading a file never touches the dump's bytes: which file a
        // session plays is already in them (the generation counters),
        // so handing one out has nothing to write.
        assert_eq!(set.save(0).unwrap().to_sram_dump(), data);
        assert_eq!(set.save(1).unwrap().to_sram_dump(), data);
    }

    /// Picking a file is an edit to the cartridge: the file becomes the
    /// one the game itself calls most recently saved, which is what
    /// [`SaveSet::current`] — and so every session, recording and
    /// priming walk — reads.
    #[test]
    fn making_a_file_current_moves_what_the_set_hands_out() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();
        assert_eq!(set.current().slot(), 0);

        let mut file1 = set.save(1).unwrap();
        file1.make_current();
        file1.rebuild_checksum();
        let dump = file1.to_sram_dump();
        let set = SaveSet::parse(&dump).unwrap();
        assert_eq!(set.current().slot(), 1);
        // Both files are still there, each still reading its own block.
        assert_eq!(set.slots(), vec![0, 1]);
        assert_eq!(set.save(0).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 1);
        assert_eq!(set.save(1).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 2);

        // Already leading: nothing moves, and picking it again is a
        // no-op rather than another bump.
        let mut again = set.save(1).unwrap();
        let before = again.to_sram_dump();
        again.make_current();
        assert_eq!(again.to_sram_dump(), before);
    }

    /// The cross is the game's own byte, per file, and survives the
    /// round trip through a rebuilt block.
    #[test]
    fn the_cross_round_trips_per_file() {
        let mut data = two_files();
        // File 0 is Team ProtoMan, file 1 Team Colonel — which is what
        // picks between the two BassCross values. (Their live blocks are
        // 0 and 4; see `two_files`.)
        data[0 * BLOCK_SIZE + TEAM_OFFSET] = 0;
        data[4 * BLOCK_SIZE + TEAM_OFFSET] = 1;
        let set = SaveSet::parse(&data).unwrap();
        assert_eq!(set.save(0).unwrap().cross(), Cross::None);
        assert_eq!(Cross::bass_for(set.save(0).unwrap().team()), Cross::BassProto);
        assert_eq!(Cross::bass_for(set.save(1).unwrap().team()), Cross::BassColonel);

        let mut file1 = set.save(1).unwrap();
        file1.set_cross(Cross::Sol);
        file1.rebuild_checksum();
        let set = SaveSet::parse(&file1.to_sram_dump()).unwrap();
        assert_eq!(set.save(1).unwrap().cross(), Cross::Sol);
        // The other file is untouched — the byte lives in each file's
        // own block.
        assert_eq!(set.save(0).unwrap().cross(), Cross::None);
    }

    #[test]
    fn each_file_edits_only_its_own_block() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();

        let mut file1 = set.save(1).unwrap();
        {
            let mut chips = chips_mut(&mut file1);
            assert!(chips.set_chip(
                0,
                0,
                tango_gamesupport_common_dataview::save::Chip {
                    id: 55,
                    code: ChipCode::B,
                },
            ));
        }
        file1.rebuild_checksum();

        // File 0's blocks are untouched, and re-reading the edited dump
        // through the set sees the edit only in file 1.
        let dump = file1.to_sram_dump();
        assert_eq!(dump[..2 * BLOCK_SIZE], data[..2 * BLOCK_SIZE]);
        let set = SaveSet::parse(&dump).unwrap();
        assert_eq!(set.save(0).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 1);
        assert_eq!(set.save(1).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 55);
    }

    #[test]
    fn edits_rebuild_into_a_block_the_game_would_load() {
        let mut data = plausible();
        // Make block 2's pair the active file contents.
        data[2 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&7u32.to_le_bytes());
        data[3 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&7u32.to_le_bytes());

        let mut save = SaveSet::parse(&data).unwrap().current();
        {
            let mut chips = chips_mut(&mut save);
            assert!(chips.set_chip(
                0,
                0,
                tango_gamesupport_common_dataview::save::Chip {
                    id: 42,
                    code: ChipCode::C,
                },
            ));
            assert!(chips.set_equipped_folder(2));
            assert!(chips.set_regular_chip_index(0, Some(5)));
            assert!(chips.set_pack_count(42, 2, 99));
        }
        save.rebuild_checksum();

        let dump = save.to_sram_dump();
        // The edited pair is mirrored...
        assert_eq!(dump[2 * BLOCK_SIZE..3 * BLOCK_SIZE], dump[3 * BLOCK_SIZE..4 * BLOCK_SIZE]);
        // ...its checksum pairs hold the game's invariants...
        let block = &dump[2 * BLOCK_SIZE..3 * BLOCK_SIZE];
        let word = |o: usize| u16::from_le_bytes(block[o..o + 2].try_into().unwrap());
        assert_eq!(word(CHECKSUM_OFFSET).wrapping_add(word(CHECKSUM_OFFSET + 2)), 0);
        assert_eq!(word(CHECKSUM_OFFSET + 4).wrapping_add(word(CHECKSUM_OFFSET + 6)), 0);
        assert_eq!(word(CHECKSUM_OFFSET + 4), checksum(&block[..SAVE_IMAGE_SIZE]));
        assert_eq!(
            word(CHECKSUM_OFFSET),
            checksum(&block[FOOTER_SUM_START..FOOTER_SUM_END])
        );
        // ...including the interior byte-sum the game verifies at load.
        // These edits change byte VALUES (a reorder wouldn't — byte
        // sums are permutation-invariant, which is how a stale interior
        // once hid), so a stale one would boot the new-game path.
        let image = &block[..SAVE_IMAGE_SIZE];
        assert_eq!(
            u32::from_le_bytes(image[INTERIOR_CHECKSUM_OFFSET..][..4].try_into().unwrap()),
            image.iter().map(|&v| v as u32).sum::<u32>().wrapping_sub(
                image[INTERIOR_CHECKSUM_OFFSET..][..4]
                    .iter()
                    .map(|&v| v as u32)
                    .sum::<u32>()
            ),
        );
        // ...and a re-parse sees the edits.
        let reparsed = SaveSet::parse(&dump).unwrap().current();
        let chips = reparsed.view_chips().unwrap();
        assert_eq!(chips.chip(0, 0).unwrap().id, 42);
        assert_eq!(chips.equipped_folder_index(), 2);
        assert_eq!(chips.regular_chip_index(0), Some(Some(5)));
        assert_eq!(chips.pack_count(42, 2), Some(99));
    }

    #[test]
    fn pack_counts_keep_their_acquisition_keys_in_shape() {
        let data = plausible();
        let mut save = SaveSet::parse(&data).unwrap().current();
        let key_at = |save: &Save, id: usize, variant: usize| {
            let off = PACK_OFFSET + id * PACK_ENTRY_SIZE + 4 + variant * 2;
            u16::from_le_bytes(save.active()[off..][..2].try_into().unwrap())
        };

        // The first grant on an empty pack takes the first key, and the
        // next one takes the key below it.
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(7, 2, 4));
        drop(chips);
        assert_eq!(key_at(&save, 7, 2), PACK_KEY_FIRST);
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(9, 0, 1));
        drop(chips);
        assert_eq!(key_at(&save, 9, 0), PACK_KEY_FIRST - 1);

        // A key sticks across count changes and clears with the count.
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(7, 2, 1));
        drop(chips);
        assert_eq!(key_at(&save, 7, 2), PACK_KEY_FIRST);
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(7, 2, 0));
        drop(chips);
        assert_eq!(key_at(&save, 7, 2), 0);

        // Ids without a pack slot stay unpokeable.
        let mut chips = chips_mut(&mut save);
        assert!(!chips.set_pack_count(NUM_PACK_CHIPS, 0, 1));
        assert_eq!(chips.pack_count(NUM_PACK_CHIPS, 0), None);
    }
}

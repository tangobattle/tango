//! PvP-engine support: priming pokes and telemetry polls.
//!
//! Priming walks the game's own boot code with the human sync points
//! PC-redirected: the logo skip rides the start screen's own
//! fade-gated title transition, the intro skip runs the intro's own
//! exit handler, CONTINUE skips the title menu's gates into its own
//! confirm handler (cursor preset to the CONTINUE row), and the comm
//! menu opens through the game's own open-comm routine, redirected
//! from the title exit's load tick — every dispatcher byte is written
//! by ROM code. A start-battle poke then routes the submenu
//! dispatcher into the link-battle flow, and everything from the
//! settings state on runs the game's real link protocol over the
//! emulated cable — none of the trap engine's SIO stand-ins are
//! installed. The rngs are seeded per core once at save load (two
//! cartridges never share RNG state on real hardware), so the
//! players' draws diverge naturally and whatever must agree is agreed
//! over the wire.

use tango_backend_mgba::Trap;
use tango_gamesupport_common::telemetry::LoadedChip;

pub struct Pvp {
    offsets: &'static Offsets,
}

pub static PVP_BR4J_00: Pvp = Pvp { offsets: &BR4J_00 };

impl Pvp {
    /// Raw submenu-control bytes, for headless probe diagnostics.
    pub fn debug_menu_state(&self, core: &mut mgba::core::Core) -> [u8; 8] {
        let mut buf = [0u8; 8];
        core.raw_read_range(self.offsets.ewram.submenu_control, -1, &mut buf);
        buf
    }

    /// Raw unit-slot state (owner bytes + current HP per slot), for
    /// headless probe diagnostics.
    pub fn debug_battle_state(&self, core: &mut mgba::core::Core) -> [u8; 8] {
        let ewram = &self.offsets.ewram;
        let mut buf = [0u8; 8];
        for slot in 0..2u32 {
            let unit = read_unit(ewram, core, slot);
            buf[slot as usize * 4] = unit.owner;
            let hp = unit.hp;
            buf[slot as usize * 4 + 1] = (hp & 0xff) as u8;
            buf[slot as usize * 4 + 2] = (hp >> 8) as u8;
            buf[slot as usize * 4 + 3] = core.raw_read_8(ewram.custom_flags + slot, -1);
        }
        buf
    }

    /// Both players' current in-battle HP, for headless probe control
    /// (same read as the poller's).
    pub fn debug_battle_hp(&self, core: &mut mgba::core::Core) -> Option<[u16; 2]> {
        battle_units(&self.offsets.ewram, core).map(|units| units.map(|u| u.hp))
    }

    /// Probe tooling (the KO recipe): set `player`'s current in-battle
    /// HP by finding the unit slot that player owns this round. Returns
    /// false (writing nothing) while the slots aren't two live player
    /// units. Call on BOTH cores in the same tick to keep the pair's
    /// simulations agreeing.
    pub fn debug_set_hp(&self, core: &mut mgba::core::Core, player: u8, hp: u16) -> bool {
        let ewram = &self.offsets.ewram;
        let mut owners = [0u8; 2];
        for slot in 0..2u32 {
            owners[slot as usize] = read_unit(ewram, core, slot).owner;
        }
        if !matches!(owners, [0, 1] | [1, 0]) {
            return false;
        }
        for slot in 0..2u32 {
            if owners[slot as usize] == player {
                core.raw_write_16(unit_field(ewram, slot, std::mem::offset_of!(RawUnit, hp)), -1, hp);
                return true;
            }
        }
        false
    }
}

impl tango_backend_mgba::GameSupport for Pvp {
    fn primer_traps(
        &self,
        config: &tango_backend_mgba::PrimeConfig,
        player: usize,
        events: &tango_match::telemetry::EventSink,
        primed: &tango_backend_mgba::PrimedLatch,
    ) -> Vec<Trap> {
        use tango_match::telemetry::Outcome;

        let rom = &self.offsets.rom;
        let ewram = &self.offsets.ewram;
        let disable_bgm = config.disable_bgm;
        let match_type = config.match_type.0;
        // Seed the rngs per core once, at save load (see module docs).
        let rng1 = config.core_rng_seed(player, 0);
        let rng2 = config.core_rng_seed(player, 1);
        // Redirect targets, copied out for the move closures (see each
        // trap below).
        let start_screen_title_transition = rom.start_screen_title_transition;
        let intro_exit = rom.intro_exit;
        let title_menu_confirm = rom.title_menu_confirm;
        let open_comm_menu = rom.open_comm_menu;
        // Lifecycle signals are host-side only — core state is untouched,
        // so the simulation is unaffected. Rounds are reported from core 0
        // (whose local player is player 0); core 1's lifecycle traps stay
        // silent. Match end is the exception, reported from both cores —
        // see its trap below.
        let sink = (player == 0).then(|| events.clone());
        let primed = primed.clone();
        // The game's own round verdict, announced from the sites
        // where the battle loop decides it (the KO path and the
        // timeout damage judge). Core 0's "local player" is player
        // 0, so its set_win is P0's win. The round itself closes at
        // the next round start or the match end; these only stamp
        // the verdict.
        let verdict = |addr: u32, outcome: Outcome| -> Trap {
            let sink = sink.clone();
            (
                addr,
                Box::new(move |_core: &mut mgba::core::Core| {
                    if let Some(sink) = &sink {
                        sink.round_outcome(outcome);
                    }
                }),
            )
        };

        vec![
            // ----- the boot fast-path -----
            (
                // The start screen's state-0 (logo) handler entry — r5 =
                // title_menu_control, dispatcher-loaded. Redirect to the
                // applet's own state-3 transition handler (a full
                // `push {lr}` .. `pop {pc}` body): fade-gated `[r5] =
                // 0x10`, the same handoff the logo's organic
                // timeout/keypress path lands (state 4 opens the intro).
                // The trap re-fires each tick while state 0 holds, so the
                // gated store self-retries until the game walks on itself.
                rom.start_screen_logo_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(start_screen_title_transition);
                }),
            ),
            (
                // The intro's state-0 (PET zoom bring-up) handler entry —
                // r5 = intro_control, dispatcher-loaded. Redirect to the
                // intro's own state-2 exit handler (a full `push {lr}` ..
                // `pop {pc}` body, the state the organic A-press skip
                // lands): it tears the intro down, zeroes the title block
                // and flips the subsystem to the title screen — after
                // which the intro dispatcher never runs again.
                rom.intro_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(intro_exit);
                }),
            ),
            (
                // The title menu's PUSH-START wait (title state 2, sub 4)
                // at its handler entry, re-fired each tick the wait holds.
                // Preset the menu cursor to the CONTINUE row (a human's
                // selection; the organic default with a valid save is row
                // 0, NEW GAME), then redirect to the full menu-confirm
                // handler (state 2 sub 0xc, `push {lr}` .. `pop {pc}`):
                // fade-gated, it reads the cursor and writes the walk to
                // the title exit itself. This replaces the trap-era block
                // poke at the title init's return; the init, the state-1
                // bring-up and the menu init (which seeds the confirm's
                // timer gate) now run for real.
                rom.title_pushstart_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_8(ewram.title_menu_control + 0x08, -1, 0x01);
                    core.gba_mut().cpu_mut().set_thumb_pc(title_menu_confirm);
                }),
            ),
            (
                // The title exit's terminal `pop {r7, pc}`, popped once
                // per state-3 tick. Gated on the exit's countdown byte:
                // it hits 0 exactly on the tick whose deep path runs the
                // CONTINUE load and the field bring-up — the countdown
                // ticks before it pop straight through. On that tick, seed
                // the rngs (see module docs) and, instead of popping,
                // PC-redirect into the game's own open-comm-menu routine
                // one instruction past its `push {r4, lr}` (the routine
                // the overworld comm-terminal event runs): it saves the
                // field position, zeroes the submenu block, sets the comm
                // applet id and flips the subsystem to submenu mode —
                // everything the old poke wrote by hand. Its terminating
                // `pop {r4, pc}` pops this handler's own saved {r7, lr}
                // (two words for two), so control returns to the title
                // dispatcher cleanly, and the comm menu opens without the
                // field ever ticking — after which this state never runs
                // again.
                rom.game_load_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    if core.raw_read_8(ewram.title_menu_control + 0x1a, -1) != 0 {
                        return;
                    }
                    core.raw_write_32(ewram.rng1_state, -1, rng1);
                    core.raw_write_32(ewram.rng2_state, -1, rng2);
                    core.gba_mut().cpu_mut().set_thumb_pc(open_comm_menu);
                }),
            ),
            (
                rom.comm_menu_init_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    // Deliberately NOT first-battle-gated (unlike bn4/5/6):
                    // exe45's post-battle flow re-inits the comm menu and
                    // re-runs this init every time, and the rematch's link
                    // bring-up only completes when this route poke is
                    // re-applied (gated off, the organic bring-up loops on a
                    // comm retry forever — KO-probe observed). The poke
                    // lands the dispatcher on the post-battle menu
                    // ([1] = 0x08) where the game's own input then chooses:
                    // accept walks to battle init ([1] = 0x10), decline walks
                    // to teardown ([1] = 0x0c) — so re-firing here does not
                    // force a rematch and does not corrupt the decline path
                    // (both KO-probe verified).
                    // Route the submenu dispatcher into the link-battle flow.
                    core.raw_write_8(ewram.submenu_control + 0x0, -1, 0x18);
                    core.raw_write_8(ewram.submenu_control + 0x1, -1, 0x08);
                    core.raw_write_8(ewram.submenu_control + 0x2, -1, 0x0C);
                    // submenu_control[3] = 0 routes the outer dispatcher to
                    // the settings-handler path (0x04 skips it). The handler
                    // calls the game's generator and writes
                    // submenu_control[0x15]/[0x16].
                    core.raw_write_8(ewram.submenu_control + 0x3, -1, 0x00);
                    // [0x10] is the match_type byte the generator reads to
                    // pick the per-match_type settings range; the game would
                    // normally populate it during the comm-menu UI flow that
                    // we skip.
                    core.raw_write_8(ewram.submenu_control + 0x10, -1, match_type);
                    core.raw_write_8(ewram.submenu_control + 0x11, -1, 0x01);
                    core.raw_write_8(ewram.submenu_control + 0x14, -1, match_type * 2 + 1);
                }),
            ),
            (
                // The battle-start routine's BGM call (a 4-byte `bl`):
                // skipped when the host asked for silent battles. Purely
                // local presentation — the sound driver's state never
                // feeds battle logic, so peers may disagree.
                rom.battle_start_play_music_call,
                Box::new(move |core: &mut mgba::core::Core| {
                    if !disable_bgm {
                        return;
                    }
                    let pc = core.gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                }),
            ),
            (
                // The game's own battle start: the priming handoff (the
                // trap engine's match-start hook — priming ends when this
                // fires on both cores) and, for core 0, the round
                // lifecycle signal.
                rom.round_start_ret,
                {
                    let sink = sink.clone();
                    Box::new(move |_core: &mut mgba::core::Core| {
                        primed.set();
                        if let Some(sink) = &sink {
                            sink.round_started();
                        }
                    })
                },
            ),
            verdict(rom.round_end_set_win, Outcome::P0Win),
            verdict(rom.round_end_set_loss, Outcome::P1Win),
            verdict(rom.round_end_damage_judge_set_win, Outcome::P0Win),
            verdict(rom.round_end_damage_judge_set_loss, Outcome::P1Win),
            verdict(rom.round_end_damage_judge_set_draw, Outcome::Draw),
            (
                // The game's own match end: the trap engine's
                // match_end_ret, restored — the battle mode's return to
                // the comm dispatcher, reached when the game's OWN battle
                // set is over (mode 1, triple: best-of-three chained by
                // the game itself; mode 0: one single battle). Mid-set the
                // game chains straight into the next battle
                // (`round_start_ret` re-fires) without returning here.
                // Trapped on BOTH cores: whichever core's game leaves its
                // set first ends the match. The telemetry store dedups the
                // second core's firing.
                rom.match_end_ret,
                {
                    let sink = events.clone();
                    Box::new(move |_core: &mut mgba::core::Core| sink.match_ended())
                },
            ),
        ]
    }

    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<mgba::core::Core>> {
        // Chip-use detection for the vanilla dealt-queue contract: the
        // per-tick reading is the queue's id SUM (see
        // `EWRAMOffsets::chip_queues`). The queue only ever gains chips
        // (deals) or loses exactly the fired chip, so a drop in the sum
        // IS a use event and the delta IS the chip id; increases are
        // deals and are ignored. Chosen over watching the queue head
        // because exe45 players fire from a hand in an order the head
        // doesn't determine.
        #[derive(Clone, Default)]
        struct QueueSumTracker {
            round: u32,
            /// Last tick's sum, `None` on a fresh round (the first
            /// reading only establishes the baseline).
            prev: Option<u16>,
        }
        impl QueueSumTracker {
            fn tick(&mut self, round: u32, sum: u16, player: usize, events: &tango_match::telemetry::EventSink) {
                // Sanity bound on a chip id: drops above this are queue
                // clears (KO, round end), not uses.
                const MAX_CHIP_ID: u16 = 0x1ff;
                if self.round != round {
                    *self = Self { round, prev: None };
                }
                if let Some(p) = self.prev {
                    if sum < p && p - sum <= MAX_CHIP_ID {
                        events.chip_used(player, p - sum);
                    }
                }
                self.prev = Some(sum);
            }
        }

        #[derive(Clone)]
        struct Poller {
            ewram: &'static EWRAMOffsets,
            player: usize,
            /// Under the bn45_us_pvp patch (probed per tick off the
            /// core, like the reads themselves): the per-screen hand's
            /// fired counter (see `EWRAMOffsets::pvp_hand_blocks`).
            hand: tango_gamesupport_common::telemetry::HandChipTracker,
            /// Vanilla: the dealt-queue sums. Only one of the two ever
            /// advances — the cart doesn't change mid-session.
            queue: QueueSumTracker,
        }
        impl tango_match::telemetry::CorePoller<mgba::core::Core> for Poller {
            fn poll(
                &mut self,
                core: &mut mgba::core::Core,
                events: &tango_match::telemetry::EventSink,
                round: u32,
            ) -> Option<tango_match::telemetry::CoreObs> {
                let units = battle_units(self.ewram, core)?;
                // Whether this player currently has the battle-pausing
                // tactics/chip screen open (see
                // `EWRAMOffsets::custom_flags`) — nonzero across the
                // screen's sub-modes, 0 during action.
                let custom_self = core.raw_read_8(self.ewram.custom_flags + self.player as u32, -1) != 0;
                match own_chip_reading(self.ewram, core, self.player) {
                    ChipReading::Hand(token) => self.hand.tick(
                        round,
                        token,
                        custom_self,
                        units[self.player].hp,
                        self.player,
                        events,
                    ),
                    ChipReading::QueueSum(sum) => self.queue.tick(round, sum, self.player, events),
                }
                Some(tango_match::telemetry::CoreObs {
                    units: units.map(|u| tango_match::telemetry::UnitObs {
                        hp: u.hp,
                        tile: (u.tile[0], u.tile[1]),
                    }),
                    custom_self,
                })
            }
        }
        Box::new(Poller {
            ewram: &self.offsets.ewram,
            player,
            hand: Default::default(),
            queue: Default::default(),
        })
    }
}

// ---------------------------------------------------------------------------
// The in-battle unit record.

/// One in-battle unit, as the game lays it out. The first slot's
/// address is `EWRAMOffsets::unit` and the second follows immediately,
/// so the record's size IS the slot stride -- which the assert below
/// pins. Only the fields telemetry reads are named; the rest stays
/// `_reserved_*`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::AnyBitPattern, bytemuck::NoUninit)]
#[allow(dead_code)] // some fields are named for completeness, not read
struct RawUnit {
    _reserved_00: [u8; 0x12],
    /// The tile the unit stands on, `[x, y]`, 1-based over the whole
    /// field: x 1..=6 left to right (columns 1-3 are the left player's
    /// side), y 1..=3 top to bottom. Derived empirically: a scripted
    /// d-pad route steps them +/-1 per move, and both units' values
    /// match the rendered field.
    tile: [u8; 2],
    /// Where a move in flight is headed. Leads `tile` by a few ticks
    /// while the move animates, which is why `tile` is the one we
    /// report -- it says where the unit IS.
    dest_tile: [u8; 2],
    /// The unit's owner: its absolute player index (0/1). Which player
    /// owns which slot varies per round, so every read of the pair goes
    /// through this byte.
    owner: u8,
    _reserved_17: [u8; 0xd],
    /// Current HP -- not the animated HUD counter. Derived empirically
    /// from the golden replays: starts at the save's computed max HP,
    /// drops on hits, hits 0 at the loser's KO tick, identically across
    /// regions and both perspectives.
    hp: u16,
    max_hp: u16,
    _reserved_28: [u8; 0xb0],
}
const _: () = assert!(std::mem::size_of::<RawUnit>() == 0xd8);

/// Address of `field` (an `offset_of!(RawUnit, _)`) in slot `slot`'s
/// record.
fn unit_field(ewram: &EWRAMOffsets, slot: u32, field: usize) -> u32 {
    ewram.unit + slot * std::mem::size_of::<RawUnit>() as u32 + field as u32
}

/// Unit slot `slot`'s record.
fn read_unit(ewram: &EWRAMOffsets, core: &mut mgba::core::Core, slot: u32) -> RawUnit {
    let mut buf = [0u8; std::mem::size_of::<RawUnit>()];
    core.raw_read_range(unit_field(ewram, slot, 0), -1, &mut buf);
    bytemuck::pod_read_unaligned(&buf)
}

/// Both players' unit records, indexed by absolute player index. Each
/// slot's owner byte says which player it belongs to this round (the
/// assignment swaps between rounds). `None` while the slots aren't two
/// live player units -- the battle intro, before unit init.
fn battle_units(ewram: &EWRAMOffsets, core: &mut mgba::core::Core) -> Option<[RawUnit; 2]> {
    let mut units = [None, None];
    for slot in 0..2u32 {
        let unit = read_unit(ewram, core, slot);
        *units.get_mut(unit.owner as usize)? = Some(unit);
    }
    Some([units[0]?, units[1]?])
}
/// One tick's chip reading for `player`, in whichever contract the
/// cart follows.
enum ChipReading {
    /// bn45_us_pvp patch: the next-to-fire hand entry with the fire
    /// count (see `EWRAMOffsets::pvp_hand_blocks`) — the hand-cursor
    /// contract `HandChipTracker` detects fires on.
    Hand(Option<LoadedChip>),
    /// Vanilla: the dealt queue's id sum (see
    /// `EWRAMOffsets::chip_queues`). Always a reading, never an
    /// absence — an empty queue sums to 0, which is what the sum
    /// deltas are measured against.
    QueueSum(u16),
}

/// Read `player`'s chip state. The patch is probed per call — 8 ROM
/// byte reads, cheap next to the vanilla path's 30 queue reads.
fn own_chip_reading(ewram: &EWRAMOffsets, mut core: &mut mgba::core::Core, player: usize) -> ChipReading {
    if is_pvp_patch_core(&mut core) {
        let base = ewram.pvp_hand_blocks + player as u32 * 0x50;
        let fired = core.raw_read_16(base, -1) as u32;
        let mut reading = None;
        if fired < 6 {
            let id = core.raw_read_16(base + 2 + fired * 2, -1);
            if id != 0xffff {
                reading = Some(LoadedChip {
                    id,
                    fires: fired as u16,
                });
            }
        }
        return ChipReading::Hand(reading);
    }
    let base = ewram.chip_queues + player as u32 * 0x42;
    let mut sum = 0u16;
    for i in 0..30u32 {
        let id = core.raw_read_16(base + i * 2, -1);
        if id != 0xffff {
            sum = sum.wrapping_add(id);
        }
    }
    ChipReading::QueueSum(sum)
}

// ---------------------------------------------------------------------------

/// Marker identifying the community bn45_us_pvp patch: 8 bytes of the
/// patch's own code/data in ROM space that is 0xff padding on the
/// vanilla cart. Identical across every released patch version
/// (v0.0.1–v0.6.0) and still 0xff under the plain bn45_us translation,
/// which must NOT be detected (it keeps the vanilla battle system).
const PVP_PATCH_MARKER_OFFSET: u32 = 0x7ed2f5;
const PVP_PATCH_MARKER: [u8; 8] = [0xfb, 0xee, 0xe3, 0x19, 0x35, 0x31, 0xf4, 0xfb];

/// Whether the loaded cart is the bn45_us_pvp patch (any version) —
/// flips the chip-report contract (per-screen hands instead of the
/// dealt queue). Read off the core, for the per-tick polls.
fn is_pvp_patch_core(core: &mut &mut mgba::core::Core) -> bool {
    PVP_PATCH_MARKER
        .iter()
        .enumerate()
        .all(|(i, &b)| core.raw_read_8(0x0800_0000 + PVP_PATCH_MARKER_OFFSET + i as u32, -1) == b)
}

// ---------------------------------------------------------------------------
// Per-version EWRAM/ROM offsets.

#[derive(Clone, Copy)]
struct EWRAMOffsets {
    /// Title menu jump table control.
    title_menu_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    submenu_control: u32,

    /// Local RNG state. Doesn't need to be synced.
    rng1_state: u32,

    /// Shared RNG state. Must be synced.
    rng2_state: u32,

    /// The first in-battle unit's [`RawUnit`] record; the second
    /// follows immediately. This is the record the game itself hands
    /// around -- both slots' addresses sit in its own unit pointer
    /// table (0x02033054 on this version), which is how the base was
    /// pinned rather than guessed from a mid-struct anchor.
    unit: u32,
    /// Player 0's dealt-chip queue (30 u16 ids, 0xFFFF = empty); player
    /// 1's is 0x42 beyond. Chips are dealt into the tail over the auto-
    /// custom cycle and fired from a small hand at the front in an order
    /// the player chooses, so the queue's SUM (see `QueueSumTracker` in
    /// the poller) is what encodes uses: it drops by exactly the fired
    /// id. Indexed by absolute player. Derived empirically from the
    /// golden replays.
    chip_queues: u32,

    /// Screen-state byte pair (+0/+1) in the battle struct: nonzero while
    /// the tactics/chip screen stands open — the screens that genuinely
    /// PAUSE the battle (the battle-logic tick counter at +0x2c from
    /// here freezes; HP and chip activity stop). Values vary across
    /// screen sub-modes (5 while picking, other nonzero values on other
    /// pages), so the predicate is `!= 0`, see
    /// the poller. Same shape as bn5/bn6's
    /// battle_state flags. Same address vanilla and under the
    /// bn45_us_pvp patch; verified against tick-counter stall windows on
    /// both (July 2026). The PREVIOUS address here (0x0200db3c) tracked
    /// the periodic operation-gauge/deal cycle, which does NOT pause the
    /// battle — chips fire straight through it — and made "custom" spans
    /// meaningless for this game.
    custom_flags: u32,

    /// bn45_us_pvp patch ONLY (see [`is_pvp_patch`](super::is_pvp_patch)):
    /// per-player hand blocks in the battle struct, +0x50 per absolute
    /// player. Block layout: u16 fired-count at +0 (resets to 0 when the
    /// tactics/chip screen commits a new hand), u16 ids[6] at +2 in fire
    /// order (raw ids, 0xffff = empty slot; the whole block is zeros
    /// before the round's first commit). Each fire consumes ids[fired]
    /// and increments the count — including auto-fires on an idle side.
    /// Annotated display copies of the ids (flag bits in the high byte)
    /// sit at +0x32 and in a heap arena near 0x0203a1b0 whose layout
    /// shifts per round; only this block is stable. The vanilla dealt
    /// queue at [`chip_queues`](Self::chip_queues) reads all-zero under
    /// the patch. Derived July 2026 from v0.6.0 replays (per-tick WRAM
    /// diff + press correlation against the fixed custom spans); layout
    /// assumed stable across patch versions (marker bytes are).
    pvp_hand_blocks: u32,
}

#[derive(Clone, Copy)]
struct ROMOffsets {
    /// The start screen's state-0 (CAPCOM logo) handler entry, from the
    /// applet's jump table — r5 = title_menu_control is
    /// dispatcher-loaded here. Trapped to redirect into
    /// `start_screen_title_transition`.
    start_screen_logo_entry: u32,

    /// The start screen's state-3 transition handler: fade-gated
    /// `[r5] = 0x10`, the applet's own walk to its handoff state (which
    /// opens the intro). A full `push {lr}` .. `pop {pc}` body —
    /// `start_screen_logo_entry`'s trap redirects here.
    start_screen_title_transition: u32,

    /// The intro's state-0 (PET zoom bring-up) handler entry, from the
    /// intro applet's jump table — r5 = intro_control is
    /// dispatcher-loaded here. Trapped to redirect into `intro_exit`.
    intro_entry: u32,

    /// The intro's state-2 exit handler — the state the organic A-press
    /// skip lands: tears the intro down, zeroes the title block and
    /// flips the subsystem to the title screen. A full `push {lr}` ..
    /// `pop {pc}` body — `intro_entry`'s trap redirects here.
    intro_exit: u32,

    /// The title menu's PUSH-START wait handler entry (title state 2,
    /// sub-state 4), from the state-2 sub-table. Trapped to preset the
    /// menu cursor and redirect into `title_menu_confirm`.
    title_pushstart_entry: u32,

    /// The title menu's confirm handler (title state 2, sub-state 0xc):
    /// fade-gated, reads the menu cursor at `title_menu_control + 8`
    /// and walks the dispatcher to the title exit itself. A full
    /// `push {lr}` .. `pop {pc}` body — `title_pushstart_entry`'s trap
    /// redirects here.
    title_menu_confirm: u32,

    /// The title exit handler's terminal `pop {r7, pc}` — popped every
    /// state-3 tick; on the countdown-zero tick the handler's deep path
    /// has just run the CONTINUE load and the field bring-up. The trap
    /// seeds the rngs there and redirects into `open_comm_menu`.
    game_load_ret: u32,

    /// The game's own open-comm-menu routine (the one the overworld
    /// comm-terminal event runs), one instruction past its
    /// `push {r4, lr}`: saves the field position, zeroes the submenu
    /// block, sets the comm applet id and flips the subsystem to
    /// submenu mode, ending in `pop {r4, pc}` — a two-word frame
    /// matching `game_load_ret`'s `{r7, lr}`, whose trap redirects
    /// here.
    open_comm_menu: u32,

    /// This is the entry point to the comm menu.
    ///
    /// Here, Tango jumps directly into link battle.
    comm_menu_init_ret: u32,

    /// This hooks the point after the battle start routine is complete —
    /// the game's own round start, reported to the telemetry lifecycle
    /// sink.
    round_start_ret: u32,

    /// The battle-start routine's BGM call (a 4-byte `bl`). PC-skipped
    /// by the primer when the host asked for silent battles
    /// (`PrimeConfig::disable_bgm`).
    battle_start_play_music_call: u32,

    /// Where the battle loop stores the round result: the local player
    /// (= player 0 on core 0, whose traps report) won/lost by KO. The
    /// round verdict, reported to the telemetry lifecycle sink.
    /// Trap-era anchors, disasm-verified (July 2026 audit).
    round_end_set_win: u32,
    round_end_set_loss: u32,
    /// The timeout damage judge's result stores (win/loss/draw), same
    /// contract as `round_end_set_win`/`round_end_set_loss`.
    round_end_damage_judge_set_win: u32,
    round_end_damage_judge_set_loss: u32,
    round_end_damage_judge_set_draw: u32,

    /// This hooks the return from the function that runs a match — the
    /// trap-era anchor, restored: the battle mode's return into the
    /// comm dispatcher, reached when the game's own battle set is
    /// over. A tango match is the game's own set: mode 1 (triple
    /// battle) chains its battles inside battle mode —
    /// `round_start_ret` re-fires mid-set without this return running
    /// — and only the set-deciding battle returns through here; mode 0
    /// (single battle) returns after its one battle, which IS that
    /// mode's match. Fires once per set on each core, never during
    /// priming; KO-probe verified under both modes.
    match_end_ret: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    title_menu_control:     0x02010810,
    submenu_control:        0x0200F970,
    rng1_state:             0x02003D58,
    rng2_state:             0x02003F6C,
    unit:                   0x020394e0,
    chip_queues:            0x0203a0a0,
    custom_flags:           0x02033024,
    pvp_hand_blocks:        0x02033550,
};

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static BR4J_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0803061c,
        start_screen_title_transition:          0x08030714,
        intro_entry:                            0x08045b18,
        intro_exit:                             0x08045e60,
        title_pushstart_entry:                  0x08028de4,
        title_menu_confirm:                     0x08028e50,
        game_load_ret:                          0x08028f30,
        open_comm_menu:                         0x08043fba,
        comm_menu_init_ret:                     0x080440D2,//Routine different from BN4
        round_start_ret:                            0x08006B2E,
        round_end_set_win:                      0x080075d8,
        round_end_set_loss:                     0x080075ec,
        round_end_damage_judge_set_win:         0x08007882,
        round_end_damage_judge_set_loss:        0x08007896,
        round_end_damage_judge_set_draw:        0x0800789c,
        match_end_ret:                          0x08043fb6,
        battle_start_play_music_call:               0x0800796c,
    },
};

//! PvP-engine support: priming traps and telemetry polls.
//!
//! Nothing here touches the link protocol — the two games negotiate
//! for real over the emulated cable. Priming is PC-redirects into the
//! ROM's own transition code (boot fast-path: logo skip, title
//! continue, comm-menu open; comm menu: the top prompt's A-test
//! PC-skipped into its confirm, the bring-up redirected into the
//! game's battle-start walker), so every dispatcher/menu-state byte
//! is written by the game itself; telemetry re-reads the same battle
//! structs the trap engine reported, as pure per-tick polls.
//!
//! The comm-menu dispatcher's walk to a live link battle, mapped with
//! `PVP_PROBE_TRACE` + a memory-diff over each confirm. State lives in
//! `submenu_control`: [1] is the netbattle dispatcher state, [2]/[3]
//! nested sub-states.
//!   - ([1],[2]) = (04,04): the comm-menu top, confirm-ready once its
//!     open animation settles ([3] stays 00). The confirm handler does
//!     real netbattle session-init work (resets the comm mode byte,
//!     plays its sfx, starts the fade) and moves [1] to 0x08.
//!   - [1] = 08: link bring-up. [2] walks 04 → 08 → 14; each sub-state
//!     enters at [3] = 00 (animating in / waiting on the previous
//!     conversation exchange, ignores input), waits at [3] = 04 — the
//!     partner-search sync point ([2] = 04 holds here on the SIO
//!     status word's partner bit) — then sits at [3] = 08, a prompt,
//!     until its confirm closes it out ([3] = 0c pumps the exchange,
//!     or straight to [1] = 0c off the last one via the battle-start
//!     walker, `comm_menu_start_battle`). The prompts' cursors live in
//!     the control block ([2] = 04's at +0xe; the match-type menu's at
//!     +0x12, mapping 0/1/2 → [2] = 14/18/1c). NOTE: the partner bit
//!     is pre-session state — the SIO session itself is only
//!     configured at [1] = 0x0c's entry — fed on hardware by
//!     adapter/line probing the emulated cable doesn't model, so the
//!     bring-up can never advance in the lockstep pair and the walker
//!     skips it (see `comm_menu_bring_up_entry`).
//!   - [1] then runs 0x0c → 0x10 → 0x14 on its own. 0x10 is the
//!     settings state: once the link answers, each side runs the ROM
//!     settings generator off its OWN rngs (stage = rng % range,
//!     background = table[rng % 0x15] — a pure function, 0x81209dc in
//!     BR6E) and keeps the result at submenu_control+0x2a.
//!   - [1] = 0x14 is the ready prompt (`comm_menu_ready_entry`, the
//!     battle sub-handler): every tick it transmits its settings
//!     halfword (+0x2a → tx+2) and its prompt status (+0x26 → tx+8;
//!     4 = confirmed, 8 = cancel, 0x84 = confirmed-but-ineligible),
//!     and its completion — both slots' status reading 4 — copies rx
//!     slot 0's settings (the SIO multi id 0 core's, +2) into +0x2a on
//!     BOTH cores (0x812aa98 in BR6E) and walks to battle init itself.
//!     The master's generation wins over the wire; the slave's local
//!     draw is discarded. Priming is done at [1] = 0x18 (0x1c for
//!     match type 2) — a confirm past here would land in the opening
//!     chip select.
//!
//! (The walker itself lives in [`Pvp::primer_traps`].)

use tango_backend_mgba::Trap;
use tango_gamesupport_common::telemetry::LoadedChip;

pub struct Pvp {
    offsets: &'static Offsets,
}

pub static PVP_BR6E_00: Pvp = Pvp {
    offsets: &MEGAMAN6_FXXBR6E_00,
};
pub static PVP_BR5E_00: Pvp = Pvp {
    offsets: &MEGAMAN6_GXXBR5E_00,
};
pub static PVP_BR6J_00: Pvp = Pvp {
    offsets: &ROCKEXE6_RXXBR6J_00,
};
pub static PVP_BR5J_00: Pvp = Pvp {
    offsets: &ROCKEXE6_GXXBR5J_00,
};

impl Pvp {
    /// Raw submenu-control bytes, for headless probe diagnostics.
    pub fn debug_menu_state(&self, core: &mut mgba::core::Core) -> [u8; 8] {
        let mut buf = [0u8; 8];
        core.raw_read_range(self.offsets.ewram.submenu_control, -1, &mut buf);
        buf
    }

    /// Raw battle-state header bytes, for headless probe diagnostics.
    pub fn debug_battle_state(&self, core: &mut mgba::core::Core) -> [u8; 8] {
        let mut buf = [0u8; 8];
        core.raw_read_range(self.offsets.ewram.battle_state, -1, &mut buf);
        buf
    }

    /// Both players' current in-battle HP, for headless probe control
    /// (same read as the poller's).
    pub fn debug_battle_hp(&self, core: &mut mgba::core::Core) -> Option<[u16; 2]> {
        battle_units(&self.offsets.ewram, core).map(|units| units.map(|u| u.hp))
    }

    /// The game's own battle-tick counter, for headless probe liveness
    /// checks (telemetry doesn't report it).
    pub fn debug_battle_tick(&self, core: &mut mgba::core::Core) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
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
    fn sim_version(&self) -> u16 {
        0
    }

    fn primer_traps(
        &self,
        config: &tango_backend_mgba::PrimeConfig,
        player: usize,
        events: &tango_match::telemetry::EventSink,
        primed: &tango_backend_mgba::PrimedLatch,
    ) -> Vec<Trap> {
        use tango_match::telemetry::Outcome;

        // The boot fast-path (logo skip, title continue, comm-menu open)
        // is PC-redirects into the ROM's own transition code: the game
        // writes its own dispatcher state, plays its own sfx, does its
        // own bookkeeping. The comm-menu walk to battle init is the
        // game's own too: the top prompt's A-test is PC-skipped into its
        // confirm (the real netbattle session-init work), and the link
        // bring-up — whose partner search can never answer in the
        // lockstep pair (see `comm_menu_bring_up_entry`) — is redirected
        // into the game's battle-start walker, so the SIO session
        // config, the hello and the settings and ready exchanges all
        // run for real over the emulated cable. The trap engine's
        // settings-state dispatcher jump is deliberately ABSENT:
        // jumping [1] there skips [1] = 0x0c, where the session config
        // runs, and the games report "communication failed".
        let rom = &self.offsets.rom;
        let ewram = &self.offsets.ewram;
        let disable_bgm = config.disable_bgm;
        let submenu_control = ewram.submenu_control;
        // First-battle gate for the comm-menu traps below (see each
        // trap): the game's own battle tick counter is 0 from boot up to
        // the first battle's start and holds the last round's final
        // (nonzero) count through every post-battle menu, so `== 0` is
        // true exactly on the priming walk and never on the organic
        // post-battle re-entries (the players' own navigation runs the
        // same top prompt and can re-enter the bring-up — ungated, the
        // traps would hijack their rematch conversation into a forced
        // battle start).
        // Pure core RAM — rollback re-simulation evaluates it identically.
        let battle_tick = ewram.battle_state + 0x60;
        let match_type = config.match_type.0;
        let title_transition = rom.start_screen_title_transition;
        let title_continue = rom.title_menu_continue;
        let comm_open = rom.comm_menu_open;
        let start_battle = rom.comm_menu_start_battle;
        // RNG contract: seed both rngs per core once, at save load —
        // exactly the situation the vanilla protocol is built for (two
        // cartridges never share RNG state on real hardware). Each side
        // generates its own settings in the settings state, and the
        // ready prompt's REAL exchange (below) is what synchronizes
        // them: its completion adopts the multi-master's transmitted
        // settings halfword on both cores, and the players' draws
        // differ naturally from the distinct streams.
        let rng1 = config.core_rng_seed(player, 0);
        let rng2 = config.core_rng_seed(player, 1);
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
                // The start screen's logo handler (state 0), trapped at its
                // ENTRY — r5 = the applet's control block, loaded by its
                // dispatch function right before the jump-table call.
                // Redirect into the applet's own state-0x0c handler, the
                // logo's exit: a full `push {lr}`/`pop {pc}` function that,
                // fade-gated, writes state 0x10 (the title screen) itself.
                // It self-retries — while the fade isn't settled it just
                // returns to the dispatcher and this trap redirects again
                // next tick. State 0's logo/jingle init never runs; the
                // same skip the trap engine's state poke made, landed by
                // the ROM's own transition code.
                rom.start_screen_logo_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(title_transition);
                }),
            ),
            (
                // The title screen's init handler (title state 0), at its
                // terminal `pop {pc}` — immediately after SRAM is copied to
                // EWRAM and unmasked, and the handler has computed the
                // START-menu state from it (save-present flag, item count,
                // cursor defaulted to CONTINUE when a save exists). Instead
                // of popping into the title walk (press-START wait, menu,
                // fades), PC-redirect into the title's state-0x0c body —
                // the START-menu confirm-exit handler, one instruction past
                // its `push {lr}`: it switches the main mode to in-game and
                // dispatches the cursor — CONTINUE — through the game's own
                // save load. The init handler's saved lr feeds the target's
                // terminating `pop {pc}`, so control returns to the title
                // dispatcher cleanly.
                rom.start_screen_sram_unmask_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(title_continue);
                }),
            ),
            (
                // Inside the CONTINUE load: seed the rngs (see the contract
                // above). Pure data — the surrounding flow is the game's.
                rom.game_load_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_32(ewram.rng1_state, -1, rng1);
                    core.raw_write_32(ewram.rng2_state, -1, rng2);
                }),
            ),
            (
                // The CONTINUE exit's terminal `pop {pc}`, once the save
                // load and field init are done. PC-redirect into the game's
                // own open-the-comm-menu helper — the one its post-battle
                // return path uses — one instruction past its `push {lr}`:
                // it parks the field, switches the main mode to the submenu
                // runner, selects the comm applet and runs the comm menu's
                // init handler. The saved lr feeds the helper's `pop {pc}`.
                // The START menu never opens; the comm menu comes up the
                // way the game itself brings it back after a netbattle.
                rom.title_menu_load_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(comm_open);
                }),
            ),
            // ----- the comm-menu walk -----
            (
                // The comm-menu top (([1],[2]) = (04,04)): the A-gate ahead
                // of its confirm branch — `movs r0, #1; bl <pressed?>;
                // beq <idle>`, an 8-byte test the trap PC-skips (pc + 8;
                // no input is ever synthesized, the joypad is never
                // touched). Write the top cursor (+0xd; 0 is the netbattle
                // item, read by the confirm right after the skip), then
                // let the game's own confirm run: sfx, comm mode-byte
                // reset, dispatcher to the bring-up ([1] = 08) and the
                // fade — the real netbattle session-init work, including
                // the "session object init block" the old init-return poke
                // hand-wrote (a memory-diff capture of this confirm's fade
                // record and queued sfx). First battle only (see
                // `battle_tick`) — post-battle, the players' own
                // navigation runs this prompt.
                rom.comm_menu_top_confirm_gate,
                Box::new(move |core: &mut mgba::core::Core| {
                    if core.raw_read_32(battle_tick, -1) != 0 {
                        return;
                    }
                    core.raw_write_8(submenu_control + 0xd, -1, 0);
                    let pc = core.gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 8);
                }),
            ),
            (
                // The bring-up's first handler ([1] = 08, [2] = 00), trapped
                // at its ENTRY (r5 = the control block, kept live by the
                // dispatcher chain). The bring-up is the partner-search and
                // conversation UI — and it CANNOT be walked here: the SIO
                // session only gets configured at [1] = 0x0c's entry (the
                // 16-halfword multi setup + game-code hello), so in the
                // lockstep pair there is no pre-session SIO traffic and the
                // partner-present status bit its search waits on never
                // sets (on hardware it comes from the adapter/line probing
                // the emulated cable doesn't model). Instead, write the
                // match-type selection where the bring-up's menu would have
                // left it (+0x12 is that menu's cursor, +0x13 the next
                // prompt's; the settings/battle states read them from here
                // on), and redirect into the game's own battle-start
                // walker — the very function the bring-up's final prompt
                // confirm calls: it plays its sfx and walks the dispatcher
                // to [1] = 0x0c itself, where the session config and the
                // hello, settings and ready exchanges all run for real.
                // A full `push {lr}`/`pop {pc}` function entered at this
                // handler's own entry, so the dispatcher's lr survives
                // untouched. First battle only (see `battle_tick`) —
                // post-battle, re-confirming netbattle at the top
                // re-enters the bring-up for real.
                rom.comm_menu_bring_up_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    if core.raw_read_32(battle_tick, -1) != 0 {
                        return;
                    }
                    core.raw_write_8(submenu_control + 0x12, -1, match_type);
                    core.raw_write_8(submenu_control + 0x13, -1, 0);
                    core.gba_mut().cpu_mut().set_thumb_pc(start_battle);
                }),
            ),
            (
                // The ready prompt ([1] = 0x14, battle sub-handler): its
                // status halfword (+0x26 → transmitted at tx+8 every tick)
                // only ever changes on a keypress — 4 once the human
                // confirms. Hold it at "confirmed" so the prompt's own
                // exchange completes; both cores prime in lockstep, so the
                // synchronization the prompt exists for is vacuous. The
                // settings state before it and the completion after it run
                // untouched — the completion is what adopts the
                // multi-master's settings on both cores (see the module
                // doc) and walks the dispatcher to battle init itself.
                rom.comm_menu_ready_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_16(submenu_control + 0x26, -1, 4);
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
                // comm_menu_end_battle_entry, restored — the battle mode's
                // hand-back to the comm applet. A real tango match is the
                // game's OWN battle set (mode 1, triple: best-of-three
                // chained by the game itself; mode 0: one single battle),
                // and this function runs exactly when that set is over —
                // mid-set the game chains straight into the next battle
                // (`round_start_ret` re-fires) without coming back here.
                // Trapped on BOTH cores: whichever core's game leaves its
                // set first ends the match. The telemetry store dedups the
                // second core's firing.
                rom.comm_menu_end_battle_entry,
                {
                    let sink = events.clone();
                    Box::new(move |_core: &mut mgba::core::Core| sink.match_ended())
                },
            ),
        ]
    }

    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<mgba::core::Core>> {
        #[derive(Clone)]
        struct Poller {
            ewram: &'static EWRAMOffsets,
            player: usize,
            chips: tango_gamesupport_common::telemetry::HandChipTracker,
        }
        impl tango_match::telemetry::CorePoller<mgba::core::Core> for Poller {
            fn poll(
                &mut self,
                core: &mut mgba::core::Core,
                events: &tango_match::telemetry::EventSink,
                round: u32,
            ) -> Option<tango_match::telemetry::CoreObs> {
                let units = battle_units(self.ewram, core)?;
                // Whether this player is currently picking in the custom
                // screen. battle_state holds one flag byte per player at
                // +0x14/+0x15: 4 while that player's chip-select is open,
                // 0 once they confirm (or outside the custom screen
                // entirely). Derived empirically from the golden replays
                // -- the flags' episodes match the custom screens exactly
                // (first opening right after the battle intro, one
                // turn-counter increment at +0x07 per opening),
                // identically across US/JP and both perspectives.
                let custom_self = core.raw_read_8(self.ewram.battle_state + 0x14 + self.player as u32, -1) == 4;
                // This core's own player's chip fires, off its hand
                // block's fired counter (see `loaded_chips`).
                self.chips.tick(
                    round,
                    loaded_chips(self.ewram, core)[self.player],
                    custom_self,
                    units[self.player].hp,
                    self.player,
                    events,
                );
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
            chips: Default::default(),
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
/// Both players' loaded-chip readings (`None` when the hand is spent),
/// indexed by absolute player -- the id the player will use next, with
/// the block's fired counter: the hand-cursor contract
/// `HandChipTracker` detects fires on. See `EWRAMOffsets::chip_blocks`.
fn loaded_chips(ewram: &EWRAMOffsets, core: &mut mgba::core::Core) -> [Option<LoadedChip>; 2] {
    let mut chips = [None; 2];
    for player in 0..2u32 {
        let base = ewram.chip_blocks + player * 0x50;
        let fired = core.raw_read_16(base, -1) as u32;
        if fired > 5 {
            continue;
        }
        let id = core.raw_read_16(base + 2 + fired * 2, -1);
        if id != 0 && id <= 0x0fff {
            chips[player as usize] = Some(LoadedChip {
                id,
                fires: fired as u16,
            });
        }
    }
    chips
}

// ---------------------------------------------------------------------------
// Per-version EWRAM/ROM offsets.

#[derive(Clone, Copy)]
struct EWRAMOffsets {
    /// Location of the battle state struct in memory.
    battle_state: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    submenu_control: u32,

    /// Local RNG state. Doesn't need to be synced.
    rng1_state: u32,

    /// Shared RNG state. Must be synced.
    rng2_state: u32,

    /// The first in-battle unit's [`RawUnit`] record; the second
    /// follows immediately. This is the record the game itself hands
    /// around -- both slots' addresses sit in its own unit pointer
    /// table (0x02034900 on this version), which is how the base was
    /// pinned rather than guessed from a mid-struct anchor.
    unit: u32,
    /// Player 0's selected-chip block; player 1's is 0x50 beyond. Layout:
    /// +0 u16 chips fired since the last selection landed, +2 u16 ids[6]
    /// (0xFFFF = empty slot). The selection lands mid-pick (the ids are
    /// written while the custom screen is still open) and the loaded
    /// chip is ids[fired]. Indexed by absolute player, NOT by unit slot
    /// (the block stays with its player across the per-round slot swap).
    /// Supersedes the older per-slot cell at `hp + 6` (bn5 still reads its own), whose bare id
    /// couldn't show duplicate picks fired back-to-back — the fired
    /// counter can. Derived empirically from the golden replays.
    chip_blocks: u32,
}

#[derive(Clone, Copy)]
struct ROMOffsets {
    /// Entry of the start screen's logo handler (state 0 of the boot
    /// applet, main mode 0x10) — dispatched with r5 = its control
    /// block, from the applet's jump table, word 0. The walker
    /// redirects from here into `start_screen_title_transition`.
    start_screen_logo_entry: u32,

    /// The boot applet's state-0x0c handler (jump table word 3): the
    /// logo's own exit to the title screen — fade-gated `[r5] = 0x10;
    /// pop {pc}`, a full `push {lr}`/`pop {pc}` function that
    /// self-retries until the fade settles. Redirect target for
    /// `start_screen_logo_entry`.
    start_screen_title_transition: u32,

    /// Terminal `pop {pc}` of the title screen's init handler (title
    /// state 0, the title applet's jump table word 0), immediately
    /// after SRAM is copied to EWRAM and unmasked and the START-menu
    /// state is computed from it (save-present flag, cursor defaulted
    /// to CONTINUE). The walker redirects from here into
    /// `title_menu_continue`.
    start_screen_sram_unmask_ret: u32,

    /// One instruction past the `push {lr}` of the title's state-0x0c
    /// handler (jump table word 3) — the START-menu confirm exit: it
    /// switches the main mode to in-game and dispatches the menu
    /// cursor (CONTINUE, defaulted by the init handler when a save
    /// exists) through the game's own save load. Redirect target for
    /// `start_screen_sram_unmask_ret`; the redirecting handler's saved
    /// lr feeds this path's terminating `pop {pc}`.
    title_menu_continue: u32,

    /// This is immediately after game initialization is complete: that is, the internal state is set correctly.
    ///
    /// Fires inside the CONTINUE load; the walker seeds the rngs here.
    game_load_ret: u32,

    /// Terminal `pop {pc}` of the title's state-0x0c handler, after the
    /// CONTINUE load and field init completed. The walker redirects
    /// from here into `comm_menu_open`.
    title_menu_load_ret: u32,

    /// One instruction past the `push {lr}` of the game's own
    /// open-the-comm-menu helper — the routine its post-battle return
    /// path uses: parks the field, sets the main mode to the submenu
    /// runner, selects the comm applet (`[submenu_control] = 0x18`)
    /// and calls the comm menu's init handler. Redirect target for
    /// `title_menu_load_ret`. Located per version as the
    /// `movs r1, #0x18; strb r1, [r0]`-prefixed caller of the comm
    /// init handler.
    comm_menu_open: u32,

    /// The comm-menu top's A-gate: the `movs r0, #1` of the 8-byte
    /// `movs r0, #1; bl <pressed?>; beq <idle>` test in front of the
    /// (([1],[2]) = (04,04)) handler's confirm branch; the walker
    /// PC-skips the test (pc + 8), landing in the game's own netbattle
    /// confirm.
    comm_menu_top_confirm_gate: u32,

    /// Entry of the link bring-up's first sub-handler ([1] = 08,
    /// [2] = 00; the bring-up dispatcher's sub-table word 0). The
    /// walker redirects from here into `comm_menu_start_battle` — the
    /// partner-search/conversation UI it heads cannot run in the
    /// lockstep pair (see the trap).
    comm_menu_bring_up_entry: u32,

    /// The game's battle-start walker, a full `push {lr}`/`pop {pc}`
    /// function: plays the confirm sfx and walks the dispatcher to
    /// [1] = 0x0c ([2] = [3] = 0), whose entry does the real SIO
    /// session config and hello. It is the function the bring-up's
    /// final prompt confirm calls on A; redirect target for
    /// `comm_menu_bring_up_entry`.
    comm_menu_start_battle: u32,

    /// Entry of the ready prompt's battle sub-handler — dispatcher state
    /// [1] = 0x14 with [2] = 0, reached from the settings state's battle
    /// path (located per version via the dispatcher jump table entry 5's
    /// sub-handler table, word 0). Runs once per tick while the prompt
    /// is up; the walker's trap here holds the prompt status at
    /// "confirmed" so the prompt's real exchange completes.
    comm_menu_ready_entry: u32,

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

    /// This hooks the entrypoint to the function that is called when a
    /// match ends — the trap-era anchor, restored: the battle mode's
    /// hand-back to the comm applet, called from the dispatcher's
    /// battle state ([1] = 0x18) when the game's own battle set is
    /// over (it re-arms the conversation sub-machine at [3] = 0x10 and
    /// walks the applet on toward the menu). A tango match is the
    /// game's own set: mode 1 (triple battle) chains its battles
    /// inside battle mode — `round_start_ret` re-fires mid-set without
    /// this function running — and only the set-deciding battle exits
    /// through here; mode 0 (single battle) exits after its one
    /// battle, which IS that mode's match. Fires once per set on each
    /// core, never during priming; KO-probe verified under both modes.
    comm_menu_end_battle_entry: u32,
}

// US and JP EWRAM layouts agree on everything the engine still touches
// (the old boot-poke era needed the start-screen control block, whose
// address was the sole US/JP difference).
#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    battle_state:           0x02034880,
    submenu_control:        0x02009a30,
    rng1_state:             0x02001120,
    rng2_state:             0x020013f0,
    unit:                   0x0203a9b0,
    chip_blocks:            0x020349c0,
};

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static MEGAMAN6_FXXBR6E_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                    0x0803d1fc,
        start_screen_title_transition:              0x0803d298,
        start_screen_sram_unmask_ret:               0x0802f5ea,
        title_menu_continue:                        0x0802f758,
        game_load_ret:                              0x08004dde,
        title_menu_load_ret:                        0x0802f7e0,
        comm_menu_open:                             0x0811f75a,
        comm_menu_top_confirm_gate:                 0x08129388,
        comm_menu_bring_up_entry:                   0x081295d8,
        comm_menu_start_battle:                     0x0812b414,
        comm_menu_ready_entry:                      0x0812a8ec,
        round_start_ret:                            0x08007304,
        round_end_set_win:                          0x0800811e,
        round_end_set_loss:                         0x08008132,
        round_end_damage_judge_set_win:             0x080083c6,
        round_end_damage_judge_set_loss:            0x080083da,
        round_end_damage_judge_set_draw:            0x080083e0,
        comm_menu_end_battle_entry:                 0x0812b708,
        battle_start_play_music_call:               0x08009236,
    },
};

#[rustfmt::skip]
static MEGAMAN6_GXXBR5E_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                    0x0803d1d0,
        start_screen_title_transition:              0x0803d26c,
        start_screen_sram_unmask_ret:               0x0802f5ea,
        title_menu_continue:                        0x0802f758,
        game_load_ret:                              0x08004dde,
        title_menu_load_ret:                        0x0802f7e0,
        comm_menu_open:                             0x08121536,
        comm_menu_top_confirm_gate:                 0x0812b164,
        comm_menu_bring_up_entry:                   0x0812b3b4,
        comm_menu_start_battle:                     0x0812d1f0,
        comm_menu_ready_entry:                      0x0812c6c8,
        round_start_ret:                            0x08007304,
        round_end_set_win:                          0x0800811e,
        round_end_set_loss:                         0x08008132,
        round_end_damage_judge_set_win:             0x080083c6,
        round_end_damage_judge_set_loss:            0x080083da,
        round_end_damage_judge_set_draw:            0x080083e0,
        comm_menu_end_battle_entry:                 0x0812d4e4,
        battle_start_play_music_call:               0x08009236,
    },
};

#[rustfmt::skip]
static ROCKEXE6_RXXBR6J_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                    0x0803e26c,
        start_screen_title_transition:              0x0803e308,
        start_screen_sram_unmask_ret:               0x0803059a,
        title_menu_continue:                        0x08030708,
        game_load_ret:                              0x08004dc2,
        title_menu_load_ret:                        0x08030790,
        comm_menu_open:                             0x081275ae,
        comm_menu_top_confirm_gate:                 0x08131dac,
        comm_menu_bring_up_entry:                   0x08131fec,
        comm_menu_start_battle:                     0x08133e14,
        comm_menu_ready_entry:                      0x08133300,
        round_start_ret:                            0x080072f8,
        round_end_set_win:                          0x0800814e,
        round_end_set_loss:                         0x08008162,
        round_end_damage_judge_set_win:             0x080083f6,
        round_end_damage_judge_set_loss:            0x0800840a,
        round_end_damage_judge_set_draw:            0x08008410,
        comm_menu_end_battle_entry:                 0x08134108,
        battle_start_play_music_call:               0x08009406,
    },
};

#[rustfmt::skip]
static ROCKEXE6_GXXBR5J_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                    0x0803e240,
        start_screen_title_transition:              0x0803e2dc,
        start_screen_sram_unmask_ret:               0x0803059a,
        title_menu_continue:                        0x08030708,
        game_load_ret:                              0x08004dc2,
        title_menu_load_ret:                        0x08030790,
        comm_menu_open:                             0x08129376,
        comm_menu_top_confirm_gate:                 0x08133b74,
        comm_menu_bring_up_entry:                   0x08133db4,
        comm_menu_start_battle:                     0x08135bdc,
        comm_menu_ready_entry:                      0x081350c8,
        round_start_ret:                            0x080072f8,
        round_end_set_win:                          0x0800814e,
        round_end_set_loss:                         0x08008162,
        round_end_damage_judge_set_win:             0x080083f6,
        round_end_damage_judge_set_loss:            0x0800840a,
        round_end_damage_judge_set_draw:            0x08008410,
        comm_menu_end_battle_entry:                 0x08135ed0,
        battle_start_play_music_call:               0x08009406,
    },
};

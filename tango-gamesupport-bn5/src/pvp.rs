//! PvP-engine support: priming redirects and telemetry polls.
//!
//! Priming PC-redirects through the game's own boot and menu code:
//! logo → title (the fade-gated title transition), title → CONTINUE
//! (the START-press and A-confirm branches — the cursor's organic
//! default with a save present IS the CONTINUE row), overworld →
//! START menu (the field tick's own menu-open tail, with the Comm row
//! poked as the selection), then the comm menu's three A-press
//! confirm branches walk NetBattle → match type → stage set into the
//! settings state. Every dispatcher/menu-control byte is written by
//! ROM code; the primer only writes selection values the organic
//! confirms read (menu row, match-type cursor, the rule toggle) and
//! the rng seeds. bn5's settings aren't negotiated over the wire —
//! each side's ROM generator derives them from the rngs, so seeding
//! both rngs identically on both cores (from the negotiated match
//! seed) makes the two vanilla games agree without any exchange; rng1
//! (each player's own draw stream) then diverges per core at round
//! start. From init-battle on, the games run their real link protocol
//! over the emulated cable.

use tango_backend_mgba::Trap;
use tango_gamesupport_common::telemetry::LoadedChip;

pub struct Pvp {
    offsets: &'static Offsets,
}

pub static PVP_BRBE_00: Pvp = Pvp { offsets: &BRBE_00 };
pub static PVP_BRKE_00: Pvp = Pvp { offsets: &BRKE_00 };
pub static PVP_BRBJ_00: Pvp = Pvp { offsets: &BRBJ_00 };
pub static PVP_BRKJ_00: Pvp = Pvp { offsets: &BRKJ_00 };

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

    /// The game's own battle-tick counter, for headless probe liveness
    /// checks (telemetry doesn't report it).
    pub fn debug_battle_tick(&self, core: &mut mgba::core::Core) -> u32 {
        core.raw_read_32(self.offsets.ewram.battle_state + 0x60, -1)
    }

    /// Boot-path control blocks (start screen, title menu, START menu),
    /// for headless probe diagnostics.
    pub fn debug_boot_state(&self, core: &mut mgba::core::Core) -> ([u8; 2], [u8; 4], [u8; 6]) {
        let mut ss = [0u8; 2];
        core.raw_read_range(self.offsets.ewram.start_screen_control, -1, &mut ss);
        let mut tm = [0u8; 4];
        core.raw_read_range(self.offsets.ewram.title_menu_control, -1, &mut tm);
        let mut mc = [0u8; 6];
        core.raw_read_range(self.offsets.ewram.menu_control, -1, &mut mc);
        (ss, tm, mc)
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
        // RNG: seed both rngs per core once, at save load — exactly the
        // situation the vanilla protocol is built for (two cartridges
        // never share RNG state on real hardware). The settings state's
        // REAL exchange (below) is what synchronizes settings/stage, and
        // the players' draws differ naturally from the distinct streams.
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
                // The start screen's logo (state-0) handler entry, from the
                // start-screen jump table. r5 = start_screen_control, loaded
                // by the dispatch fn right before the table call. Instead of
                // running the logo, redirect to the full state-0xc handler
                // (`start_screen_title_transition`): fade-gated
                // `[r5]=0x10`, ending in its own `pop {pc}` — the handler's
                // saved lr on the dispatch stack feeds it, so control
                // returns to the dispatcher cleanly. While the fade isn't
                // ready it writes nothing; the state stays 0 and this entry
                // trap re-fires next tick, so it self-retries until the
                // transition lands — then state 0x10 opens the title screen.
                rom.start_screen_logo_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    let target = rom.start_screen_title_transition;
                    core.gba_mut().cpu_mut().set_thumb_pc(target);
                }),
            ),
            (
                // The title screen's PRESS START wait (title state 8, sub
                // 8) at its handler entry — r5 = title_menu_control, loaded
                // by the title dispatcher. Redirect past the handler's own
                // START-press check into the transition branch of the
                // game's START handler ([1]=0xc → the NEW GAME/CONTINUE
                // menu, sfx 0x67, title-anim teardown). The branch ends in
                // `pop {pc}`: since we entered before the handler's push,
                // that pop consumes the state-8 dispatcher's saved lr and
                // returns straight to the outer title dispatcher —
                // balanced; only that tick's OAM flush is skipped.
                rom.title_start_wait_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    let target = rom.title_start_press_branch;
                    core.gba_mut().cpu_mut().set_thumb_pc(target);
                }),
            ),
            (
                // The NEW GAME/CONTINUE menu's per-tick handler (title
                // state 8, sub 0xc) at its terminal `pop {pc}`. Instead of
                // popping, redirect into the A|START confirm branch inside
                // the menu's input handler, one past its `push {lr}`:
                // fade-gated, then the game's own confirm (sfx 0x9d,
                // [1]=0x10, fade out) — sub 0x10 then walks to the title
                // confirm dispatch, which loads the save because the cursor
                // sits on CONTINUE, the game's own default whenever a save
                // exists (title init picks [8]=1). The branch ends in the
                // input handler's `pop {pc}`, fed by this handler's saved
                // lr — control returns to the sub dispatcher. While the
                // fade isn't ready it writes nothing and this pop trap
                // re-fires next tick.
                rom.title_menu_wait_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    let target = rom.title_menu_continue_branch;
                    core.gba_mut().cpu_mut().set_thumb_pc(target);
                }),
            ),
            (
                rom.game_load_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    // Seed the rngs (see the contract above). Data, not
                    // dispatcher state — everything else on the way to
                    // battle is now PC-redirects through the game's own
                    // menu code.
                    core.raw_write_32(ewram.rng1_state, -1, rng1);
                    core.raw_write_32(ewram.rng2_state, -1, rng2);
                }),
            ),
            (
                // The field tick's START-menu opener: the overworld's field
                // handler checks, in order, player state == 4 (standing),
                // the story-lock flags, menu-not-already-open
                // (menu_control[5] bit 0) and fade-ready before it even
                // looks at the START key. This site is the instruction
                // right after the fade check passes, so every organic
                // precondition holds; redirect past the key checks into the
                // same function's open tail (sfx 0x79, the menu-open call —
                // menu_control memset + [5]|=1 + slide-in — and player
                // state 0x1c). Same function, so the stack is untouched.
                // Once the menu is open the [5]-bit check upstream keeps
                // this site unreachable — it fires exactly once.
                rom.field_menu_gate,
                Box::new(move |core: &mut mgba::core::Core| {
                    // First battle only (see the comm traps below): pure
                    // core RAM, rollback-stable.
                    if core.raw_read_32(ewram.battle_state + 0x60, -1) != 0 {
                        return;
                    }
                    let target = rom.field_menu_open_branch;
                    core.gba_mut().cpu_mut().set_thumb_pc(target);
                }),
            ),
            (
                // The START menu's row-select handler at the site just past
                // its own fade-ready and input-delay gates (state [0]=4,
                // before the key checks). Poke the Comm row — the selection
                // a human would have made, read by the confirm right after
                // — then redirect into the same function's A-confirm branch
                // (sfx 0x81, [0]=0x10, [1]=4). The overworld tick watches
                // [1]==4 and launches the comm applet itself: screen-block
                // memset + submenu_control[0]=row*4 (=0x18, the comm
                // screen's identity byte the old poke wrote by hand). Same
                // function, stack untouched; after the confirm [0]!=4 so
                // the row-select handler never runs again.
                rom.start_menu_select_site,
                Box::new(move |core: &mut mgba::core::Core| {
                    // First battle only, as above.
                    if core.raw_read_32(ewram.battle_state + 0x60, -1) != 0 {
                        return;
                    }
                    core.raw_write_8(ewram.menu_control + 0x4, -1, 0x06);
                    let target = rom.start_menu_confirm_branch;
                    core.gba_mut().cpu_mut().set_thumb_pc(target);
                }),
            ),
            // ----- the comm menu: three A-presses, three redirects -----
            //
            // The comm applet inits organically ([1]=4, [2]=0 → row build →
            // [2]=4, the "press A" prompt). From there the games' own
            // confirm chain runs via the two traps below; the settings
            // handler then does its real ~77-tick negotiation with the peer
            // over the emulated cable, exactly as the organic menu flow
            // does, and the ROM generator writes submenu_control
            // [0x16]/[0x17] itself.
            //
            // Both traps gate on first-battle: the comm menu is re-inited
            // organically after EVERY battle on the way to the game's own
            // "battle again?" conversation; redirecting there would hijack
            // that conversation into a forced re-bring-up and the match
            // could never end. The game's own battle tick counter is 0 from
            // boot up to the first battle's start and holds the last
            // round's final (nonzero) count through every post-battle menu.
            // Pure core RAM — rollback re-simulation evaluates it
            // identically. They also gate on the dispatcher byte [2] they
            // are about to advance, so each redirect fires exactly once
            // (the redirected branch itself moves [2], and the trap site is
            // hit again at the shared pop).
            (
                // The comm prompt state's terminal `pop {pc}` ([2]=4: "press
                // A to open the menu"). Redirect to the A branch inside the
                // same handler: [2]=8 (nav), depth [0x14]=0 — it rejoins
                // this same pop, where the [2] gate ends the loop.
                rom.comm_menu_prompt_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    if core.raw_read_32(ewram.battle_state + 0x60, -1) != 0 {
                        return;
                    }
                    if core.raw_read_8(ewram.submenu_control + 0x2, -1) != 4 {
                        return;
                    }
                    let target = rom.comm_menu_prompt_press_branch;
                    core.gba_mut().cpu_mut().set_thumb_pc(target);
                }),
            ),
            (
                // The comm nav state's terminal `pop {r4, pc}` ([2]=8). The
                // menu is three levels deep — root (NetBattle/…, cursor
                // [0x2a]), match type (single/triple, cursor [0x34]), stage
                // set (normal/extended, cursor [0x3e]) — and one A-press
                // confirm serves all of them: [0x15]=0, sfx 0x81, then the
                // cursor switchboard. At depth 0/1 it descends ([2]=0xc →
                // 0x10 → 8, zeroing the new level's cursor pair — the
                // organic source of the [0x3e]=0 the old poke wrote); at
                // depth 2 it writes [3] = map[[0x34]][[0x3e]] (the map rows
                // are {0,1}/{2,3}, so row 0 under cursor match_type IS the
                // old poke's [3]=match_type*2) plus the terminal marker,
                // and the handler advances to the settings state ([2]=0x14).
                // The redirect target is one past the handler's own push on
                // a path that rejoins this same pop — balanced, and the [2]
                // gate ends each loop. Root cursor 0 (NetBattle) is the
                // memset default; the only selections a human would still
                // have made are the match-type cursor and the depth-2 rule
                // toggle [0x1c] (left/right, 0..1; read at battle init —
                // the trap engine always chose 1), poked right before the
                // depth-2 confirm that reads them.
                rom.comm_menu_nav_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    if core.raw_read_32(ewram.battle_state + 0x60, -1) != 0 {
                        return;
                    }
                    if core.raw_read_8(ewram.submenu_control + 0x2, -1) != 8 {
                        return;
                    }
                    if core.raw_read_8(ewram.submenu_control + 0x14, -1) == 2 {
                        core.raw_write_16(ewram.submenu_control + 0x34, -1, match_type as u16);
                        core.raw_write_8(ewram.submenu_control + 0x1c, -1, 0x01);
                    }
                    let target = rom.comm_menu_nav_confirm_branch;
                    core.gba_mut().cpu_mut().set_thumb_pc(target);
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
                // comm_menu_end_battle_entry, restored — the comm
                // dispatcher's end-battle state, entered when the game's
                // OWN battle set is over (mode 1, triple: best-of-three
                // chained by the game itself; mode 0: one single battle).
                // Mid-set the game chains straight into the next battle
                // (`round_start_ret` re-fires) without entering it.
                // Trapped on BOTH cores: whichever core's game leaves its
                // set first ends the match. The telemetry store dedups the
                // second core's firing (and the state's per-tick re-entry).
                rom.comm_menu_end_battle_entry,
                {
                    let sink = events.clone();
                    Box::new(move |_core: &mut mgba::core::Core| sink.match_ended())
                },
            ),
        ]
    }

    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<mgba::core::Core>> {
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
                // screen. Same battle_state layout as bn6: one flag byte
                // per player at +0x14/+0x15, 4 while that player's
                // chip-select is open, 0 once they confirm (or outside
                // the custom screen entirely). Verified against the
                // golden replays -- identical episode pattern to bn6's.
                let custom_self =
                    core.raw_read_8(self.ewram.battle_state + 0x14 + self.player as u32, -1) == 4;
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
            fn save(&self) -> tango_match::telemetry::Scratch {
                tango_match::telemetry::Scratch::new(self.chips.clone())
            }
            fn restore(&mut self, scratch: &tango_match::telemetry::Scratch) {
                self.chips = scratch.get().cloned().unwrap_or_default();
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
    _reserved_28: [u8; 0x2],
    /// The loaded chip's id, 0xFFFF when none -- the same per-slot cell
    /// bn6 keeps at this offset. Superseded for chip telemetry by the
    /// hand block (`EWRAMOffsets::chip_blocks`), whose fired counter
    /// shows the duplicate picks this bare id can't.
    chip: u16,
    _reserved_2c: [u8; 0xac],
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
/// `HandChipTracker` detects fires on. Same shape as bn4/bn6's blocks.
/// See `EWRAMOffsets::chip_blocks`.
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

    /// Start screen jump table control.
    start_screen_control: u32,

    /// Title menu jump table control.
    title_menu_control: u32,

    /// START menu jump table control.
    menu_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    submenu_control: u32,

    /// Local RNG state. Doesn't need to be synced.
    rng1_state: u32,

    /// Shared RNG state. Must be synced.
    rng2_state: u32,

    /// Player 0's selected-chip block; player 1's is 0x50 beyond. Same
    /// shape as bn4/bn6's: +0 u16 chips fired since the last selection
    /// landed, +2 u16 ids[6] (0xFFFF = empty slot); the loaded chip is
    /// ids[fired], agreeing with the per-slot cell (`RawUnit::chip`) at
    /// every live tick. Indexed by absolute player, NOT by unit slot.
    /// Found July 2026 by whole-EWRAM elimination scan against the cell
    /// over wiggle-driven battles (hand_probe recipe), zero mismatches
    /// across two builds (BRBJ, BRKE) including multi-chip hands with
    /// the cell advancing through ids[] as fired stepped.
    chip_blocks: u32,

    /// The first in-battle unit's [`RawUnit`] record; the second
    /// follows immediately. This is the record the game itself hands
    /// around -- both slots' addresses sit in its own unit pointer
    /// table (0x02034b10 on this version), which is how the base was
    /// pinned rather than guessed from a mid-struct anchor.
    unit: u32,
}

#[derive(Clone, Copy)]
struct ROMOffsets {
    /// Entry of the start screen's logo (state-0) handler, from the
    /// start-screen jump table (r5 = start_screen_control is loaded by
    /// the dispatch function right before the table call). Trapped to
    /// redirect into `start_screen_title_transition`.
    start_screen_logo_entry: u32,

    /// The start screen's full state-0xc handler: `push {lr}`, then a
    /// fade-gated `[r5]=0x10` (the title transition), `pop {pc}`.
    /// `start_screen_logo_entry`'s trap redirects here.
    start_screen_title_transition: u32,

    /// Entry of the title screen's PRESS START wait handler (title
    /// state 8, sub 8), from the state-8 sub jump table (r5 =
    /// title_menu_control). Trapped to redirect into
    /// `title_start_press_branch`.
    title_start_wait_entry: u32,

    /// The transition branch of the title screen's START handler, past
    /// its newly-pressed-START check: [1]=0xc (the NEW GAME/CONTINUE
    /// menu), sfx 0x67, title-anim teardown, `pop {pc}`.
    /// `title_start_wait_entry`'s trap redirects here.
    title_start_press_branch: u32,

    /// Terminal `pop {pc}` of the NEW GAME/CONTINUE menu's per-tick
    /// handler (title state 8, sub 0xc). Trapped to redirect into
    /// `title_menu_continue_branch`.
    title_menu_wait_ret: u32,

    /// The A|START confirm branch inside the title menu's input
    /// handler, one past its `push {lr}`: fade-gated, sfx 0x9d,
    /// [1]=0x10 (post-confirm fade-out), `pop {pc}`. With a save
    /// present the cursor's organic default is the CONTINUE row, so
    /// the title confirm dispatch loads the save.
    /// `title_menu_wait_ret`'s trap redirects here.
    title_menu_continue_branch: u32,

    /// This is immediately after game initialization is complete: that is, the internal state is set correctly.
    ///
    /// At this point, the rng seeds are written.
    game_load_ret: u32,

    /// The overworld field tick's START-menu opener, at the
    /// instruction right after its fade-ready check passes (player
    /// state == 4, story locks clear and menu-not-open have already
    /// been checked upstream; only the key checks remain). Trapped to
    /// redirect into `field_menu_open_branch`.
    field_menu_gate: u32,

    /// The same function's menu-open tail: sfx 0x79, the menu-open
    /// call (menu_control memset, [5]|=1, slide-in) and player state
    /// 0x1c. `field_menu_gate`'s trap redirects here.
    field_menu_open_branch: u32,

    /// The START menu's row-select handler (menu_control state
    /// [0]=4), at the site just past its fade-ready and input-delay
    /// gates, before the key checks. Trapped to poke the Comm row
    /// ([4]=6) and redirect into `start_menu_confirm_branch`.
    start_menu_select_site: u32,

    /// The same handler's A-confirm branch: sfx 0x81, [0]=0x10,
    /// [1]=4 — the overworld tick watches [1]==4 and launches the
    /// selected screen's applet itself. `start_menu_select_site`'s
    /// trap redirects here.
    start_menu_confirm_branch: u32,

    /// Terminal `pop {pc}` of the comm menu's prompt state ([2]=4,
    /// "press A to open the menu"). Trapped to redirect into
    /// `comm_menu_prompt_press_branch`.
    comm_menu_prompt_ret: u32,

    /// The prompt handler's A branch: [2]=8 (nav), depth [0x14]=0,
    /// rejoining the handler's own pop. `comm_menu_prompt_ret`'s trap
    /// redirects here.
    comm_menu_prompt_press_branch: u32,

    /// Terminal `pop {r4, pc}` of the comm menu's nav state ([2]=8).
    /// Trapped to redirect into `comm_menu_nav_confirm_branch`, once
    /// per menu level.
    comm_menu_nav_ret: u32,

    /// The nav handler's A-confirm path, one past its `push {r4, lr}`:
    /// [0x15]=0, sfx 0x81, the cursor switchboard (descend, or at
    /// depth 2 the terminal confirm into the settings state
    /// [2]=0x14), rejoining the handler's own pop.
    /// `comm_menu_nav_ret`'s trap redirects here.
    comm_menu_nav_confirm_branch: u32,

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
    /// match ends — the trap-era anchor, restored: the comm
    /// dispatcher's end-battle state (outer jump-table entry 4,
    /// [1] = 0x10), entered when the game's own battle set is over. A
    /// tango match is the game's own set: mode 1 (triple battle)
    /// chains its battles inside battle mode — `round_start_ret`
    /// re-fires mid-set without this state running — and only the
    /// set-deciding battle exits through it; mode 0 (single battle)
    /// exits after its one battle, which IS that mode's match. A state
    /// entry runs per tick while the state holds, so it may re-fire
    /// for a few ticks — the telemetry store dedups. Never fires
    /// during priming; KO-probe verified under both modes.
    comm_menu_end_battle_entry: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    battle_state:           0x02034a90,
    start_screen_control:   0x02013000,
    title_menu_control:     0x0200b980,
    menu_control:           0x0200e950,
    submenu_control:        0x0200ab20,
    rng1_state:             0x02001c94,
    rng2_state:             0x02001d40,
    unit:                   0x0203b200,
    chip_blocks:            0x02034e20,
};

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static BRBE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0803c4c0,
        start_screen_title_transition:          0x0803c5f8,
        title_start_wait_entry:                 0x08030136,
        title_start_press_branch:               0x080302b2,
        title_menu_wait_ret:                    0x080301aa,
        title_menu_continue_branch:             0x080302e6,
        game_load_ret:                          0x08004a74,
        field_menu_gate:                        0x080054b6,
        field_menu_open_branch:                 0x080054d6,
        start_menu_select_site:                 0x0812864c,
        start_menu_confirm_branch:              0x08128688,
        comm_menu_prompt_ret:                   0x08134d0a,
        comm_menu_prompt_press_branch:          0x08134cde,
        comm_menu_nav_ret:                      0x08134e04,
        comm_menu_nav_confirm_branch:           0x08134d2e,
        round_start_ret:                            0x0800673e,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        comm_menu_end_battle_entry:             0x08134b50,
        battle_start_play_music_call:               0x08007e1a,
    },
};

#[rustfmt::skip]
static BRKE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0803c4c4,
        start_screen_title_transition:          0x0803c5fc,
        title_start_wait_entry:                 0x0803013a,
        title_start_press_branch:               0x080302b6,
        title_menu_wait_ret:                    0x080301ae,
        title_menu_continue_branch:             0x080302ea,
        game_load_ret:                          0x08004a74,
        field_menu_gate:                        0x080054b6,
        field_menu_open_branch:                 0x080054d6,
        start_menu_select_site:                 0x08128734,
        start_menu_confirm_branch:              0x08128770,
        comm_menu_prompt_ret:                   0x08134df2,
        comm_menu_prompt_press_branch:          0x08134dc6,
        comm_menu_nav_ret:                      0x08134eec,
        comm_menu_nav_confirm_branch:           0x08134e16,
        round_start_ret:                            0x0800673e,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        comm_menu_end_battle_entry:             0x08134c38,
        battle_start_play_music_call:               0x08007e1a,
    },
};

#[rustfmt::skip]
static BRBJ_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0803c424,
        start_screen_title_transition:          0x0803c50c,
        title_start_wait_entry:                 0x080300d2,
        title_start_press_branch:               0x0803024e,
        title_menu_wait_ret:                    0x08030146,
        title_menu_continue_branch:             0x08030282,
        game_load_ret:                          0x08004a74,
        field_menu_gate:                        0x080054b6,
        field_menu_open_branch:                 0x080054d6,
        start_menu_select_site:                 0x08128258,
        start_menu_confirm_branch:              0x08128294,
        comm_menu_prompt_ret:                   0x081348c2,
        comm_menu_prompt_press_branch:          0x08134896,
        comm_menu_nav_ret:                      0x081349bc,
        comm_menu_nav_confirm_branch:           0x081348e6,
        round_start_ret:                            0x0800673e,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        comm_menu_end_battle_entry:             0x08134708,
        battle_start_play_music_call:               0x08007e1a,
    },
};

#[rustfmt::skip]
static BRKJ_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0803c428,
        start_screen_title_transition:          0x0803c510,
        title_start_wait_entry:                 0x080300d6,
        title_start_press_branch:               0x08030252,
        title_menu_wait_ret:                    0x0803014a,
        title_menu_continue_branch:             0x08030286,
        game_load_ret:                          0x08004a74,
        field_menu_gate:                        0x080054b6,
        field_menu_open_branch:                 0x080054d6,
        start_menu_select_site:                 0x08128340,
        start_menu_confirm_branch:              0x0812837c,
        comm_menu_prompt_ret:                   0x081349aa,
        comm_menu_prompt_press_branch:          0x0813497e,
        comm_menu_nav_ret:                      0x08134aa4,
        comm_menu_nav_confirm_branch:           0x081349ce,
        round_start_ret:                            0x0800673e,
        round_end_set_win:                      0x08007474,
        round_end_set_loss:                     0x08007488,
        round_end_damage_judge_set_win:         0x080076f6,
        round_end_damage_judge_set_loss:        0x0800770a,
        round_end_damage_judge_set_draw:        0x08007710,
        comm_menu_end_battle_entry:             0x081347f0,
        battle_start_play_music_call:               0x08007e1a,
    },
};

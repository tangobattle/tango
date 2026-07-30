//! PvP-engine support: priming pokes and telemetry polls.
//!
//! bn1's comm menu needs no settings surgery at all: the primer
//! PC-redirects the comm menu's init return into the comm switchboard's
//! netbattle-confirm branch — the game itself resets the packet
//! buffers, broadcasts its hello marker and walks the dispatcher into
//! the link-battle bring-up — and the games' own exchange runs for real
//! over the emulated cable from there.
//!
//! RNG/stage model: the rng is seeded per core once, at save load —
//! two real cartridges never share RNG state, and nothing in bn1's
//! protocol needs them to (each player's chip luck is their own). The
//! stage is not rng at all: each console picks it as
//! `frames_since_boot % 12` in the battle-start routine, so that
//! counter is overwritten with one SHARED match-seed value on both
//! cores at round start — the picks agree, and vary per match
//! (organically the counter is a constant of the deterministic
//! priming walk).

use tango_backend_mgba::Trap;

pub struct Pvp {
    offsets: &'static Offsets,
}

pub static PVP_AREE_00: Pvp = Pvp { offsets: &AREE_00 };
pub static PVP_AREJ_00: Pvp = Pvp { offsets: &AREJ_00 };

impl Pvp {
    /// Raw submenu-control bytes, for headless probe diagnostics.
    pub fn debug_menu_state(&self, core: &mut mgba::core::Core) -> [u8; 8] {
        let mut buf = [0u8; 8];
        core.raw_read_range(self.offsets.ewram.submenu_control, -1, &mut buf);
        buf
    }

    /// Both players' current in-battle HP, for headless probe control
    /// (same read as the poller's).
    pub fn debug_battle_hp(&self, core: &mut mgba::core::Core) -> Option<[u16; 2]> {
        battle_units(&self.offsets.ewram, core).map(|units| units.map(|u| u.hp))
    }

    /// Poke `player`'s current in-battle HP — headless KO probes only,
    /// never shipped flows. Finds the unit slot owned by `player` this
    /// round and writes its current-HP halfword.
    pub fn debug_set_hp(&self, core: &mut mgba::core::Core, player: u8, hp: u16) {
        let ewram = &self.offsets.ewram;
        for slot in 0..2u32 {
            if read_unit(ewram, core, slot).owner == player {
                core.raw_write_16(unit_field(ewram, slot, std::mem::offset_of!(RawUnit, hp)), -1, hp);
            }
        }
    }
}

impl tango_backend_mgba::GameSupport for Pvp {
    fn primer_traps(
        &self,
        config: &tango_backend_mgba::PrimeConfig,
        player: usize,
        lifecycle: &tango_match::telemetry::LifecycleSink,
        primed: &tango_backend_mgba::PrimedLatch,
    ) -> Vec<Trap> {
        use tango_match::telemetry::Outcome;

        let rom = &self.offsets.rom;
        let ewram = &self.offsets.ewram;
        let disable_bgm = config.disable_bgm;
        let rng = config.core_rng_seed(player, 0);
        let fade_to_title = rom.start_screen_fade_to_title;
        let title_confirm = rom.title_confirm_continue;
        let open_start_menu = rom.overworld_open_start_menu;
        let start_netbattle = rom.comm_menu_start_netbattle;
        // The stage pick, synced per match (see module docs — the stage
        // is a frame counter, not rng).
        let stage_counter = config.core_rng_seed(0, 1) as u16;
        // Lifecycle signals are host-side only — core state is untouched,
        // so the simulation is unaffected. Rounds are reported from core 0
        // (whose local player is player 0); core 1's lifecycle traps stay
        // silent. Match end is the exception, reported from both cores —
        // see its trap below.
        let sink = (player == 0).then(|| lifecycle.clone());
        let primed = primed.clone();
        // The game's own round-result judgment: set_win/set_loss are
        // where THIS core's game records its local player's result,
        // ~100 ticks before the battle-mode exit (KO-probe-verified
        // to fire on the real protocol route, at the KO). Core 0's
        // local player is player 0, so set_win = P0Win.
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
        // A sound-request thunk entry: while this core is still priming,
        // return straight to the caller so the request never reaches the
        // driver (see the trap comment below).
        let silence_while_priming = {
            let primed = primed.clone();
            move |addr: u32| -> Trap {
                let primed = primed.clone();
                (
                    addr,
                    Box::new(move |core: &mut mgba::core::Core| {
                        if primed.is_set() {
                            return;
                        }
                        let lr = core.gba().cpu().gpr(14) as u32 & !1;
                        core.gba_mut().cpu_mut().set_thumb_pc(lr);
                    }),
                )
            }
        };

        vec![
            // ----- the silent walk -----
            // The walked menus run the game's own confirm/dialog code,
            // which requests sounds like any organic pass: the title
            // confirm jingle, the netbattle confirm, the connecting
            // dialog's per-character text blips (22 in one tick — the
            // driver drains them one by one), and the link bring-up's
            // connect jingle ~8 ticks before battle start. Priming runs
            // far faster than real time, so the host clears the piled-up
            // sample buffers when it ends — but that drops rendered
            // samples, not driver state, and the late requests keep
            // sounding over the session open. Gate the driver's two
            // request thunks (sfx and music, adjacent functions) while
            // this core is unprimed instead, so the walk queues nothing.
            // Purely local presentation — the sound driver's state never
            // feeds battle logic, so peers may disagree. Inert once
            // primed (probe-verified: the battle-start BGM request lands
            // AFTER the priming handoff in every family, so it and all
            // in-battle sounds run normally), and rollback re-simulation
            // always sees the latch set.
            silence_while_priming(rom.play_sfx_entry),
            silence_while_priming(rom.play_music_entry),
            // ----- the boot fast-path -----
            (
                // The logo (start-screen state-0) handler's entry, reached
                // through the start screen's jump table — the dispatcher
                // has already loaded r5 = the applet control block and lr
                // = its own return. Instead of the logo bring-up,
                // PC-redirect to the start screen's OWN state-0x0c handler
                // (a full push{lr}..pop{pc} function): its body is the
                // fade-gated `[r5] = 0x10` transition to the title state —
                // the exact write the logo flow lands after its input-skip
                // fade-out. While the fade gate holds the state stays 0
                // and this trap re-fires next tick, so the transition
                // self-retries; at cold boot no fade is active and it
                // lands on the first try.
                rom.start_screen_logo_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(fade_to_title);
                }),
            ),
            (
                rom.start_screen_play_music_call,
                Box::new(move |core: &mut mgba::core::Core| {
                    let pc = core.gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                }),
            ),
            (
                // The title-wait substate handler's terminal `pop {pc}` —
                // the state that polls for START each tick (the previous
                // substate armed the attract timer; the title init's own
                // SRAM checksum checks set the NEW GAME/CONTINUE cursor,
                // = CONTINUE exactly when the save is valid). Instead of
                // popping, PC-redirect into the title input helper's
                // confirm branch — the code an A/START press runs: stops
                // the music, plays the confirm sfx and walks the substate
                // into the fade-gated exit, which routes (attract timer
                // still >0) to the load state; that state reads the
                // organic cursor and calls the game's own save load. The
                // branch is past the helper's `push {lr}` and ends at its
                // `pop {pc}`, so the wait handler's saved lr feeds the pop
                // and control returns to the dispatcher cleanly. Fires
                // once: the first redirect leaves the wait substate.
                rom.title_wait_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(title_confirm);
                }),
            ),
            (
                rom.game_load_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    // Seed the rng per core once, at save load (see module
                    // docs).
                    core.raw_write_32(ewram.rng_state, -1, rng);
                    // The load fn is also called once from boot-time global
                    // init (subsystem still 0 then); only the title load
                    // state's call — the CONTINUE flow, subsystem = 4,
                    // written by that state just before the call — opens
                    // the menu.
                    if core.raw_read_8(ewram.subsystem_control, -1) != 0x04 {
                        return;
                    }
                    // Write the START menu's remembered-tab byte to the
                    // comm menu — the selection the player would have made
                    // — then, instead of popping, PC-redirect into the
                    // overworld's own START-menu-open branch (state 0x1c,
                    // past its fade gate): it does the game's menu-open
                    // bookkeeping, zero-fills and re-inits the submenu
                    // block from the tab byte (submenu id = tab * 4 = the
                    // comm menu), sets subsystem control to the START menu
                    // and requests the menu sound — everything the old
                    // poke wrote by hand. The branch's push/pop pairs are
                    // balanced and it ends in `pop {pc}`, so the load fn's
                    // own saved lr feeds the pop and control returns to
                    // the title load state cleanly; r5 even holds the same
                    // game-progress block the branch's home handler would
                    // have.
                    core.raw_write_8(ewram.start_menu_tab, -1, 0x05);
                    core.gba_mut().cpu_mut().set_thumb_pc(open_start_menu);
                }),
            ),
            (
                // The comm menu's init return — the init handler's terminal
                // `pop {pc}`. Instead of popping, PC-redirect into the comm
                // switchboard's netbattle branch: the game's own confirm
                // code resets the tx/rx buffers, broadcasts its hello
                // marker into the tx packet and walks the dispatcher to
                // the link-battle bring-up state — everything the old poke
                // wrote by hand. The branch target is past the
                // switchboard's `push {lr}`, so the init handler's own
                // saved lr feeds the branch's terminating `pop {pc}` and
                // control returns to the dispatcher cleanly.
                rom.comm_menu_init_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    // From-field opens only: the game's own menu init
                    // writes an entry flag at submenu byte [3] — 0 when
                    // the field opens the menu (our primed route), 1 when
                    // the battle exit re-opens it. The comm menu now
                    // returns organically after the battle (the remembered
                    // tab holds the comm menu for the whole link session),
                    // and redirecting that re-init would hijack the
                    // post-battle menu into a forced re-battle. Pure core
                    // RAM — rollback re-simulation evaluates it
                    // identically.
                    if core.raw_read_8(ewram.submenu_control + 0x3, -1) != 0 {
                        return;
                    }
                    core.gba_mut().cpu_mut().set_thumb_pc(start_netbattle);
                }),
            ),
            (
                // Battle-start prologue, once per round: the stage-pick
                // sync (see `stage_counter` above).
                rom.round_start_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_16(ewram.frame_counter, -1, stage_counter);
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
            (
                // The battle-mode exit. bn1 matches are a single battle (no
                // rematch conversation), so the game leaving its battle loop
                // IS the match end — KO-probe-verified to fire on the real
                // route, right as the dispatcher returns to the comm menu.
                // Trapped on BOTH cores: each game exits the link session
                // through its own path, and on a one-sided decline only the
                // decliner's game exits (the other waits at its menu for a
                // peer that isn't coming back) — whichever core leaves
                // first ends the match. The telemetry store dedups the
                // second core's firing on a mutual exit.
                rom.match_end_ret,
                {
                    let sink = lifecycle.clone();
                    Box::new(move |_core: &mut mgba::core::Core| sink.match_ended())
                },
            ),
        ]
    }

    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<mgba::core::Core>> {
        let ewram = &self.offsets.ewram;
        Box::new(move |core: &mut mgba::core::Core| {
            let units = battle_units(ewram, core)?;
            // Only this core's own player gets a chip: bn1 keeps the
            // record per console (see `loaded_chip`), and the merge
            // takes each player's from that player's core.
            let chip = loaded_chip(ewram, core);
            Some(tango_match::telemetry::CoreObs {
                units: std::array::from_fn(|p| tango_match::telemetry::UnitObs {
                    hp: units[p].hp,
                    tile: (units[p].tile[0], units[p].tile[1]),
                    chip: (p == player).then_some(chip).flatten(),
                }),
                // bn1's custom flag is local-player semantics: the
                // battle-mode state entry holds this one handler value
                // exactly while this side's chip select is open (see
                // `EWRAMOffsets::custom_state`).
                custom_self: core.raw_read_16(ewram.custom_state, -1) == 0xad00,
            })
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
/// bn1's older engine lays the record out differently from bn2-6 and
/// exe45, and keeps no destination-tile copy.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::AnyBitPattern, bytemuck::NoUninit)]
#[allow(dead_code)] // some fields are named for completeness, not read
struct RawUnit {
    _reserved_00: [u8; 0x3],
    /// The unit's owner: its absolute player index (0/1). Which player
    /// owns which slot varies per round, so every read of the pair goes
    /// through this byte.
    owner: u8,
    _reserved_04: [u8; 0x14],
    /// The tile the unit stands on, `[x, y]`, 1-based over the whole
    /// field: x 1..=6 left to right (columns 1-3 are the left player's
    /// side), y 1..=3 top to bottom. Derived empirically: a scripted
    /// d-pad route steps them +/-1 per move, and both units' values
    /// match the rendered field.
    tile: [u8; 2],
    _reserved_1a: [u8; 0x6],
    /// Current HP -- not the animated HUD counter. Derived empirically
    /// from the golden replays: starts at the save's computed max HP,
    /// drops on hits, hits 0 at the loser's KO tick, identically across
    /// regions and both perspectives.
    hp: u16,
    max_hp: u16,
    _reserved_24: [u8; 0x9c],
}
const _: () = assert!(std::mem::size_of::<RawUnit>() == 0xc0);

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
/// This console's own loaded chip, tagged with the fire count the way
/// bn2-6 tag theirs so repeats of the same id still transition per use.
/// `None` once the stack is spent. See `EWRAMOffsets::chip_stack`
/// -- bn1 records this per console, so it answers for one player only.
fn loaded_chip(ewram: &EWRAMOffsets, core: &mut mgba::core::Core) -> Option<u16> {
    let fired = core.raw_read_8(ewram.chips_fired, -1) as u32;
    if fired >= 6 {
        return None;
    }
    let id = core.raw_read_8(ewram.chip_stack + fired, -1) as u16;
    // 0xFF is the empty slot and 0 is the pre-battle fill; every other
    // byte is a real id -- bn1's run past 0xBD, so there is no tighter
    // plausibility bound to draw here.
    if id == 0 || id == 0xff {
        return None;
    }
    Some(id | (((fired as u16) & 7) << 12))
}

// ---------------------------------------------------------------------------
// Per-version EWRAM/ROM offsets.

#[derive(Clone, Copy)]
struct EWRAMOffsets {
    /// Subsystem control.
    subsystem_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    submenu_control: u32,

    /// The START menu's remembered-tab byte (+6 of the persistent menu
    /// context block): the menu-open code re-inits the submenu block
    /// with submenu id = this byte * 4, and the battle exit rebuilds
    /// the menu from it too — it holds the comm tab for the whole link
    /// session. Tango writes it (the selection the player would have
    /// made) just before redirecting into the menu-open branch.
    start_menu_tab: u32,

    /// Shared RNG state. Must be synced.
    rng_state: u32,

    /// Main-loop tick counter (`frames_since_boot`). The battle-start
    /// routine reads this halfword and uses it as `stage = counter % 12`,
    /// so Tango overwrites it from `rng_state` before each round to make
    /// the game's own stage pick come out synced across peers.
    frame_counter: u32,

    /// The first in-battle unit's [`RawUnit`] record; the second
    /// follows immediately. This is the record the game itself hands
    /// around -- both slots' addresses sit in its own unit pointer
    /// table (0x02003750 on this version), which is how the base was
    /// pinned rather than guessed from a mid-struct anchor.
    unit: u32,

    /// This console's own loaded chip stack -- the one the HUD draws --
    /// as `ids[6]`, 0xFF for an empty slot. NOT the chip-select block
    /// at 0x02003788: that one is rewritten the moment the screen
    /// closes, while this stack and `chips_fired` only flip over when
    /// the battle actually resumes, so they stay consistent with each
    /// other. Reading the select block instead leaves a ~280-tick
    /// window each cycle where a stale fire count indexes the new
    /// picks, which invents a chip use. CONSOLE-LOCAL: the remote's
    /// stack is in no byte of this core's EWRAM, so each core answers
    /// for its own player, exactly like `custom_state`. Derived
    /// empirically by queueing known chips through the real chip select
    /// and following them through firing.
    chip_stack: u32,
    /// How many of `chip_stack`'s chips have fired -- so the one loaded
    /// is `ids[chips_fired]`. Resets to 0 as the stack flips over and
    /// stops once the stack is spent.
    chips_fired: u32,

    /// Battle-mode state entry that pins the custom screen: the u16 here
    /// holds one specific handler value exactly while the LOCAL player's
    /// chip-select is open (opening through this side's confirm) and never
    /// otherwise. The screen-flow state is per-console on this engine
    /// generation, so the remote's solo picking time is not visible here â€”
    /// unlike bn4/bn5/bn6, whose flags cover either player. Derived
    /// empirically from the golden replays against frozen-field windows;
    /// identical across US/JP. The value lives in
    /// the poller.
    custom_state: u32,
}

#[derive(Clone, Copy)]
struct ROMOffsets {
    /// The sound driver's sfx-request thunk (r0 = the sfx id): the
    /// single entry every sfx request funnels through (`push {r1-r7,
    /// lr}; bl <driver>; pop`). The primer returns straight to the
    /// caller from here while this core is still priming — see the
    /// silent-walk trap.
    play_sfx_entry: u32,

    /// The music counterpart of `play_sfx_entry` (its twin thunk, one
    /// function down; r0 = the song id): every music request funnels
    /// through here, including `battle_start_play_music_call`'s.
    play_music_entry: u32,

    /// The logo (state-0) handler's entry in the start screen's jump
    /// table — trapped with r5 = the applet control block already
    /// loaded by the dispatcher.
    ///
    /// Here, Tango redirects into `start_screen_fade_to_title`.
    start_screen_logo_entry: u32,

    /// The start screen's state-0x0c handler: a full push{lr}..pop{pc}
    /// function whose body is the fade-gated `[r5] = 0x10` transition
    /// to the title state. `start_screen_logo_entry`'s trap
    /// PC-redirects here (entry-site → full-function shape).
    start_screen_fade_to_title: u32,

    /// The title-wait substate handler's terminal `pop {pc}` — the
    /// title screen's own START-poll loop, one tick after its init ran
    /// the SRAM unmask + checksum checks and armed the attract timer.
    ///
    /// Here, Tango redirects into `title_confirm_continue`.
    title_wait_ret: u32,

    /// The title input helper's A/START confirm branch: stops the
    /// music, plays the confirm sfx and fades out into the load state,
    /// which reads the organically-set cursor (CONTINUE when the save
    /// checksums pass) and calls the game's own save load. One
    /// instruction past the helper's fade gate; ends at the helper's
    /// `pop {pc}` with balanced pushes, so the wait handler's saved lr
    /// feeds the pop. `title_wait_ret`'s trap PC-redirects here.
    title_confirm_continue: u32,

    start_screen_play_music_call: u32,

    /// This is immediately after game initialization is complete (the
    /// save-load fn's terminal `pop {pc}`): the internal state is set
    /// correctly.
    ///
    /// Here, Tango seeds the rng and redirects into
    /// `overworld_open_start_menu`.
    game_load_ret: u32,

    /// The overworld's START-menu-open branch (state 0x1c, one
    /// instruction past its fade gate): menu-open bookkeeping, submenu
    /// block re-init from the remembered-tab byte, subsystem control =
    /// START menu, menu sound. Balanced pushes ending in `pop {pc}`,
    /// so `game_load_ret`'s saved lr feeds the pop.
    /// `game_load_ret`'s trap PC-redirects here.
    overworld_open_start_menu: u32,

    /// First instruction of the battle-start routine (the `push {r5, lr}`
    /// prologue). Tango uses this to seed `rng_state` early enough that the
    /// stage-pick code further down the same function sees a synced value.
    round_start_entry: u32,

    /// This is the entry point to the comm menu.
    ///
    /// Here, Tango redirects into `comm_menu_start_netbattle`.
    comm_menu_init_ret: u32,

    /// The comm switchboard's netbattle branch: the confirm code the
    /// menu runs when the player picks link battle, one instruction
    /// past the guard dispatch. Resets the packet buffers, broadcasts
    /// the hello marker and walks the dispatcher to the link-battle
    /// bring-up state. `comm_menu_init_ret`'s trap PC-redirects here.
    comm_menu_start_netbattle: u32,

    /// This hooks the point after the battle start routine is complete —
    /// the game's own round start, reported to the telemetry lifecycle
    /// sink.
    round_start_ret: u32,

    /// The battle-start routine's BGM call (a 4-byte `bl`). PC-skipped
    /// by the primer when the host asked for silent battles
    /// (`PrimeConfig::disable_bgm`).
    battle_start_play_music_call: u32,

    /// Where the battle-result code records a WIN for this core's local
    /// player (trap-era `round_end_set_win`, KO-probe-verified to fire on
    /// the PvP engine's real protocol route). Reported from core 0 as the
    /// round outcome (core 0's local player is player 0).
    round_end_set_win: u32,
    /// The LOSS counterpart of `round_end_set_win`.
    round_end_set_loss: u32,

    /// This hooks the exit from the battle mode's teardown, right as the
    /// dispatcher returns to the comm menu. bn1 matches are one battle —
    /// there is no rematch conversation — so this is the game's own match
    /// end, reported to the telemetry lifecycle sink (KO-probe-verified
    /// on the real protocol route).
    match_end_ret: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    subsystem_control:      0x02006cb8,
    submenu_control:        0x020062e0,
    start_menu_tab:         0x0200acf6,
    rng_state:              0x02006cc0,
    frame_counter:          0x020064a0,
    unit:                   0x020066b0,
    chip_stack:             0x0200765a,
    chips_fired:            0x020075f1,
    custom_state:           0x02008014,
};

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static AREE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x08000668,
        play_music_entry:                           0x08000678,
        start_screen_logo_entry:                    0x08018ccc,
        start_screen_fade_to_title:                 0x08018d84,
        title_wait_ret:                             0x080105ca,
        title_confirm_continue:                     0x08010866,
        start_screen_play_music_call:               0x08010498,
        game_load_ret:                              0x0800407e,
        overworld_open_start_menu:                  0x0800442e,
        round_start_entry:                          0x080051f4,
        comm_menu_init_ret:                         0x0801ce94,
        comm_menu_start_netbattle:                  0x0801d1ce,
        round_start_ret:                            0x0800527a,
        round_end_set_win:                          0x08006d18,
        round_end_set_loss:                         0x08006d20,
        match_end_ret:                              0x08005cd0,
        battle_start_play_music_call:               0x080059ec,
    },
};

#[rustfmt::skip]
static AREJ_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x08000658,
        play_music_entry:                           0x08000668,
        start_screen_logo_entry:                    0x08018c40,
        start_screen_fade_to_title:                 0x08018cf8,
        title_wait_ret:                             0x08010562,
        title_confirm_continue:                     0x080107ea,
        start_screen_play_music_call:               0x08010464,
        game_load_ret:                              0x0800406e,
        overworld_open_start_menu:                  0x0800441e,
        round_start_entry:                          0x080051e4,
        comm_menu_init_ret:                         0x0801cd90,
        comm_menu_start_netbattle:                  0x0801d0b2,
        round_start_ret:                            0x0800526a,
        round_end_set_win:                          0x08006d08,
        round_end_set_loss:                         0x08006d10,
        match_end_ret:                              0x08005cc0,
        battle_start_play_music_call:               0x080059dc,
    },
};

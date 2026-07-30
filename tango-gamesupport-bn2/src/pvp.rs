//! PvP-engine support: priming pokes and telemetry polls.
//!
//! Priming: the boot fast-path PC-redirects through the game's own
//! menu code (logo → title → CONTINUE → START menu → comm menu), then
//! the comm menu's init return is PC-redirected into the comm
//! switchboard's netbattle branch — the game itself stages its
//! settings packet and walks the dispatcher into the settings-exchange
//! state. From there the games run the REAL settings
//! exchange over the emulated cable (dispatcher jump table 0x0802b0e4
//! AE2E, indexed submenu_control[1]/4):
//! - 0x28 (handler 0x0802b6f0): per-tick parse of the peer's packet
//!   (rx[6..16] all equal rx[1]); nibble 0xf = idle, keep waiting;
//!   nibble 4 (this mode's settings value) = agreement — the SIO
//!   master (player id 0, SIOCNT>>4&3) draws the background from its
//!   own rng (rand(8) through the bg table at 0x0802b78c) and writes
//!   it into tx[2], which the SIO layer keeps broadcasting.
//! - 0x2c (handler 0x0802b79c): once the link session is up, battle
//!   init (0x80043d0) — the master with its own tx[2], the slave with
//!   the byte the master's packet carried over the cable (it waits
//!   for rx[2] != 0xff first). This native master→slave transmission
//!   is what makes the two consoles' stages agree.
//!
//! RNG model: the game's single rng is seeded once per core at save
//! load, like two real carts — nothing is shared. The stage agreement
//! comes from the cable (above), not rng lockstep, and each player's
//! chip luck is naturally their own.

use tango_backend_mgba::Trap;
use tango_gamesupport_common::telemetry::LoadedChip;

pub struct Pvp {
    offsets: &'static Offsets,
}

pub static PVP_AE2E_00: Pvp = Pvp { offsets: &AE2E_00 };
pub static SIO_AE2J_00_AC: Pvp = Pvp { offsets: &AE2J_00_AC };

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
        events: &tango_match::telemetry::EventSink,
        primed: &tango_backend_mgba::PrimedLatch,
    ) -> Vec<Trap> {
        use tango_match::telemetry::Outcome;

        let rom = &self.offsets.rom;
        let ewram = &self.offsets.ewram;
        let disable_bgm = config.disable_bgm;
        // Seed the game's single rng per core once, at save load (see
        // module docs).
        let rng = config.core_rng_seed(player, 0);
        let fade_to_title = rom.start_screen_fade_to_title;
        let title_confirm = rom.title_confirm_continue;
        let open_start_menu = rom.field_open_start_menu;
        let start_netbattle = rom.comm_menu_start_netbattle;
        // Lifecycle signals are host-side only — core state is untouched,
        // so the simulation is unaffected. Rounds are reported from core 0
        // (whose local player is player 0); core 1's lifecycle traps stay
        // silent. Match end is the exception, reported from both cores —
        // see its trap below.
        let sink = (player == 0).then(|| events.clone());
        let primed = primed.clone();
        // The game's own round-result judgment, ~120 ticks before the
        // battle-mode exit: set_win/set_loss on the KO route
        // (KO-probe-verified on the real protocol route, at the KO),
        // the damage-judge trio on the timeout route (same trap-era
        // judgment family; not reachable cheaply headless). Each
        // records THIS core's local player's result, and core 0's
        // local player is player 0.
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
            // The walked menus run the game's own confirm/open code,
            // which requests sounds like any organic pass: the title
            // confirm jingle, the netbattle confirm, the link bring-up's
            // connect jingle ~8 ticks before battle start — and the
            // START-menu-open bookkeeping resumes the field music the
            // fast-path never started, which would otherwise loop under
            // the whole battle when the host asked for silent ones.
            // Priming runs far faster than real time, so the host clears
            // the piled-up sample buffers when it ends — but that drops
            // rendered samples, not driver state, and the late requests
            // keep sounding over the session open. Gate the driver's two
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
                // substate armed the attract timer; the title init's SRAM
                // checksum checks set the valid-save flags). Poke the
                // NEW GAME/CONTINUE cursor to CONTINUE — the selection the
                // player would have made; bn2's organic default comes from
                // an SRAM slot byte, not the checksum checks, so it is
                // written deterministically here, just before the load
                // state reads it — then, instead of popping, PC-redirect
                // into the title input helper's confirm branch: the code
                // an A/START press runs (stops the music, plays the
                // confirm sfx, walks the substate into the fade-gated
                // exit, which routes — attract timer still >0 — to the
                // load state; that state reads the cursor and calls the
                // game's own save load). The branch is past the helper's
                // `push {lr}` and ends at its `pop {pc}`, so the wait
                // handler's saved lr feeds the pop and control returns to
                // the dispatcher cleanly. Fires once: the first redirect
                // leaves the wait substate.
                rom.title_wait_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_8(ewram.title_menu_control + 0x08, -1, 0x01);
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
                    // field's own START-menu-open branch (past its fade
                    // gate): its two calls zero-fill and re-init the
                    // submenu block from the tab byte (submenu id = tab *
                    // 4 = the comm menu, entry byte [4] = 0 = from-field)
                    // and run the menu-open bookkeeping — sounds, display
                    // sync, subsystem control = START menu — everything
                    // the old poke wrote by hand. The branch is `bl; bl;
                    // pop {pc}`, so the load fn's own saved lr feeds the
                    // pop and control returns to the title load state
                    // cleanly.
                    core.raw_write_8(ewram.start_menu_tab, -1, 0x06);
                    core.gba_mut().cpu_mut().set_thumb_pc(open_start_menu);
                }),
            ),
            (
                // The comm menu init handler's entry, one tick after the
                // menu-open above. [4] = the comm-menu entry being entered
                // (1-based; organically the comm list's confirm re-enters
                // the menu with it, and the battle exit re-inits with it
                // preserved). Write entry 1 — netbattle, the selection the
                // player would have made — at the moment just before the
                // init's own read: nonzero routes the init's link branch,
                // which sets the list cursor to [4]-1, runs the SIO
                // bring-up (0x80e62c8 + 0x80e6124: RCNT=0, SIOCNT=
                // multi+IRQ, lib struct reset — without it no multi-mode
                // transfer chain ever starts and the settings exchange
                // stalls out to the error state) and walks the dispatcher
                // to the menu top. Gated on [4] == 0: the battle exit's
                // own re-init already carries the entry, so this only
                // fills in the from-field open (pure core RAM — rollback
                // re-simulation evaluates it identically).
                rom.comm_menu_init_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    if core.raw_read_8(ewram.submenu_control + 0x4, -1) != 0 {
                        return;
                    }
                    core.raw_write_8(ewram.submenu_control + 0x4, -1, 0x01);
                    // The menu-open above has consumed the tab byte; clear
                    // it so the battle exit rebuilds the plain START menu
                    // (tab 0), exactly as it always has — bn2's submenu
                    // block has no from-battle flag to gate the init-ret
                    // redirect on, so a lingering comm tab would make the
                    // post-battle re-init run back into it and force a
                    // re-battle.
                    core.raw_write_8(ewram.start_menu_tab, -1, 0x00);
                }),
            ),
            (
                // The comm menu's init return — the init handler's terminal
                // `pop {pc}`; its link branch has already run the SIO
                // bring-up and advanced the dispatcher to the menu top.
                // Instead of popping, PC-redirect into the comm
                // switchboard's netbattle branch: the game's own code
                // resets the tx/rx buffers, broadcasts the netbattle
                // settings value into its tx packet, zeroes the session
                // counters and walks the dispatcher to the
                // settings-exchange state ([1]=0x28) — everything the old
                // poke wrote by hand. The branch target is past the
                // switchboard's `push {lr}`, so the init handler's own
                // saved lr feeds the branch's terminating `pop {pc}` and
                // control returns to the dispatcher cleanly.
                rom.comm_menu_init_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(start_netbattle);
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
                // The battle-mode exit. bn2 matches are a single battle (no
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
                // bn2's custom flag is local-player semantics: the
                // battle-mode state entry holds this one handler value
                // exactly while this side's chip select is open (see
                // `EWRAMOffsets::custom_state`).
                let custom_self = core.raw_read_16(self.ewram.custom_state, -1) == 0xb900;
                // This core's own player's chip fires, off its block's
                // picked-minus-remaining cursor (see `loaded_chips`).
                self.chips.tick(
                    round,
                    loaded_chips(self.ewram, core, units)[self.player],
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
    _reserved_17: [u8; 0x3],
    /// Chips remaining to fire. Decrements at each fire; the loaded
    /// chip is `ids[picked - remaining]` of this player's block. Byte-
    /// sized on purpose: the adjacent bytes hold unrelated counters (a
    /// countdown timer after exhaustion, slow drifting state on the
    /// other slot), so values above the picked count are garbage, not
    /// chips.
    chips_remaining: u8,
    _reserved_1b: [u8; 0x9],
    /// Current HP -- not the animated HUD counter. Derived empirically
    /// from the golden replays: starts at the save's computed max HP,
    /// drops on hits, hits 0 at the loser's KO tick, identically across
    /// regions and both perspectives.
    hp: u16,
    max_hp: u16,
    _reserved_28: [u8; 0x98],
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
/// Both players' loaded-chip readings (`None` when the hand is spent),
/// indexed by absolute player: `ids[picked - remaining]`
/// of each slot owner's chip block, with the fire cursor: the
/// hand-cursor contract `HandChipTracker` detects fires on. See
/// `EWRAMOffsets::chip_blocks`.
fn loaded_chips(ewram: &EWRAMOffsets, core: &mut mgba::core::Core, units: [RawUnit; 2]) -> [Option<LoadedChip>; 2] {
    std::array::from_fn(|player| {
        let remaining = units[player].chips_remaining as u32;
        let base = ewram.chip_blocks + player as u32 * 0x7e;
        let mut picked = 0u32;
        while picked < 6 {
            let id = core.raw_read_16(base + picked * 2, -1);
            if id == 0 || id == 0xffff {
                break;
            }
            picked += 1;
        }
        if remaining == 0 || remaining > picked {
            return None;
        }
        let idx = picked - remaining;
        Some(LoadedChip {
            id: core.raw_read_16(base + idx * 2, -1),
            fires: idx as u16,
        })
    })
}

// ---------------------------------------------------------------------------
// Per-version EWRAM/ROM offsets.

#[derive(Clone, Copy)]
struct EWRAMOffsets {
    /// Title menu jump table control. Byte [8] is the NEW GAME/CONTINUE
    /// cursor the title load state reads (0 = new game, 1 = continue).
    title_menu_control: u32,

    /// Subsystem control.
    subsystem_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    submenu_control: u32,

    /// The START menu's remembered-tab byte (+6 of the persistent menu
    /// context block): the menu-open code re-inits the submenu block
    /// with submenu id = this byte * 4, and the battle exit rebuilds
    /// the menu from it too. Tango writes it (the selection the player
    /// would have made) just before redirecting into the menu-open
    /// branch, and clears it once the open has consumed it — see the
    /// `comm_menu_init_entry` trap.
    start_menu_tab: u32,

    /// Shared RNG state. Must be synced.
    rng_state: u32,

    /// The first in-battle unit's [`RawUnit`] record; the second
    /// follows immediately. This is the record the game itself hands
    /// around -- both slots' addresses sit in its own unit pointer
    /// table (0x02004f20 on this version), which is how the base was
    /// pinned rather than guessed from a mid-struct anchor.
    unit: u32,
    /// Player 0's selected-chip ids; player 1's are 0x7e beyond. Layout:
    /// u16 ids[6] (0xFFFF = empty) then u16 codes at +0xc. Written when
    /// the owner's console commits its selection, indexed by absolute
    /// player, NOT by unit slot. Anchored by the picked-count matching
    /// each side's remaining counter at load (a stray id-valued word
    /// sits at player 0's -2 and is NOT part of the array); the stride
    /// is the distance between the two observed anchors. Derived
    /// empirically from the golden replays.
    chip_blocks: u32,

    /// Battle-mode state entry that pins the custom screen: the u16 here
    /// holds one specific handler value exactly while the LOCAL player's
    /// chip-select is open (opening through this side's confirm) and never
    /// otherwise. The screen-flow state is per-console on this engine
    /// generation, so the remote's solo picking time is not visible here —
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
    /// through here, including `battle_start_play_music_call`'s and
    /// the menu-open bookkeeping's field-music resume.
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
    /// Here, Tango pokes the CONTINUE cursor and redirects into
    /// `title_confirm_continue`.
    title_wait_ret: u32,

    /// The title input helper's A/START confirm branch: stops the
    /// music, plays the confirm sfx and fades out into the load state,
    /// which reads the cursor and calls the game's own save load. One
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
    /// `field_open_start_menu`.
    game_load_ret: u32,

    /// The field handler's START-menu-open branch (one instruction
    /// past its fade gate): `bl` menu-block re-init from the
    /// remembered-tab byte, `bl` menu-open bookkeeping (sounds,
    /// display sync, subsystem control = START menu), `pop {pc}` — so
    /// `game_load_ret`'s saved lr feeds the pop.
    /// `game_load_ret`'s trap PC-redirects here.
    field_open_start_menu: u32,

    /// The comm menu init handler's entry (the state-0 slot of the
    /// comm dispatcher). Trapped to write the comm-menu entry byte [4]
    /// the init's link branch reads (see the trap).
    comm_menu_init_entry: u32,

    /// This is the entry point to the comm menu (the init handler's
    /// terminal `pop {pc}`).
    ///
    /// Here, Tango redirects into `comm_menu_start_netbattle`.
    comm_menu_init_ret: u32,

    /// The comm switchboard's netbattle branch: the confirm code the
    /// menu top runs when the player picks netbattle (`[r5, #0x12]` =
    /// 0), one instruction past the function's `push {lr}`. Resets the
    /// packet buffers, broadcasts the netbattle settings value, zeroes
    /// the session counters and walks the dispatcher to the
    /// settings-exchange state ([1]=0x28). `comm_menu_init_ret`'s trap
    /// PC-redirects here.
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
    /// player on the KO route (trap-era `round_end_set_win`,
    /// KO-probe-verified to fire on the PvP engine's real protocol
    /// route). Reported from core 0 as the round outcome (core 0's local
    /// player is player 0).
    round_end_set_win: u32,
    /// The LOSS counterpart of `round_end_set_win`.
    round_end_set_loss: u32,
    /// The timeout route's damage-judge verdicts (trap-era anchors, same
    /// judgment family as `round_end_set_win`; a timeout isn't reachable
    /// cheaply headless so these ride on the trap-era disasm).
    round_end_damage_judge_set_win: u32,
    round_end_damage_judge_set_loss: u32,
    round_end_damage_judge_set_draw: u32,

    /// This hooks the exit from the battle mode's teardown, right as the
    /// dispatcher returns to the comm menu. bn2 matches are one battle —
    /// there is no rematch conversation — so this is the game's own match
    /// end, reported to the telemetry lifecycle sink (KO-probe-verified
    /// on the real protocol route).
    match_end_ret: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    title_menu_control:     0x02009b80,
    subsystem_control:      0x02009078,
    submenu_control:        0x02007ea0,
    start_menu_tab:         0x0200d586,
    rng_state:              0x02009080,
    unit:                   0x02008a70,
    chip_blocks:            0x02009c32,
    custom_state:           0x0200a3b4,
};

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static AE2E_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x08000598,
        play_music_entry:                           0x080005a8,
        start_screen_logo_entry:                    0x08024a7c,
        start_screen_fade_to_title:                 0x08024b44,
        title_wait_ret:                             0x0801c302,
        title_confirm_continue:                     0x0801c4b0,
        start_screen_play_music_call:               0x0801c174,
        game_load_ret:                              0x08003ccc,
        field_open_start_menu:                      0x08004108,
        comm_menu_init_entry:                       0x0802b19c,
        comm_menu_init_ret:                         0x0802b2a0,
        comm_menu_start_netbattle:                  0x0802b4fe,
        round_start_ret:                            0x08004e34,
        round_end_set_win:                          0x08006ec8,
        round_end_set_loss:                         0x08006ed0,
        round_end_damage_judge_set_win:             0x08005fd8,
        round_end_damage_judge_set_loss:            0x08005fc8,
        round_end_damage_judge_set_draw:            0x08005fbe,
        match_end_ret:                              0x080061a2,
        battle_start_play_music_call:               0x08006ce0,
    },
};

#[rustfmt::skip]
static AE2J_00_AC: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x08000598,
        play_music_entry:                           0x080005a8,
        start_screen_logo_entry:                    0x08024984,
        start_screen_fade_to_title:                 0x08024a44,
        title_wait_ret:                             0x0801c18e,
        title_confirm_continue:                     0x0801c33c,
        start_screen_play_music_call:               0x0801c000,
        game_load_ret:                              0x08003ccc,
        field_open_start_menu:                      0x08004104,
        comm_menu_init_entry:                       0x0802b018,
        comm_menu_init_ret:                         0x0802b11c,
        comm_menu_start_netbattle:                  0x0802b37a,
        round_start_ret:                            0x08004e30,
        round_end_set_win:                          0x08006d88,
        round_end_set_loss:                         0x08006d90,
        round_end_damage_judge_set_win:             0x08005fc8,
        round_end_damage_judge_set_loss:            0x08005fb8,
        round_end_damage_judge_set_draw:            0x08005fae,
        match_end_ret:                              0x08006192,
        battle_start_play_music_call:               0x08006ba0,
    },
};

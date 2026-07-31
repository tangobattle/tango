//! PvP-engine support: priming pokes and telemetry polls.
//!
//! Priming: the boot fast-path PC-redirects through the game's own
//! menu code (logo → title → CONTINUE → START menu → comm menu), then
//! the comm menu's init return is PC-redirected into the comm
//! switchboard's netbattle-confirm branch — the game itself stages its
//! settings packet and walks the dispatcher into the settings-exchange
//! state —
//! with the rx slots pre-seeded with the organic idle packet on the
//! exchange's first tick (buffer bring-up, see the settings-entry
//! trap). From there the games
//! run the REAL settings exchange over the emulated cable (dispatcher
//! jump table 0x0803df48 A6BE, indexed submenu_control[1]/4):
//! - 0x2c (handler 0x0803ea10): per-tick parse of the peer's packet
//!   (rx[0]==1, rx[8..16] all equal rx[1]); settings nibble 0xf =
//!   idle, keep waiting; nibble == battle_kind*2 (the game's
//!   `submenu_control+0x1c` battle-kind halfword doubled, helper
//!   0x0803ea88) = agreement — the SIO
//!   master (player id 0, SIOCNT>>4&3) draws the background/stage
//!   from its own rng2 (rand(8) via 0x0800168c through the bg table)
//!   and writes it into tx[4].
//! - 0x34 (handler 0x0803eac4): wait for the peer's bg byte
//!   (rx[4] != 0xff) and adopt it into tx[4] — the native
//!   master→slave bg transmission (on the master, the slave's echo of
//!   its own value). This is what makes the two consoles' stages
//!   agree.
//! - 0x30 (handler 0x0803eb30): final packet check, then battle init
//!   (0x0803ec30) with bg = tx[4]. Parse failure in any state routes
//!   to 0x38, the "communication failed" dialog.
//!
//! RNG model: both rngs are seeded once per core at save load, like
//! two real carts — nothing is shared. Only the master's rng2 draw
//! decides the stage (the slave receives the byte over the cable),
//! and each player's chip luck (rng1) is naturally their own.

use tango_backend_mgba::Trap;
use tango_gamesupport_common::telemetry::LoadedChip;

pub struct Pvp {
    offsets: &'static Offsets,
}

pub static PVP_A3XE_00: Pvp = Pvp { offsets: &A3XE_00 };
pub static PVP_A6BE_00: Pvp = Pvp { offsets: &A6BE_00 };
pub static PVP_A3XJ_01: Pvp = Pvp { offsets: &A3XJ_01 };
pub static PVP_A6BJ_01: Pvp = Pvp { offsets: &A6BJ_01 };

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
        // The game's battle-kind byte (`submenu_control + 0x1c`, read as a
        // halfword by the settings parse): 0 = lightweight, 1 = midweight,
        // 2 = heavyweight, 3 = tri-battle (a best-of-three set). Tango's
        // (mode, subtype) selection maps onto it exactly as the trap
        // engine's `bn3_match_type` did: mode 0 = single battle with
        // subtype 1/2/3 = light/mid/heavy and subtype 0 = a random weight
        // (drawn from the shared match seed — identical on both cores and
        // both peers); mode 1 = tri-battle.
        let battle_kind = match config.match_type {
            (0, 1) => 0,
            (0, 2) => 1,
            (0, 3) => 2,
            (0, _) => (config.core_rng_seed(0, 4) % 3) as u8,
            (1, _) => 3,
            _ => 0,
        };
        // Seed both rngs per core once, at save load (see module docs).
        let fade_to_title = rom.start_screen_fade_to_title;
        let intro_exit = rom.title_intro_exit;
        let title_confirm = rom.title_confirm_continue;
        let open_start_menu = rom.field_open_start_menu;
        let start_netbattle = rom.comm_menu_start_netbattle;
        let rng1 = config.core_rng_seed(player, 0);
        let rng2 = config.core_rng_seed(player, 1);
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

        let traps: Vec<Trap> = vec![
            // ----- the silent walk -----
            // The walked menus run the game's own confirm/open code,
            // which requests sounds like any organic pass: the title
            // confirm jingle, the comm menu's confirm, and the link
            // bring-up's connect jingle ~30 ticks before battle start.
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
                // JP only (0 = absent on US): the JP builds put an
                // unskippable ~190-tick intro animation phase between the
                // title init and the START-poll phase, as an extra outer
                // title state. Trap that state's handler entry (r5 = the
                // title control block, loaded by the title dispatcher) and
                // PC-redirect to the intro's OWN exit substate handler (a
                // full push{lr}..pop{pc} function): fade-gated, it runs
                // the game's title-gfx set call and walks the outer state
                // to the START-poll phase — exactly where the intro's last
                // substate lands. While the fade gate holds the state
                // stays put and this trap re-fires next tick, so the
                // transition self-retries.
                rom.title_intro_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(intro_exit);
                }),
            ),
            (
                // The title-wait substate handler's terminal `pop {pc}` —
                // the state that polls for START each tick (the previous
                // substate armed the attract timer). Poke the NEW GAME/
                // CONTINUE cursor to CONTINUE — the selection the player
                // would have made. The game's own default comes from its
                // SRAM checksum checks and holds CONTINUE only for a save
                // of the exact same version; Tango feeds White saves to
                // Black carts (and vice versa), which the load fn accepts
                // but the checksum check rejects, so the cursor is written
                // deterministically here, just before the load state reads
                // it — exactly the choice the old poke forced. Then,
                // instead of popping, PC-redirect into the title input
                // helper's confirm branch — the code an A/START press
                // runs: stops the music, plays the confirm sfx and walks
                // the substate into the fade-gated exit, which routes
                // (attract timer still >0) to the load state; that state
                // reads the cursor and calls the game's own save load.
                // The branch is past the helper's `push {lr}` and ends at
                // its `pop {pc}`, so the wait handler's saved lr feeds the
                // pop and control returns to the dispatcher cleanly. Fires
                // once: the first redirect leaves the wait substate.
                rom.title_wait_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_8(ewram.title_menu_control + 0x08, -1, 0x01);
                    core.gba_mut().cpu_mut().set_thumb_pc(title_confirm);
                }),
            ),
            (
                // The instruction right after the title load state's
                // CONTINUE-path save-load call — game initialization is
                // complete (bn3's load fn ends in `pop {r4-r7, pc}`, so
                // the redirect is anchored here, back in the load state's
                // plain-lr frame, instead of at the load fn's own pop).
                rom.title_continue_load_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    // Seed the rngs (see module docs).
                    core.raw_write_32(ewram.rng1_state, -1, rng1);
                    core.raw_write_32(ewram.rng2_state, -1, rng2);
                    // Write the START menu's remembered-tab byte to the
                    // comm menu — the selection the player would have made
                    // — then PC-redirect into the field's own START-menu-
                    // open branch (past its fade gate): its two calls
                    // zero-fill and re-init the submenu block from the tab
                    // byte (submenu id = tab * 4 = the comm menu, entry
                    // flag [3] = 0 = from-field) and run the menu-open
                    // bookkeeping — sounds, display sync, subsystem
                    // control = START menu — everything the old poke wrote
                    // by hand. The branch is `bl; bl; pop {pc}`, so the
                    // load state's own saved lr feeds the pop and control
                    // returns to the title dispatcher cleanly (skipping
                    // only the state's field-music call).
                    core.raw_write_8(ewram.start_menu_tab, -1, 0x06);
                    core.gba_mut().cpu_mut().set_thumb_pc(open_start_menu);
                }),
            ),
            (
                // The comm menu's init return — the init handler's terminal
                // `pop {pc}`. Write the battle-kind byte the confirm path
                // reads (the game would have taken it from the weight-menu
                // cursor), then, instead of popping, PC-redirect into the
                // comm switchboard's netbattle-confirm branch: the game's
                // own code resets the tx/rx buffers, broadcasts its
                // settings value (the `submenu_control+0x1c` halfword
                // doubled, via its own helper) into the tx packet and walks
                // the dispatcher to the settings-exchange state ([1]=0x2c)
                // — everything the old poke wrote by hand. The branch
                // target is past the switchboard's `push {lr}`, so the init
                // handler's own saved lr feeds the branch's terminating
                // `pop {pc}` and control returns to the dispatcher cleanly.
                rom.comm_menu_init_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    // The menu-open has consumed the tab byte; clear it so
                    // the battle exit rebuilds the plain START menu (tab
                    // 0), exactly as it always has — a lingering comm tab
                    // would make the post-battle re-init run back into the
                    // comm menu and this trap would hijack it into a
                    // forced re-battle.
                    core.raw_write_8(ewram.start_menu_tab, -1, 0x00);
                    core.raw_write_8(ewram.submenu_control + 0x1c, -1, battle_kind);
                    core.gba_mut().cpu_mut().set_thumb_pc(start_netbattle);
                }),
            ),
            (
                // The settings-exchange state's per-tick handler. On its
                // first tick, pre-seed both rx packet slots with the
                // organic idle packet ([0]=1 marker, [2]=0, everything else
                // 0xff — the reset tx a peer broadcasts while it sits in
                // the comm menu). The pre-settings menu states we skip pump
                // this packet over the cable continuously, so a core
                // organically entering the settings state always finds it
                // in rx; without it the per-tick rx parse (rx[0] must be 1)
                // fails on the 0xff-filled reset buffer during the few
                // ticks the SIO transfer chain needs to deliver the peer's
                // first real packet, and the dispatcher bails to the error
                // state (0x38, the "communication failed" dialog). Real
                // received packets overwrite the seed within a few ticks.
                // This is buffer bring-up, not handshake data: the idle
                // settings nibble (0xf) just holds the parse in its wait
                // branch. Gated on the reset rx marker (pure core RAM —
                // rollback re-simulation evaluates it identically), so it
                // fires once per bring-up and never once packets flow.
                rom.comm_menu_settings_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    if core.raw_read_8(ewram.rx_packet_arr, -1) != 0xff {
                        return;
                    }
                    const IDLE_PACKET: [u8; 0x10] = [
                        0x01, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                    ];
                    core.raw_write_range(ewram.rx_packet_arr, -1, &IDLE_PACKET);
                    core.raw_write_range(ewram.rx_packet_arr + 0x10, -1, &IDLE_PACKET);
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
                // The battle-set exit. A bn3 comm match is one battle for
                // kinds 0-2 and a best-of-three set for tri-battle, and this
                // anchor has exactly match-end semantics for both
                // (KO-probe-verified on the real protocol route): mid-set the
                // round teardown branches into the next battle's init and
                // `round_start_ret` re-fires the same tick — skipping this
                // exit — while the set-deciding battle (and any single
                // battle) takes the exit path through here, exactly once.
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
        ];
        // Version-absent sites (the JP-only intro skip) carry address 0.
        traps.into_iter().filter(|(addr, _)| *addr != 0).collect()
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
                // bn3's custom flag is local-player semantics: each core
                // reports its own player's screen (see
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
    _reserved_28: [u8; 0xac],
}
const _: () = assert!(std::mem::size_of::<RawUnit>() == 0xd4);

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
/// of each slot owner's chip block (same shape as bn2's), with the
/// fire cursor: the hand-cursor contract `HandChipTracker`
/// detects fires on. See
/// `EWRAMOffsets::chip_blocks`.
fn loaded_chips(ewram: &EWRAMOffsets, core: &mut mgba::core::Core, units: [RawUnit; 2]) -> [Option<LoadedChip>; 2] {
    std::array::from_fn(|player| {
        let remaining = units[player].chips_remaining as u32;
        let base = ewram.chip_blocks + player as u32 * 0x24;
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
    // Incoming packet.
    rx_packet_arr: u32,

    /// Title menu jump table control. Byte [8] is the NEW GAME/CONTINUE
    /// cursor the title load state reads (0 = new game, 1 = continue).
    title_menu_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    submenu_control: u32,

    /// The START menu's remembered-tab byte (+6 of the persistent menu
    /// context block): the menu-open code re-inits the submenu block
    /// with submenu id = this byte * 4, and the battle exit rebuilds
    /// the menu from it too. Tango writes it (the selection the player
    /// would have made) just before redirecting into the menu-open
    /// branch, and clears it once the open has consumed it — see the
    /// `comm_menu_init_ret` trap.
    start_menu_tab: u32,

    /// Local RNG state. Doesn't need to be synced.
    rng1_state: u32,

    /// Shared RNG state. Must be synced.
    rng2_state: u32,

    /// The first in-battle unit's [`RawUnit`] record; the second
    /// follows immediately. This is the record the game itself hands
    /// around -- both slots' addresses sit in its own unit pointer
    /// table (0x02006ce4 on this version), which is how the base was
    /// pinned rather than guessed from a mid-struct anchor.
    unit: u32,
    /// Player 0's selected-chip block; player 1's is 0x24 beyond. Layout:
    /// +0 u16 ids[6] (0xFFFF = empty), +0xc u16 codes[6]. Written when
    /// the owner's console commits its selection (so cross-core write
    /// ticks differ inside the shared pause), cleared at custom open.
    /// Indexed by absolute player, NOT by unit slot. Derived empirically
    /// from the golden replays.
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

    /// JP only, 0 = absent: the JP builds' extra outer title state — a
    /// ~190-tick unskippable intro animation phase between the title
    /// init and the START-poll phase. Trapped at the state handler's
    /// entry.
    ///
    /// Here, Tango redirects into `title_intro_exit`.
    title_intro_entry: u32,

    /// JP only, 0 = absent: the intro phase's own exit substate
    /// handler — a full push{lr}..pop{pc} function: fade-gated title-
    /// gfx set + the outer-state walk to the START-poll phase.
    /// `title_intro_entry`'s trap PC-redirects here (entry-site →
    /// full-function shape).
    title_intro_exit: u32,

    /// The title-wait substate handler's terminal `pop {pc}` — the
    /// title screen's own START-poll loop (on JP builds it sits one
    /// intro phase after the init), one tick after the previous
    /// substate armed the attract timer.
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

    /// The instruction right after the title load state's
    /// CONTINUE-path `bl` to the save-load fn: game initialization is
    /// complete. (bn3's load fn ends in `pop {r4-r7, pc}`, so the
    /// primer anchors here — back in the load state's plain-lr frame —
    /// rather than at the load fn's own pop; the site exists only on
    /// the CONTINUE path, so it fires exactly once.)
    ///
    /// Here, Tango seeds the rngs and redirects into
    /// `field_open_start_menu`.
    title_continue_load_ret: u32,

    /// The field handler's START-menu-open branch (one instruction
    /// past its fade gate): `bl` menu-block re-init from the
    /// remembered-tab byte, `bl` menu-open bookkeeping (sounds,
    /// display sync, subsystem control = START menu), `pop {pc}` — so
    /// `title_continue_load_ret`'s saved lr feeds the pop.
    /// `title_continue_load_ret`'s trap PC-redirects here.
    field_open_start_menu: u32,

    /// This is the entry point to the comm menu.
    ///
    /// Here, Tango redirects into `comm_menu_start_netbattle`.
    comm_menu_init_ret: u32,

    /// The comm switchboard's netbattle-confirm branch: the code the
    /// menu runs when the player confirms a netbattle, one instruction
    /// past its variant dispatch. Resets the packet buffers, broadcasts
    /// the settings value derived from the battle-kind halfword and
    /// walks the dispatcher to the settings-exchange state ([1]=0x2c).
    /// `comm_menu_init_ret`'s trap PC-redirects here.
    comm_menu_start_netbattle: u32,

    /// Entry of the settings-exchange state's per-tick handler
    /// (dispatcher state [1] = 0x2c). Trapped to pre-seed the rx
    /// buffers with the organic idle packet on the first tick (see the
    /// trap).
    comm_menu_settings_entry: u32,

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

    /// This hooks the round-end routine's EXIT path — taken only when the
    /// battle set is over (any single-battle kind, or a tri-battle set
    /// decided 2-0/2-1): the game's own match end, reported to the
    /// telemetry lifecycle sink. Mid-set the same routine instead branches
    /// into the next battle's init (`round_start_ret` re-fires the same
    /// tick), skipping this address entirely. KO-probe-verified on the
    /// real protocol route for both shapes.
    match_end_ret: u32,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    rx_packet_arr:          0x0200a330,
    title_menu_control:     0x0200a300,
    submenu_control:        0x020093d0,
    start_menu_tab:         0x0200de56,
    rng1_state:             0x02009730,
    rng2_state:             0x02009800,
    unit:                   0x02037270,
    chip_blocks:            0x02034060,
    custom_state:           0x0200c0c4,
};

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static A3XE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x080005b8,
        play_music_entry:                           0x080005c8,
        start_screen_logo_entry:                    0x0802b358,
        start_screen_fade_to_title:                 0x0802b448,
        title_intro_entry:                          0x0,
        title_intro_exit:                           0x0,
        title_wait_ret:                             0x0802210a,
        title_confirm_continue:                     0x0802228c,
        title_continue_load_ret:                    0x08022080,
        field_open_start_menu:                      0x08004a24,
        comm_menu_init_ret:                         0x0803e08a,
        comm_menu_start_netbattle:                  0x0803e70e,
        comm_menu_settings_entry:                   0x0803e9f8,
        round_start_ret:                            0x080059a8,
        round_end_set_win:                          0x0800946a,
        round_end_set_loss:                         0x08009472,
        round_end_damage_judge_set_win:             0x080096b0,
        round_end_damage_judge_set_loss:            0x080096c4,
        round_end_damage_judge_set_draw:            0x080096c8,
        match_end_ret:                              0x08006958,
        battle_start_play_music_call:               0x080076b4,
    },
};

#[rustfmt::skip]
static A6BE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x080005b8,
        play_music_entry:                           0x080005c8,
        start_screen_logo_entry:                    0x0802b370,
        start_screen_fade_to_title:                 0x0802b460,
        title_intro_entry:                          0x0,
        title_intro_exit:                           0x0,
        title_wait_ret:                             0x08022122,
        title_confirm_continue:                     0x080222a4,
        title_continue_load_ret:                    0x08022098,
        field_open_start_menu:                      0x08004a24,
        comm_menu_init_ret:                         0x0803e0a2,
        comm_menu_start_netbattle:                  0x0803e726,
        comm_menu_settings_entry:                   0x0803ea10,
        round_start_ret:                            0x080059a8,
        round_end_set_win:                          0x0800946a,
        round_end_set_loss:                         0x08009472,
        round_end_damage_judge_set_win:             0x080096b0,
        round_end_damage_judge_set_loss:            0x080096c4,
        round_end_damage_judge_set_draw:            0x080096c8,
        match_end_ret:                              0x08006958,
        battle_start_play_music_call:               0x080076b4,
    },
};

#[rustfmt::skip]
static A3XJ_01: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x080005b8,
        play_music_entry:                           0x080005c8,
        start_screen_logo_entry:                    0x0802b848,
        start_screen_fade_to_title:                 0x0802b910,
        title_intro_entry:                          0x08021f98,
        title_intro_exit:                           0x080221fc,
        title_wait_ret:                             0x080220ca,
        title_confirm_continue:                     0x08022350,
        title_continue_load_ret:                    0x08022040,
        field_open_start_menu:                      0x080049b8,
        comm_menu_init_ret:                         0x0803e532,
        comm_menu_start_netbattle:                  0x0803ebb6,
        comm_menu_settings_entry:                   0x0803eea8,
        round_start_ret:                            0x0800593c,
        round_end_set_win:                          0x080093e6,
        round_end_set_loss:                         0x080093ee,
        round_end_damage_judge_set_win:             0x0800962c,
        round_end_damage_judge_set_loss:            0x08009640,
        round_end_damage_judge_set_draw:            0x08009644,
        match_end_ret:                              0x080068ec,
        battle_start_play_music_call:               0x08007648,
    },
};

#[rustfmt::skip]
static A6BJ_01: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        play_sfx_entry:                             0x080005b8,
        play_music_entry:                           0x080005c8,
        start_screen_logo_entry:                    0x0802b860,
        start_screen_fade_to_title:                 0x0802b928,
        title_intro_entry:                          0x08021fb0,
        title_intro_exit:                           0x08022214,
        title_wait_ret:                             0x080220e2,
        title_confirm_continue:                     0x08022368,
        title_continue_load_ret:                    0x08022058,
        field_open_start_menu:                      0x080049b8,
        comm_menu_init_ret:                         0x0803e54a,
        comm_menu_start_netbattle:                  0x0803ebce,
        comm_menu_settings_entry:                   0x0803eec0,
        round_start_ret:                            0x0800593c,
        round_end_set_win:                          0x080093e6,
        round_end_set_loss:                         0x080093ee,
        round_end_damage_judge_set_win:             0x0800962c,
        round_end_damage_judge_set_loss:            0x08009640,
        round_end_damage_judge_set_draw:            0x08009644,
        match_end_ret:                              0x080068ec,
        battle_start_play_music_call:               0x08007648,
    },
};

//! SIO-engine support: priming pokes and telemetry polls.
//!
//! BCC's link battle lives behind four confirmations: the title screen's
//! CONTINUE, the save-file picker, the PET's Transmit icon, and the
//! Normal/Random/Guest mode menu. The primer never presses a key — it
//! forces each of those decisions the way the game's own code does, by
//! making the "was this confirmed?" test come out true (and, where the
//! game reads a cursor, by writing the cursor value the player would
//! have picked). Every step then runs the game's real code: the save is
//! loaded by the picker's own copy loop, the Transmit module is entered
//! through the PET's own module switch, and the link handshake is the
//! game's own, running for real over the emulated cable.
//!
//! The state machine this walks (all offsets from the game's global
//! context block, [`EWRAMOffsets::ctx`]):
//!
//! - `ctx[0]` is the outer game state: 1 = intro, 3 = title, 11 = save
//!   picker, 8 = load, 12 = PET. `ctx[1]` is its substate.
//! - Inside the PET, `ctx[0x4699]` is the current module (0 = the icon
//!   menu, 1 = Program Deck, 6 = Save, 7 = Transmit) and `ctx[0x46b7]`
//!   is the icon the menu picked — the module switch reads it when a
//!   module returns nonzero.
//! - Inside Transmit, `ctx[0x46a4]` selects the submodule (0 = the mode
//!   menu, 1 = the connection) and `ctx[0x46a6]` is the mode-menu cursor
//!   (0 = Normal, 1 = Random, 2 = Guest).

use tango_backend_mgba::Trap;

/// The value the battle's own state byte (`ctx+0x46a8`) holds once the
/// battle is over and its fade has played.
const BATTLE_FINISHED: u8 = 2;

/// The game's RNG state: a u16 at `ctx + 8`. It only steps on demand
/// (menus idle without touching it), so the deterministic boot walk
/// leaves every core at the same value — and without reseeding, every
/// match would roll identically. The game's own connect handshake is
/// what keeps the two sides agreeing: the parent relays
/// `arena << 16 | rng16` in its drvD word and the child's own code
/// copies the rng16 into this halfword (US `0x08048B02`: it's the
/// `strh r0, [r5, #8]` with r5 = ctx — the same `r5 + 0x46a4/5` base
/// the surrounding transmit code uses).
const RNG_STATE: u32 = 0x8;

pub struct Pvp {
    offsets: &'static Offsets,
}

pub static PVP_A89E_00: Pvp = Pvp { offsets: &A89E_00 };
pub static PVP_A89J_00: Pvp = Pvp { offsets: &A89J_00 };

impl Pvp {
    /// The walk's progress bytes, for headless probe diagnostics:
    /// `[outer state, substate, inner substate, cursor, pet module,
    /// transmit submodule, transmit state, mode cursor]`.
    pub fn debug_menu_state(&self, core: &mut mgba::core::Core) -> [u8; 8] {
        let e = &self.offsets.ewram;
        [
            core.raw_read_8(e.ctx + 0x0, -1),
            core.raw_read_8(e.ctx + 0x1, -1),
            core.raw_read_8(e.ctx + 0x2, -1),
            core.raw_read_8(e.ctx + 0x3, -1),
            core.raw_read_8(e.ctx + 0x4699, -1),
            core.raw_read_8(e.ctx + 0x46a4, -1),
            core.raw_read_8(e.ctx + 0x46a5, -1),
            core.raw_read_8(e.ctx + 0x46a6, -1),
        ]
    }
}

impl tango_backend_mgba::GameSupport for Pvp {
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<mgba::core::Core>> {
        /// One tick's reading of the chip on screen acting.
        #[derive(Clone, Copy, PartialEq, Eq)]
        struct Acting {
            id: u16,
            /// The actor's fire count — what makes a volley of one chip
            /// read as one use per shot.
            shot: u16,
        }

        /// Chip-use detection for BCC's acting-chip contract: the
        /// reading is the chip on screen ACTING, `None` between actions
        /// — not a pick waiting to fire, so the use is when it ARRIVES.
        /// The loaded-chip games report a pick that sits in a cell
        /// until it fires, so a departure is the use there; BCC has no
        /// pick to sit — its turns resolve straight out of the deck —
        /// and the cell is set for as long as the action plays out, so
        /// waiting for the departure would mark the animation's end
        /// rather than the hit. A zero shot means the game is
        /// mid-update of its own counter, so the whole reading is
        /// ignored rather than let the flicker score.
        #[derive(Clone, Default)]
        struct ActingChipTracker {
            round: u32,
            prev: Option<Acting>,
        }
        impl ActingChipTracker {
            fn tick(&mut self, round: u32, reading: Option<Acting>, player: usize, events: &tango_match::telemetry::EventSink) {
                if self.round != round {
                    *self = Self { round, prev: None };
                }
                if reading == self.prev || reading.is_some_and(|c| c.shot == 0) {
                    return;
                }
                if let Some(c) = reading {
                    let fired = match self.prev {
                        None => true,
                        Some(p) => c.id != p.id || c.shot > p.shot,
                    };
                    if fired {
                        events.chip_used(player, c.id);
                    }
                }
                self.prev = reading;
            }
        }

        #[derive(Clone)]
        struct Poller {
            ewram: &'static EWRAMOffsets,
            player: usize,
            chips: ActingChipTracker,
        }
        impl tango_match::telemetry::CorePoller<mgba::core::Core> for Poller {
            fn poll(
                &mut self,
                core: &mut mgba::core::Core,
                events: &tango_match::telemetry::EventSink,
                round: u32,
            ) -> Option<tango_match::telemetry::CoreObs> {
                let ewram = self.ewram;
                // No battle to read until the round's HP is initialized —
                // the block is zeroed between battles.
                let hp = [
                    core.raw_read_16(ewram.battle_hp, -1) as u16,
                    core.raw_read_16(ewram.battle_hp + 2, -1) as u16,
                ];
                if hp == [0, 0] {
                    return None;
                }
                // The chip in play belongs to whoever is acting; both
                // cores see the same shared pair, so this core reports
                // only its OWN player's turns and the peer core answers
                // for the other's.
                let actor = core.raw_read_8(ewram.battle_actor, -1) as usize;
                let acting_id = core.raw_read_8(ewram.battle_actor_chip, -1) as u16;
                let fires = core.raw_read_8(
                    ewram.battle_fire_count + (actor.min(1) as u32) * BATTLE_PLAYER_STRIDE,
                    -1,
                ) as u16;
                // A navi's own attack is not a deck program and never
                // moves the fire counter, so it takes a fixed non-zero
                // shot marker and is scored when it takes the cell.
                const NAVI_ATTACK_SHOT: u16 = 0xf;
                let acting_chip = match acting_id {
                    0 => None,
                    id if crate::dataview::save::NAVI_CHIP_IDS.contains(&(id as usize)) => Some(Acting {
                        id,
                        shot: NAVI_ATTACK_SHOT,
                    }),
                    id => Some(Acting { id, shot: fires }),
                };
                self.chips.tick(
                    round,
                    acting_chip.filter(|_| actor == self.player),
                    self.player,
                    events,
                );
                Some(tango_match::telemetry::CoreObs {
                    // BCC's navis have no field to stand on: its battles
                    // are turn-based, not tiled.
                    units: std::array::from_fn(|p| tango_match::telemetry::UnitObs {
                        hp: hp[p],
                        tile: (0, 0),
                    }),
                    custom_self: core.raw_read_8(ewram.deck_confirm_wait, -1) != 0,
                })
            }
        }
        Box::new(Poller {
            ewram: &self.offsets.ewram,
            player,
            chips: Default::default(),
        })
    }

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
        let ctx = ewram.ctx;
        // Mode menu cursor: 0 = Normal, 1 = Random. Anything else the
        // host might send (there is no third netplay mode) walks Normal.
        let mode = if config.match_type.0 == 1 { 1u8 } else { 0u8 };

        // The game's own result bookkeeping, reported from core 0 only
        // (core 1's would be the same match seen from the other side).
        let sink = (player == 0).then(|| events.clone());
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

        // "The player confirmed": force the tested value true while this
        // core is still priming, so the game runs its own confirm branch.
        let confirm = |addr: u32| -> Trap {
            let primed = primed.clone();
            (
                addr,
                Box::new(move |core: &mut mgba::core::Core| {
                    if primed.is_set() {
                        return;
                    }
                    core.gba_mut().cpu_mut().set_gpr(0, 1);
                }),
            )
        };

        vec![
            // ----- the title screen -----
            // Its PRESS START wait, then its START/CONTINUE menu's
            // A-or-START test: both are "was a button pressed", and both
            // are answered yes so the title walks itself to the menu and
            // then off it.
            confirm(rom.title_start_test),
            confirm(rom.title_menu_confirm_test),
            // The menu's own confirm test, one substate later.
            confirm(rom.title_confirm_test),
            // The title's NEW GAME/CONTINUE cursor test: nonzero =
            // CONTINUE, the branch that walks to the save picker. Tango
            // always continues — a netplay session runs off the save the
            // host loaded, never a new game.
            confirm(rom.title_cursor_test),
            // ----- the save picker -----
            // Picking the highlighted file, then the confirm behind it.
            // The picker has already read both files out of SRAM by
            // these substates, and the confirm's branch copies the
            // selected one into the live block and enters the load
            // state. The slot stays the picker's own default (file 1),
            // which is the file Tango's save writer fills.
            confirm(rom.save_picker_select_test),
            confirm(rom.save_picker_confirm_test),
            confirm(rom.save_picker_load_test),
            // ----- the location screen -----
            // The loaded game drops the player on a location strip
            // ("you can battle or save here"). Its A test is forced;
            // the picker's own load code left the location cursor on
            // entry 0, which is the PET — the strip's own table maps
            // that to the PET game state.
            confirm(rom.location_confirm_test),
            confirm(rom.location_enter_test),
            // ----- the PET icon menu -----
            (
                // The module switch's test of the running module's return
                // value. Nonzero means "this module is done"; when the
                // module that finished is the icon menu (module 0), the
                // switch enters the module its icon byte names. Writing
                // that byte and forcing the test true is exactly what
                // picking the Transmit icon does.
                rom.pet_module_switch_test,
                {
                    let primed = primed.clone();
                    Box::new(move |core: &mut mgba::core::Core| {
                        if primed.is_set() {
                            return;
                        }
                        if core.raw_read_8(ctx + 0x4699, -1) != 0 {
                            return;
                        }
                        core.raw_write_8(ctx + 0x46b7, -1, 7);
                        core.gba_mut().cpu_mut().set_gpr(0, 1);
                    })
                },
            ),
            // ----- the Normal/Random mode menu -----
            (
                // The mode menu's key dispatch, reached once a frame with
                // `r4` = the key that was pressed (1 = A, 2 = B, 0x40/
                // 0x80 = up/down). Writing the mode cursor and handing it
                // an A runs the menu's own confirm, which switches the
                // Transmit module to its connection submodule — from
                // there the two games' real link handshake takes over.
                //
                // This is also where the RNG gets its per-match seed:
                // the last stop before the connect handshake, whose drvD
                // exchange relays the parent core's live state to the
                // child through the game's own protocol (see
                // [`RNG_STATE`]). Without this, the deterministic boot
                // walk would hand every match the same rolls. Zero is
                // avoided in case the generator can't escape it.
                rom.mode_menu_key_dispatch,
                {
                    let primed = primed.clone();
                    let rng = (config.core_rng_seed(player, 0) & 0xffff).max(1);
                    Box::new(move |core: &mut mgba::core::Core| {
                        if primed.is_set() {
                            return;
                        }
                        core.raw_write_8(ctx + 0x46a6, -1, mode);
                        core.raw_write_16(ctx + RNG_STATE, -1, rng as u16);
                        core.gba_mut().cpu_mut().set_gpr(4, 1);
                    })
                },
            ),
            // ----- the handoff and the round lifecycle -----
            (
                // The battle's own setup state, which runs for exactly
                // one frame at the head of each battle — the first thing
                // the game does once the two sides' handshake has agreed
                // on a match, and the frame the warp-in cutscene starts
                // on. Priming ends here, so the cutscene plays at real
                // speed instead of being fast-forwarded with the walk,
                // and each later battle reports its own round.
                rom.battle_setup_state,
                {
                    let primed = primed.clone();
                    let sink = sink.clone();
                    Box::new(move |_core: &mut mgba::core::Core| {
                        primed.set();
                        if let Some(sink) = &sink {
                            sink.round_started();
                        }
                    })
                },
            ),
            // ----- the round verdict -----
            // Each game records its own result into the save's
            // scoreboard (wins and losses are the two counters the mode
            // menu draws as "<n>B <m>W"). Core 0's local player is
            // player 0, so its win is P0's.
            verdict(rom.round_end_win, Outcome::P0Win),
            verdict(rom.round_end_loss, Outcome::P1Win),
            (
                // The battle module handing its "finished" state back as
                // its exit code: the post-battle fade has played out and
                // the module is done. Anchoring at the fade's end rather
                // than at the moment the battle is decided keeps the
                // wind-down on screen; anchoring any later would wait on
                // BCC's post-match save, which sits on an Overwrite?
                // prompt until a player answers it. The US build reaches
                // this only when the state is "finished" and the JP one
                // reads it every frame, so the check is explicit.
                // Trapped on both cores because a one-sided quit only
                // walks the quitter's game out; the telemetry store
                // dedups the second firing when both exit together.
                rom.battle_exit,
                {
                    let sink = events.clone();
                    Box::new(move |core: &mut mgba::core::Core| {
                        if core.raw_read_8(ctx + 0x46a8, -1) == BATTLE_FINISHED {
                            sink.match_ended();
                        }
                    })
                },
            ),
        ]
    }
}

#[derive(Clone, Copy)]
struct EWRAMOffsets {
    /// The game's global context block. Everything the primer reads or
    /// writes is an offset from here; the block also holds the live save
    /// (at +0x10) and the two loaded save files (at +0x1744).
    ctx: u32,
    /// The battling navis' HP, `u16` each in player order (P1 then P2).
    /// Both cores see both sides, so either one's poll is complete.
    /// Found by matching the values the battle header draws against
    /// EWRAM.
    ///
    /// This is the *resolved* HP: BCC computes a whole turn up front
    /// and then animates it, so this lands the moment the engine
    /// settles the exchange, ahead of the bar the player watches drain
    /// (that one lives at 0x02005170, one [`BATTLE_PLAYER_STRIDE`] per
    /// player). The resolved value is the truthful record of the
    /// battle, which is why the series plots it — it just means a chip
    /// mark, which can only be placed once the game names the chip,
    /// trails the drop it caused.
    battle_hp: u32,
    /// Nonzero exactly while this side's PROGRAM DECK is waiting for
    /// its player to confirm — BCC's equivalent of the BN games'
    /// battle-pausing custom screen. Verified by parking a lockstep
    /// pair with no input (it stays set for as long as the wait runs)
    /// against a pair that taps through (set only at the prompts).
    deck_confirm_wait: u32,
    /// Which player is acting this beat (0 = P1, 1 = P2), and the chip
    /// id they are acting with (0 = none, e.g. between actions). BCC
    /// resolves one navi at a time, so the pair reads as "whose turn
    /// is on screen, and with what" rather than a per-player loadout.
    /// Pinned by running a pair whose two decks held different chips
    /// and watching the id follow the actor byte.
    battle_actor: u32,
    battle_actor_chip: u32,
    /// P1's count of programs fired this battle; P2's sits one
    /// [`BATTLE_PLAYER_STRIDE`] along. It ticks on the hit itself — a
    /// deck firing M-Cannon three times walks 1, 2, 3 as each lands —
    /// so it is what tells repeat firings apart. The chip cell alone
    /// can't: it holds one id across the whole volley.
    battle_fire_count: u32,
}

#[derive(Clone, Copy)]
struct ROMOffsets {
    /// The title's START test, in the substate that blinks PRESS START
    /// while an attract timer runs down. Forced true while priming; the
    /// branch it guards opens the START/CONTINUE menu and — this is why
    /// the walk goes through it rather than around it — presets that
    /// menu's cursor to CONTINUE when the cart has a save.
    title_start_test: u32,
    /// The START/CONTINUE menu's A-or-START test. Forced true, which
    /// advances the title to its confirm substate.
    title_menu_confirm_test: u32,
    /// The title screen's confirm test — the `cmp` on the button-poll
    /// helper's result. Forced true while priming.
    title_confirm_test: u32,
    /// The title's NEW GAME/CONTINUE cursor test, two instructions
    /// later. Forced nonzero (= CONTINUE) while priming.
    title_cursor_test: u32,
    /// The save picker's A test, in the substate that lists the two save
    /// files. Forced true to pick the highlighted one.
    save_picker_select_test: u32,
    /// The save picker's confirm test, one substate later; its confirm
    /// branch advances to the substate that copies the chosen file into
    /// the live block and enters the load state.
    save_picker_confirm_test: u32,
    /// That last substate's own test, guarding the copy-and-load branch.
    save_picker_load_test: u32,
    /// The location strip's A test. Forced true to enter the location
    /// the cursor is on (the PET).
    location_confirm_test: u32,
    /// The strip's follow-up test, one inner substate later, that
    /// commits the entry and hands the game to the location's state.
    location_enter_test: u32,
    /// The PET module switch's test of the running module's return
    /// value, in the module dispatcher. Forced true (with the icon byte
    /// written) to enter the Transmit module.
    pet_module_switch_test: u32,
    /// The Transmit mode menu's key dispatch, with `r4` = the pressed
    /// key. Handed an A while priming.
    mode_menu_key_dispatch: u32,
    /// The battle submodule's setup state — one frame at the head of
    /// each battle, and the frame its warp-in cutscene begins on. The
    /// priming handoff and the round signal.
    battle_setup_state: u32,
    /// Where the game adds a win to its own scoreboard — the branch the
    /// battle's result code takes when this console won.
    round_end_win: u32,
    /// The loss counterpart of [`Self::round_end_win`].
    round_end_loss: u32,
    /// Where the battle module reads its own state to return it as an
    /// exit code, once the post-battle fade has played — BCC's match
    /// end, before the post-match save screen the game shows next.
    battle_exit: u32,
}

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static EWRAM_OFFSETS: EWRAMOffsets = EWRAMOffsets {
    ctx:                        0x020070f0,
    battle_hp:                  0x0200513c,
    deck_confirm_wait:          0x02005165,
    battle_actor:               0x02005156,
    battle_actor_chip:          0x02005157,
    battle_fire_count:          0x0200516f,
};

/// Distance between the two players' battle blocks.
const BATTLE_PLAYER_STRIDE: u32 = 0x14;

#[rustfmt::skip]
static A89E_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        title_start_test:           0x08026c60,
        title_menu_confirm_test:    0x08026d20,
        title_confirm_test:         0x08026d52,
        title_cursor_test:          0x08026d5e,
        save_picker_select_test:    0x080284b6,
        save_picker_confirm_test:   0x08028500,
        save_picker_load_test:      0x08028540,
        location_confirm_test:      0x08027b6c,
        location_enter_test:        0x08027bc0,
        pet_module_switch_test:     0x08035e76,
        mode_menu_key_dispatch:     0x080485a4,
        battle_setup_state:         0x08048cbc,
        round_end_win:              0x08034638,
        round_end_loss:             0x0803466a,
        battle_exit:                0x08048d8a,
    },
};

#[rustfmt::skip]
static A89J_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        title_start_test:           0x080269c0,
        title_menu_confirm_test:    0x08026a80,
        title_confirm_test:         0x08026ab2,
        title_cursor_test:          0x08026abe,
        save_picker_select_test:    0x0802820e,
        save_picker_confirm_test:   0x08028258,
        save_picker_load_test:      0x08028298,
        location_confirm_test:      0x080278c4,
        location_enter_test:        0x08027918,
        pet_module_switch_test:     0x08035bc6,
        mode_menu_key_dispatch:     0x080482b2,
        battle_setup_state:         0x080489c0,
        round_end_win:              0x08034368,
        round_end_loss:             0x0803439a,
        battle_exit:                0x08048b00,
    },
};

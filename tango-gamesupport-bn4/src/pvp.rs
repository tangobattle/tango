//! PvP-engine support: priming pokes and telemetry polls.
//!
//! Priming walks the game's own boot and comm-menu code with the human
//! sync points PC-redirected: the logo skip rides the start screen's
//! own fade-gated title transition, CONTINUE skips the title menu's
//! gates into its own confirm handler (cursor preset to the CONTINUE
//! row), and the NG+ prompt's dialog gate is PC-skipped to its confirm
//! store — every boot dispatcher byte written by ROM code. The comm
//! menu open at game load stays a poke (bn4's only ROM opener of the
//! netbattle applet is a post-battle resume, not a fresh open — see
//! that trap). At the comm menu's init return the primer arms
//! the comm machinery the direct open skipped (the vblank SIO-pump arm
//! flag + comm driver block that the START-menu opener initializes
//! organically — without the arm flag no multi transfer ever starts
//! and every link bring-up times out cold), and PC-redirects into the
//! comm switchboard's netbattle-select branch so the game itself runs
//! the netbattle conversation and walks the dispatcher to the mode
//! menu. The rngs are seeded identically from the match seed at game
//! load. The mode menu's and the vs prompt's A-button gates (whose
//! only job is to sync two humans — both cores arrive the same tick
//! under bit-identical priming) are PC-redirected to their own confirm
//! paths, the mode menu's with the cursor preset to the match type.
//! Everything else is the game's own code:
//! the vs-prompt confirm connects the link session, runs the ROM
//! settings generator (off the seeded rngs — identical on both cores)
//! and transmits the result; the wait states poll the exchange over
//! the emulated cable to completion; the accept handler takes the peer
//! settings and the player role from the real transfer's SIO multi id,
//! and battle init brings up the battle's own link session itself.
//! rng1 (each player's own draw stream) diverges per core at round
//! start.

use tango_backend_mgba::Trap;
use tango_gamesupport_common::telemetry::LoadedChip;

pub struct Pvp {
    offsets: &'static Offsets,
}

/// Byte distance from the vs prompt's A-button poll
/// (`ROMOffsets::vs_prompt_poll`) to its confirm path (the `bne` target
/// of its `cmp r0, #2` cancel check).
const VS_PROMPT_CONFIRM_DELTA: u32 = 0x3a;

/// Byte distance from the NG+ prompt's dialog-choice gate
/// (`ROMOffsets::ngplus_prompt_poll`) to its confirm store (the
/// `movs r0, #0x10; strh r0, [r5]` that walks the dispatcher to the
/// title exit). Identical on all four versions.
const NGPLUS_PROMPT_CONFIRM_DELTA: u32 = 0x20;

pub static PVP_B4WE_00: Pvp = Pvp { offsets: &B4WE_00 };
pub static PVP_B4BE_00: Pvp = Pvp { offsets: &B4BE_00 };
pub static PVP_B4WJ_01: Pvp = Pvp { offsets: &B4WJ_01 };
pub static PVP_B4BJ_01: Pvp = Pvp { offsets: &B4BJ_01 };

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

        let rom = &self.offsets.rom;
        let ewram = &self.offsets.ewram;
        let disable_bgm = config.disable_bgm;
        let match_type = config.match_type.0;
        // RNG contract: seed both rngs per core once, at save load —
        // exactly the situation the vanilla protocol is built for (two
        // cartridges never share RNG state on real hardware). The
        // vs-prompt confirm's REAL exchange (below) transmits the
        // master's generated settings to the slave, so agreement comes
        // from the protocol itself, and the players' draws differ
        // naturally from the distinct streams.
        let rng1 = config.core_rng_seed(player, 0);
        let rng2 = config.core_rng_seed(player, 1);
        // Redirect targets, copied out for the move closures (see each
        // trap below).
        let start_screen_title_transition = rom.start_screen_title_transition;
        let title_menu_confirm = rom.title_menu_confirm;
        let comm_menu_start_netbattle = rom.comm_menu_start_netbattle;
        // The vs prompt's confirm path (see its trap below).
        let confirm = rom.vs_prompt_poll + VS_PROMPT_CONFIRM_DELTA;
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
                // 0x10`, the same title handoff the logo's organic
                // timeout/keypress path lands. The trap re-fires each tick
                // while state 0 holds, so the gated store self-retries
                // until the game walks to the title bring-up (state 4)
                // itself.
                rom.start_screen_logo_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(start_screen_title_transition);
                }),
            ),
            (
                // The title menu's PUSH-START wait (title state 2, sub 4)
                // at its handler entry, re-fired each tick the wait holds.
                // Preset the menu cursor to the CONTINUE row (a human's
                // selection; the organic default with a valid save is row
                // 0, NEW GAME), then redirect to the full menu-confirm
                // handler (state 2 sub 0xc, `push {lr}` .. `pop {pc}`):
                // fade-gated, it reads the cursor and writes the dispatcher
                // walk itself — straight to the title exit, or to the NG+
                // menu first when the save carries the game-clear flag.
                // This replaces the trap-era block poke at the title init's
                // return; the init, the state-1 bring-up and the menu init
                // (which seeds the confirm's timer gate) now run for real.
                rom.title_pushstart_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_8(ewram.title_menu_control + 0x08, -1, 0x01);
                    core.gba_mut().cpu_mut().set_thumb_pc(title_menu_confirm);
                }),
            ),
            (
                // The NG+ ("Continue from where?") prompt's dialog-choice
                // gate, one instruction past its handler's `push {lr}`.
                // PC-skip to the handler's own confirm store
                // (`NGPLUS_PROMPT_CONFIRM_DELTA` bytes on): with the dialog
                // choice untaken it keeps the cursor's CONTINUE row — "From
                // save point" — and walks the dispatcher to the title exit
                // itself. Only ever reached on saves with the game-clear
                // flag; the NG+ menu init and its bring-up (skipped by the
                // trap era) run for real.
                rom.ngplus_prompt_poll,
                Box::new(move |core: &mut mgba::core::Core| {
                    let pc = core.gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + NGPLUS_PROMPT_CONFIRM_DELTA);
                }),
            ),
            (
                rom.game_load_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    // Seed the rngs (see the contract above), then open the
                    // comm menu from the overworld. This one stays a poke:
                    // bn4's only ROM opener of the netbattle comm applet
                    // (submenu[0] = 0x18) is the post-battle RESUME helper,
                    // which re-keys the applet at a resume sub-state
                    // ([3] = 0x1c) that assumes a battle already ran — it
                    // walks the dispatcher into a post-battle-only state,
                    // not a fresh netbattle open. The overworld's own comm
                    // access goes through the interactive START-menu
                    // overlay (row navigation we can't drive), so there is
                    // no single organic entry that fresh-opens the applet at
                    // [1] = 0, [3] = 0 the way this poke does.
                    core.raw_write_32(ewram.rng1_state, -1, rng1);
                    core.raw_write_32(ewram.rng2_state, -1, rng2);
                    core.raw_write_8(ewram.subsystem_control, -1, 0x1c);
                    core.raw_write_8(ewram.submenu_control + 0x0, -1, 0x18);
                    core.raw_write_8(ewram.submenu_control + 0x1, -1, 0x00);
                    core.raw_write_8(ewram.submenu_control + 0x2, -1, 0x00);
                    core.raw_write_8(ewram.submenu_control + 0x3, -1, 0x00);
                }),
            ),
            (
                // The comm menu's init return — the init handler's terminal
                // `pop {r4, r6, pc}`. Two jobs.
                //
                // First, arm the comm machinery the boot fast-path skipped.
                // Organically, opening the START menu runs an overworld
                // handler (0x8005400 region in B4WE) that initializes the
                // comm driver block and sets the vblank SIO-pump arm flag,
                // the flag the per-vblank hook checks before running the SIO
                // library's transfer pump (0x8111118 in B4WE); it arms both
                // through its own helper (0x802da34, its ONLY caller — the
                // comm applet's own netbattle route never arms the pump, so
                // this stays a poke). Poking the comm menu open from
                // game-load skips that handler, the pump stays disarmed, no
                // multi transfer ever STARTS, and every later link bring-up
                // (settings exchange, battle init) times out cold. Poke the
                // helper's essence: zero + mask-init the driver block, set
                // its "comm initialized" bit, and arm the pump. (The helper
                // also zeroes a menu-side scratch block, but a fresh boot's
                // is still zero — and at this trap's time the active submenu
                // state lives there, so we must NOT clear it.)
                //
                // Second, instead of popping, PC-redirect into the comm
                // switchboard's netbattle-select branch (the `bne` target of
                // the comm-top poll's `cmp r0, #2` cancel check — the code
                // the menu runs when the player picks link battle). It sets
                // the netbattle conversation running (variant chosen from the
                // link-cable state, sub-state 8), which the game drives over
                // the emulated cable and completes to the mode menu itself —
                // no dispatcher bytes poked. The branch lives inside the
                // comm-top poll, a `push {r4, r6, lr}` / `pop {r4, r6, pc}`
                // function, so the init handler's own saved {r4, r6, lr}
                // feeds the branch's terminating `pop {r4, r6, pc}` and
                // control returns to the dispatcher cleanly.
                rom.comm_menu_init_ret,
                Box::new(move |core: &mut mgba::core::Core| {
                    for i in 0..0xc {
                        core.raw_write_8(ewram.comm_driver_block + i, -1, 0);
                    }
                    core.raw_write_16(ewram.comm_driver_block + 6, -1, 0xffe0);
                    core.raw_write_8(ewram.comm_driver_block + 8, -1, 0x01);
                    core.raw_write_32(ewram.sio_pump_arm, -1, 1);
                    core.gba_mut().cpu_mut().set_thumb_pc(comm_menu_start_netbattle);
                }),
            ),
            (
                // The netbattle mode menu's handler. Preset the menu cursor
                // to the match type (0 = single battle, 1 = triple battle;
                // the value a human would have picked here), then PC-redirect
                // past its A-button gate to the confirm code (+0x52,
                // identical on all four versions), which reads the cursor as
                // the settings-generator argument and walks the dispatcher to
                // the vs prompt — all organically. The cursor write lives
                // here, the same trap that skips the A-gate, so it lands on
                // the halfword the confirm is about to read (`ldrh [r5,
                // #0x14]`) rather than being poked in from afar. (The trap
                // engine instead redirected +0x62 into the cursor==2 branch
                // and jumped the dispatcher straight to battle init; that
                // branch's exchange is not the two-player netbattle route,
                // and the jump skips the states that establish the link
                // session, so under a real cable the battle bring-up
                // cold-starts.)
                rom.comm_menu_settings_entry,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.raw_write_16(ewram.submenu_control + 0x14, -1, match_type as u16);
                    let pc = core.gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 0x52);
                }),
            ),
            (
                // The vs prompt ("(20,04)"): PC-redirect its A-button gate
                // to its confirm path. With the prompt cursor at its init
                // position the confirm connects the link session, runs the
                // ROM settings generator and transmits the result; the
                // wait states that follow poll the exchange to completion
                // and the accept handler slots in the peer settings (slave
                // side) and the real SIO multi id as the player role, then
                // walks to battle init. No is_p2 forcing is needed: under
                // the lockstep pair the hardware multi id IS the core
                // index.
                rom.vs_prompt_poll,
                Box::new(move |core: &mut mgba::core::Core| {
                    core.gba_mut().cpu_mut().set_thumb_pc(confirm);
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
                // screen (see `EWRAMOffsets::custom_flags`).
                let custom_self = core.raw_read_8(self.ewram.custom_flags + self.player as u32, -1) == 4;
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
    /// Title menu jump table control.
    title_menu_control: u32,

    /// Subsystem control.
    subsystem_control: u32,

    /// START menu submenu (e.g. comm menu) jump table control.
    submenu_control: u32,

    /// Local RNG state. Doesn't need to be synced.
    rng1_state: u32,

    /// Shared RNG state. Must be synced.
    rng2_state: u32,

    /// The first in-battle unit's [`RawUnit`] record; the second
    /// follows immediately. This is the record the game itself hands
    /// around -- both slots' addresses sit in its own unit pointer
    /// table (0x02035854 on this version), which is how the base was
    /// pinned rather than guessed from a mid-struct anchor.
    unit: u32,
    /// Player 0's selected-chip block; player 1's is 0x50 beyond. Layout:
    /// +0 u16 chips fired since the last selection landed, +2 u16 ids[6]
    /// (0xFFFF = empty slot). The selection lands AT the shared custom
    /// close; the loaded chip is ids[fired]. Indexed by absolute player,
    /// NOT by unit slot (verified cross-perspective: each player's block
    /// stays put in both cores). Derived empirically from the golden
    /// replays -- note picks aren't always folder chips (dark chips are
    /// offered off-folder).
    chip_blocks: u32,

    /// Per-player custom-screen flag bytes: one byte per player at +0/+1,
    /// a single known value while that player's chip-select is open and 0
    /// (or another mode value) otherwise. Same shape as bn5/bn6's
    /// battle_state flags; derived empirically from the golden replays,
    /// identical across regions and both perspectives. The value lives in
    /// the poller.
    custom_flags: u32,

    /// The comm subsystem's driver state, zero-initialized by the
    /// START-menu opener organically: the byte at +8 is its "comm
    /// initialized" flag bit and the halfword at +6 its event mask.
    /// Each ROM carries the address as a literal; verified present in
    /// B4WE/B4BE 00 and B4WJ/B4BJ 01.
    comm_driver_block: u32,
    /// The flag the per-vblank hook checks before running the SIO
    /// library's transfer pump; set by the START-menu opener alongside
    /// `comm_driver_block`.
    sio_pump_arm: u32,
}

#[derive(Clone, Copy)]
struct ROMOffsets {
    /// The start screen's state-0 (CAPCOM logo) handler entry, from the
    /// applet's jump table — r5 = title_menu_control is
    /// dispatcher-loaded here. Trapped to redirect into
    /// `start_screen_title_transition`.
    start_screen_logo_entry: u32,

    /// The start screen's state-3 transition handler: fade-gated
    /// `[r5] = 0x10`, the applet's own walk to the title bring-up. A
    /// full `push {lr}` .. `pop {pc}` body — `start_screen_logo_entry`'s
    /// trap redirects here.
    start_screen_title_transition: u32,

    /// The title menu's PUSH-START wait handler entry (title state 2,
    /// sub-state 4), from the state-2 sub-table. Trapped to preset the
    /// menu cursor and redirect into `title_menu_confirm`.
    title_pushstart_entry: u32,

    /// The title menu's confirm handler (title state 2, sub-state 0xc):
    /// fade-gated, reads the menu cursor at `title_menu_control + 8`
    /// and walks the dispatcher to the title exit (via the NG+ menu on
    /// game-clear saves) itself. A full `push {lr}` .. `pop {pc}` body —
    /// `title_pushstart_entry`'s trap redirects here.
    title_menu_confirm: u32,

    /// The NG+ prompt's dialog-choice gate: one instruction past the
    /// NG+ sub-state-8 handler's `push {lr}`, at its `movs r0, #0x80`
    /// dialog poll. The trap PC-skips `NGPLUS_PROMPT_CONFIRM_DELTA`
    /// bytes to the handler's own confirm store.
    ngplus_prompt_poll: u32,

    /// This is immediately after game initialization is complete: that is, the internal state is set correctly.
    ///
    /// Here, Tango seeds the rngs from the match seed.
    game_load_ret: u32,

    /// This hooks the point after the battle start routine is complete.
    ///
    /// Tango initializes its own battle tracking state at this point.
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

    /// This is the entry point to the comm menu.
    ///
    /// Here, Tango redirects into `comm_menu_start_netbattle`.
    comm_menu_init_ret: u32,

    /// The comm switchboard's netbattle-select branch: the `bne` target
    /// of the comm-top poll's `cmp r0, #2` cancel check — the code the
    /// menu runs when the player picks link battle. Sets the netbattle
    /// conversation running (its variant chosen from the link-cable
    /// state), which the game drives to the mode menu itself. Lives
    /// inside the comm-top poll's `push {r4, r6, lr}` / `pop {r4, r6,
    /// pc}` frame, matching the init handler's, so `comm_menu_init_ret`'s
    /// trap PC-redirects here and the init handler's saved regs feed the
    /// branch's terminating pop.
    comm_menu_start_netbattle: u32,

    /// Inside the in-game settings-handler function, at the
    /// `ldrh r0, [r5, #0x14]; ldrh r1, [r5, #0x16]; cmp` sequence. The
    /// trap pre-seeds rng1/rng2 from the synced match RNG, advances the
    /// submenu substate, then PC-redirects +0x62 bytes — past the
    /// function's SIO/button check *and* its own `[2]=0xc; [3]=0`
    /// writes (which would undo that substate) — landing on the
    /// `ldrh r0, [r5, #0x14]; lsls; adds` that feeds the `bl` to the
    /// ROM settings generator, so the generator path runs
    /// unconditionally and writes submenu_control[0x11] (settings) and
    /// [0x2c] (background) itself. Delta 0x62 is identical across
    /// B4BE/B4WE/B4BJ_01/B4WJ_01.
    comm_menu_settings_entry: u32,

    /// The netbattle vs prompt's A-button poll — the `movs r0, #0;
    /// bl button_poll` inside the comm dispatcher's ([1],[2]) = (20,04)
    /// handler. The primer PC-redirects it to the handler's own confirm
    /// path (`VS_PROMPT_CONFIRM_DELTA` bytes on, identical across all
    /// four versions), the second and last human sync point skipped on
    /// the route.
    vs_prompt_poll: u32,

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
    title_menu_control:     0x0200b220,
    subsystem_control:      0x0200a7e0,
    submenu_control:        0x0200a450,
    rng1_state:             0x020015d4,
    rng2_state:             0x02001790,
    unit:                   0x0203b180,
    chip_blocks:            0x02035cb0,
    custom_flags:           0x02036440,
    comm_driver_block:      0x0200f770,
    sio_pump_arm:           0x0200a714,
};

#[derive(Clone, Copy)]
struct Offsets {
    rom: ROMOffsets,
    ewram: EWRAMOffsets,
}

#[rustfmt::skip]
static B4BE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0802d7b4,
        start_screen_title_transition:          0x0802d8ec,
        title_pushstart_entry:                  0x08025470,
        title_menu_confirm:                     0x080254e0,
        ngplus_prompt_poll:                     0x080255da,
        game_load_ret:                          0x08004996,
        round_start_ret:                        0x08006710,
        round_end_set_win:                      0x08007130,
        round_end_set_loss:                     0x08007144,
        round_end_damage_judge_set_win:         0x080073da,
        round_end_damage_judge_set_loss:        0x080073ee,
        round_end_damage_judge_set_draw:        0x080073f4,
        comm_menu_init_ret:                     0x0803956a,
        comm_menu_start_netbattle:              0x080396c8,
        comm_menu_settings_entry:               0x08039756,
        vs_prompt_poll:                         0x0803a32e,
        match_end_ret:                          0x08004f68,
        battle_start_play_music_call:               0x080074bc,
    },
};

#[rustfmt::skip]
static B4WE_00: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0802d7b0,
        start_screen_title_transition:          0x0802d8e8,
        title_pushstart_entry:                  0x0802546c,
        title_menu_confirm:                     0x080254dc,
        ngplus_prompt_poll:                     0x080255d6,
        game_load_ret:                          0x08004996,
        round_start_ret:                        0x08006710,
        round_end_set_win:                      0x08007130,
        round_end_set_loss:                     0x08007144,
        round_end_damage_judge_set_win:         0x080073da,
        round_end_damage_judge_set_loss:        0x080073ee,
        round_end_damage_judge_set_draw:        0x080073f4,
        comm_menu_init_ret:                     0x08039562,
        comm_menu_start_netbattle:              0x080396c0,
        comm_menu_settings_entry:               0x0803974e,
        vs_prompt_poll:                         0x0803a326,
        match_end_ret:                          0x08004f68,
        battle_start_play_music_call:               0x080074bc,
    },
};

#[rustfmt::skip]
static B4BJ_01: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0802d704,
        start_screen_title_transition:          0x0802d7fc,
        title_pushstart_entry:                  0x080253b4,
        title_menu_confirm:                     0x08025424,
        ngplus_prompt_poll:                     0x0802551e,
        game_load_ret:                          0x08004976,
        round_start_ret:                        0x080066f0,
        round_end_set_win:                      0x08007108,
        round_end_set_loss:                     0x0800711c,
        round_end_damage_judge_set_win:         0x080073b2,
        round_end_damage_judge_set_loss:        0x080073c6,
        round_end_damage_judge_set_draw:        0x080073cc,
        comm_menu_init_ret:                     0x0803947e,
        comm_menu_start_netbattle:              0x080395dc,
        comm_menu_settings_entry:               0x0803966a,
        vs_prompt_poll:                         0x0803a242,
        match_end_ret:                          0x08004f48,
        battle_start_play_music_call:               0x08007494,
    },
};

#[rustfmt::skip]
static B4WJ_01: Offsets = Offsets {
    ewram: EWRAM_OFFSETS,
    rom: ROMOffsets {
        start_screen_logo_entry:                0x0802d700,
        start_screen_title_transition:          0x0802d7f8,
        title_pushstart_entry:                  0x080253b0,
        title_menu_confirm:                     0x08025420,
        ngplus_prompt_poll:                     0x0802551a,
        game_load_ret:                          0x08004976,
        round_start_ret:                        0x080066f0,
        round_end_set_win:                      0x08007108,
        round_end_set_loss:                     0x0800711c,
        round_end_damage_judge_set_win:         0x080073b2,
        round_end_damage_judge_set_loss:        0x080073c6,
        round_end_damage_judge_set_draw:        0x080073cc,
        comm_menu_init_ret:                     0x08039476,
        comm_menu_start_netbattle:              0x080395d4,
        comm_menu_settings_entry:               0x08039662,
        vs_prompt_poll:                         0x0803a23a,
        match_end_ret:                          0x08004f48,
        battle_start_play_music_call:               0x08007494,
    },
};

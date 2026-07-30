//! PvP-engine support: the priming walk.
//!
//! Nothing here touches the link protocol — the two consoles negotiate
//! for real over emulated local wireless. Priming is PC-redirects into
//! the game's own transition code, exactly as the GBA crates' is: every
//! menu state, every sfx, every byte of the save is written by the game
//! itself, and nothing below presses a button, touches the screen, or
//! is timed against the clock.
//!
//! It runs in two halves, which is what the trap list below is ordered
//! by. **Boot to the Network board** redirects the game's key checks:
//! the logo's dwell, the title's arming delay, the save select and its
//! CONTINUE submenu, the field's START dispatch, the START menu's
//! Network entry, and the script engine's waits around the save the
//! board is gated behind. **The board to the battle** cannot do that,
//! because from there the game stops asking "was this key pressed" and
//! starts polling touch widgets — so it redirects those instead. Each
//! of those screens reads a **hit code**, the halfword saying which
//! button was hit, behind a gate asking whether a touch event exists at
//! all; jumping that gate into the branch the hit would have taken
//! answers the screen without fabricating an event.
//!
//! Those screens are fewer than they look. The mode pick, the
//! Practice/Real Thing pick and both connect prompts are all one
//! two-button chooser, so three sites carry four screens; what varies
//! is only which button, on which screen, and each answer is gated on
//! the screen object's own sub-state rather than scheduled.
//!
//! The pair is symmetric, so the seats are assigned rather than
//! negotiated: **console 0 takes the game's host seat and console 1
//! joins it**. Both peers walk both consoles, so both agree without
//! asking and nothing has to cross the wire.
//!
//! Both releases walk the identical route — one cart, two builds — but
//! the addresses do not, so each registration closes over its build's
//! [`priming::Layout`]. (The walker itself is that type's `traps`.)

use tango_backend_melonds::{Link, Nds};

/// The game's engine support: the priming walk over one build's
/// addresses, plus the battle telemetry reader.
pub struct Pvp {
    layout: &'static priming::Layout,
    /// The battle unit block: two [`UNIT_STRIDE`]-spaced unit records,
    /// **slot**-ordered — which player owns which slot swaps between
    /// the rounds of a Triple Battle, exactly as on the GBA family, so
    /// every read resolves through each record's owner byte. The
    /// record is the GBA BN5 unit record with four bytes of growth:
    /// the fields telemetry reads sit at the same offsets (tile
    /// `+0x12`, owner `+0x16`, hp `+0x24`, max hp `+0x26`, loaded chip
    /// `+0x2a`), verified against a recorded match on both builds.
    /// Zeroed outside a live battle — the round intro, the
    /// between-rounds intermission, the post-match screens — which is
    /// what makes the owner check below the liveness gate.
    unit: u32,
    /// Player 0's custom (chip-select) flag; player 1's is `+0x20`.
    /// Per-player, exactly like GBA bn5's `battle_state + 0x14 +
    /// player`: 4 while that player's screen is up (briefly 5 right
    /// after a mid-battle open), 0 once THEY commit — and a player who
    /// never commits is force-closed by the screen's countdown, their
    /// byte dropping about 20 ticks before the selections land in the
    /// chip cells. Shared simulation state (both consoles hold both
    /// players' bytes and agree). Found by elimination scan over
    /// recorded matches, the pair separated by a recording where the
    /// two players commit ~90 ticks apart — on both builds, across
    /// every custom episode in the July 2026 recordings.
    custom: u32,
    /// The game's comm-result globals: a small struct the battle loop's
    /// own result-deciding code reports into through a suite of tiny
    /// accessors (US `0x2097ca4..=0x2097d54`, found by elimination scan
    /// over forced KOs and confirmed in the disassembly — the setter's
    /// one caller also mirrors the value into a battle object).
    ///
    /// Byte `+0` is the battle loop's end sub-state: 0 until the round's
    /// result is decided, nonzero from the KO through the round's
    /// teardown, cleared by the next round's setup — the value varies
    /// with mode (0x0a/0x0b Single, 0x05 Triple), so only its liveness
    /// speaks. Byte `+1` is the verdict, in the console's OWN
    /// perspective (each console mirrors the other's): 1 = this side
    /// won, 2 = lost, 3 = the judge's draw, 4/5/7 = the comm-abnormal
    /// exits. It is NOT cleared between the rounds of a Triple match,
    /// which is why the `+0` gate is the read's precondition.
    result: u32,
    /// Player 0's selected-chip block; player 1's is 0x50 beyond. The
    /// GBA family's hand block, carried over by the port at the same
    /// shape as bn4/bn5/bn6's: +0 u16 chips fired since the last
    /// selection landed, +2 u16 ids[6] (0xFFFF = empty slot); the
    /// loaded chip is ids[fired], agreeing with the unit record's
    /// `+0x2a` cell at every live tick. Indexed by absolute player, NOT
    /// by unit slot. Found July 2026 by whole-RAM elimination scan
    /// against the cell over replayed matches (hand_probe recipe), zero
    /// mismatches on both builds, the fired cursor sweeping 0..=3.
    chips: u32,
}

/// What the US registration's
/// [`DsBackend`](tango_backend_melonds::DsBackend) closes over.
pub static US: Pvp = Pvp {
    layout: &priming::US,
    unit: 0x022d_6498,
    custom: 0x0216_0992,
    result: 0x0216_f738,
    chips: 0x021b_8af8,
};

/// What the JP registration's
/// [`DsBackend`](tango_backend_melonds::DsBackend) closes over.
pub static JP: Pvp = Pvp {
    layout: &priming::JP,
    unit: 0x022c_ee18,
    custom: 0x0215_9732,
    result: 0x0216_84d8,
    chips: 0x021b_1848,
};

/// The unit record's size, which is also the second slot's offset.
const UNIT_STRIDE: u32 = 0xdc;

/// The modules the game runs once it has left the link session: the
/// Network board (the id the priming walk itself pivots on, on both
/// builds) and the Net Battle screen the exit passes through one tick
/// ahead of it. Which module a BATTLE runs as varies with the save's
/// provenance (see `BOARD` in [`priming`]), but the phase read never
/// asks: a live unit block means a round regardless of module, and
/// these two ids only count with the block dead — a state the
/// between-rounds interlude spends on its transition modules, never
/// here, across every recorded end and interlude on both builds.
const MENUS: [u32; 2] = [0x17, 0x18];

/// Whether the unit block holds two live player units — one owner byte
/// per slot, reading exactly `{0, 1}`. The game zeroes the block
/// outside a live battle (round intros, the interlude, the post-match
/// menus), which makes this the battle-liveness gate.
fn units_live(ram: &[u8], unit: u32) -> bool {
    let mask = ram.len() - 1;
    let owner = |slot: u32| ram[((unit + slot * UNIT_STRIDE + 0x16) as usize - 0x0200_0000) & mask];
    matches!((owner(0), owner(1)), (0, 1) | (1, 0))
}

impl tango_backend_melonds::GameSupport for Pvp {
    fn prime(
        &self,
        link: &mut Link,
        match_type: (u8, u8),
        session_payloads: [Option<&dyn tango_match::SessionPayload>; 2],
        rng_seed: [u8; 16],
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<(), tango_match::Error> {
        self.layout.walk(link, match_type, session_payloads, rng_seed, cancel)
    }

    /// This game's payload type is
    /// [`PlayedFile`](crate::dataview::save::PlayedFile): one byte,
    /// the file-select slot the committing save view was on.
    fn parse_session_payload(&self, bytes: &[u8]) -> Result<tango_match::BoxedSessionPayload, tango_match::Error> {
        match *bytes {
            [slot] => Ok(Box::new(crate::dataview::save::PlayedFile(slot))),
            _ => Err(tango_match::Error::MalformedSessionPayload),
        }
    }

    /// Battle telemetry for one console: both units' HP and tile,
    /// absolute player order, plus this console's own player's chip
    /// fires into the sink. The battle sim is both-sided — each console
    /// simulates both units under the wireless lockstep — so the two
    /// consoles read identical values, and `player` picks which side's
    /// custom flag answers [`custom_self`] and whose fires this console
    /// reports. `None` until the slots hold two live player units.
    ///
    /// Console 0's poller additionally carries the game's lifecycle as
    /// RAM facts (this engine's stand-in for the mgba families' trap
    /// anchors): a battle is live while the unit block holds two owned
    /// units — chip cut-ins, effect sub-modules and the pause screen
    /// all keep it — and the match is over once the block is dead with
    /// the module word on the post-link menus. Everything else is the
    /// space between rounds. Verified against a recorded match played
    /// to its natural end: the game reaches its menus ~80 ticks before
    /// it powers the wireless down, so the match-end report lands right
    /// as the battle screens leave.
    ///
    /// [`custom_self`]: tango_match::telemetry::CoreObs::custom_self
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<Nds>> {
        use tango_gamesupport_common::telemetry::{HandChipTracker, LoadedChip};
        use tango_match::telemetry::{CoreObs, EventSink, Outcome, UnitObs};

        /// Console 0's lifecycle watch: the phase and verdict LEVELS,
        /// reported as edges against last tick's readings. The verdict
        /// comes from the comm-result globals the battle loop's own KO
        /// and judge paths report into (see [`Pvp::result`]): no
        /// verdict until the end sub-state at `+0` stands, then `+1`
        /// read through console 0's perspective — its local player is
        /// player 0, the game's host seat, exactly the mgba families'
        /// core-0 convention. The comm-abnormal values (a peer
        /// vanishing mid-round) deliberately read as no verdict: the
        /// round genuinely never got one. Both consoles hold mirrored
        /// copies and agree at every settled tick — KO-forge verified
        /// on both builds, both outcomes.
        struct LifecycleWatch {
            unit: u32,
            module: u32,
            result: u32,
            /// Last tick's phase: 0 = nothing seen yet, 1 = a round is
            /// live, 2 = between rounds, 3 = the post-link menus.
            prev_phase: u8,
            /// Last tick's standing verdict (0 = none): reporting on
            /// its edges is what stamps one outcome per round unless
            /// the level drops and stands again.
            prev_verdict: u8,
        }
        impl LifecycleWatch {
            fn tick(&mut self, nds: &mut Nds, events: &EventSink) {
                let phase: u8 = if units_live(nds.main_ram(), self.unit) {
                    1
                } else if MENUS.contains(&nds.read32(self.module)) {
                    3
                } else {
                    2
                };
                let verdict = if nds.read8(self.result) == 0 {
                    0
                } else {
                    match nds.read8(self.result + 1) {
                        v @ 1..=3 => v,
                        _ => 0,
                    }
                };
                if phase == 1 {
                    if self.prev_phase != 1 {
                        events.round_started();
                    }
                    if verdict != 0 && verdict != self.prev_verdict {
                        events.round_outcome(match verdict {
                            1 => Outcome::P0Win,
                            2 => Outcome::P1Win,
                            _ => Outcome::Draw,
                        });
                    }
                } else {
                    if self.prev_phase == 1 {
                        events.round_ended();
                    }
                    if phase == 3 && self.prev_phase != 3 {
                        events.match_ended();
                    }
                }
                self.prev_phase = phase;
                self.prev_verdict = verdict;
            }
        }

        struct Poller {
            unit: u32,
            /// This player's own custom flag address (see [`Pvp::custom`]).
            custom: u32,
            /// This player's own hand block (see [`Pvp::chips`]).
            chip_block: u32,
            player: usize,
            chips: HandChipTracker,
            /// Console 0 only, like the mgba families' round anchors.
            lifecycle: Option<LifecycleWatch>,
        }
        impl tango_match::telemetry::CorePoller<Nds> for Poller {
            fn poll(&mut self, nds: &mut Nds, events: &EventSink, round: u32) -> Option<CoreObs> {
                // The lifecycle reads first: the phases it watches are
                // exactly the ones where the unit block is dead and the
                // battle read below bails out.
                if let Some(lc) = &mut self.lifecycle {
                    lc.tick(nds, events);
                }
                let ram = nds.main_ram();
                let mask = ram.len() - 1;
                let read8 = |addr: u32| ram[(addr as usize - 0x0200_0000) & mask];
                let read16 = |addr: u32| u16::from_le_bytes([read8(addr), read8(addr + 1)]);

                let mut units = [None, None];
                for slot in 0..2 {
                    let base = self.unit + slot * UNIT_STRIDE;
                    let owner = read8(base + 0x16) as usize;
                    *units.get_mut(owner)? = Some(UnitObs {
                        hp: read16(base + 0x24),
                        tile: (read8(base + 0x12), read8(base + 0x13)),
                    });
                }
                let units = [units[0]?, units[1]?];
                // This player's own flag (see [`Pvp::custom`]) — the
                // span ends at their own commit, exactly as on GBA
                // bn5; 5 is the screen's brief just-opened sub-state.
                let custom_self = matches!(read8(self.custom), 4 | 5);
                // This console's own player's chip fires, off its hand
                // block's fired counter (see [`Pvp::chips`]) — the same
                // cursor contract as the GBA family's.
                let fired = read16(self.chip_block);
                let reading = (fired < 6)
                    .then(|| read16(self.chip_block + 2 + 2 * fired as u32))
                    .filter(|&id| id != 0 && id <= 0x0fff)
                    .map(|id| LoadedChip { id, fires: fired });
                self.chips
                    .tick(round, reading, custom_self, units[self.player].hp, self.player, events);
                Some(CoreObs { units, custom_self })
            }
            fn save(&self) -> tango_match::telemetry::Scratch {
                tango_match::telemetry::Scratch::new((
                    self.chips.clone(),
                    self.lifecycle.as_ref().map(|lc| (lc.prev_phase, lc.prev_verdict)),
                ))
            }
            fn restore(&mut self, scratch: &tango_match::telemetry::Scratch) {
                let (chips, watch) = scratch
                    .get::<(HandChipTracker, Option<(u8, u8)>)>()
                    .cloned()
                    .unwrap_or_default();
                self.chips = chips;
                if let Some(lc) = &mut self.lifecycle {
                    (lc.prev_phase, lc.prev_verdict) = watch.unwrap_or_default();
                }
            }
        }

        Box::new(Poller {
            unit: self.unit,
            custom: self.custom + 0x20 * player as u32,
            chip_block: self.chips + 0x50 * player as u32,
            player,
            chips: Default::default(),
            lifecycle: (player == 0).then(|| LifecycleWatch {
                unit: self.unit,
                module: self.layout.module_word(),
                result: self.result,
                prev_phase: 0,
                prev_verdict: 0,
            }),
        })
    }
}

pub mod priming {
    use tango_backend_melonds::{Link, Nds};
    use tango_match::{HostInput, Link as _};

    /// One build's addresses. Everything *about* the walk — what each
    /// answer means, how many confirms it takes, how long it runs — is
    /// shared; only these move between builds.
    ///
    /// The ARM7 sites are not in here: the two builds' ARM7 binaries
    /// are byte-identical, so those addresses hold for both.
    pub struct Layout {
        /// Which build this is, for log lines.
        tag: &'static str,
        code: CodeOffsets,
        ram: RAMOffsets,
    }

    /// Sites in the ARM9's code: what the walk traps, and the branches
    /// it redirects into.
    ///
    /// Each `*_gate` is where the game reads input and decides, and the
    /// address under it is the branch the press or touch it was looking
    /// for would have taken. Both sides of a pair sit in the same
    /// function, so the stack is untouched and whatever the handler had
    /// already loaded is still loaded — the redirect only answers the
    /// question the check was about to ask. ARM code except where
    /// noted; a jump keeps whatever instruction set it lands in.
    struct CodeOffsets {
        /// The Capcom logo's dwell, and where a spent counter lands.
        logo_hold: u32,
        logo_expired: u32,
        /// The title screen's arming delay (**Thumb**), and the
        /// press-START check it guards.
        title_arming_gate: u32,
        title_press_check: u32,
        /// The save select's test-A, and its confirm branch.
        save_select_gate: u32,
        save_select_confirm: u32,
        /// The CONTINUE / NEW GAME submenu's test-A, and the confirm its
        /// cursor dispatch hangs off.
        continue_gate: u32,
        continue_confirm: u32,
        /// The overworld field dispatcher's START compare, and the
        /// branch that opens the START menu.
        field_start_gate: u32,
        field_start_menu_open: u32,
        /// The START menu's read of the pad, and the accepted path of
        /// its Network entry.
        start_menu_gate: u32,
        start_menu_network: u32,
        /// The script engine's timed waits, and the branch a spent
        /// timer takes.
        script_timer_gate: u32,
        script_timer_expired: u32,
        /// Opcode 0xE7 — the wait holding a message box open until it
        /// is dismissed — and the branch a dismissal takes.
        script_box_gate: u32,
        script_box_dismissed: u32,
        /// Where the main loop has just refreshed the game's own pad
        /// state, one instruction after the call that fills it. The one
        /// site the walk writes at rather than jumps from.
        pad_refresh_ret: u32,
        /// The Network board's touch gate, and the branch that runs
        /// once a touch exists. The target clears the event
        /// dereference as well as the pressed test, so no touch event
        /// has to be fabricated to get through it.
        board_touch_gate: u32,
        board_touch_taken: u32,
        /// Where the board's handler loads the hit code, which is where
        /// the walk has to have written it.
        board_code_load: u32,
        /// The comparison that decides whether the Net Battle screens
        /// have to collect a name and comment first. The screens are
        /// selected by an index the module keeps, and the first thing it
        /// runs reads a registered-name flag to choose between the name
        /// entry and the screen after it — so answering the comparison
        /// the flag feeds picks the latter. The site is that `cmp`, one
        /// instruction past the load, so the flag's own value is already
        /// in the register the answer replaces.
        ///
        /// A save that has never done a wireless battle has the flag
        /// clear, which parks the walk on a touch keyboard it cannot
        /// answer; the registration is the *player's* to make, and it
        /// is not something a match needs.
        name_registered_test: u32,
        /// The needs-registration accessor's return — a tiny getter
        /// (`ldrb` of save image `+0xb` through the context's image
        /// pointer) that every asker of "has this save registered its
        /// Net Battle name?" calls, from the CONTINUE load through the
        /// comm profile. The walk answers 0 ("registered") here, at
        /// the source, for the whole boot: answering only the entry
        /// screen's own test left an unregistered save carrying the
        /// flag into the wireless — a state real hardware can't
        /// produce, which the game tolerates but serves at a crawl
        /// (a registered host against an unregistered challenger runs
        /// the whole comm ~2.4x slow, board half and battle alike,
        /// emulator at full speed throughout). Poking the flag byte
        /// itself is no good twice over: cleared mid-boot it desyncs
        /// whatever already snapshotted it, and cleared in the image
        /// it stales the interior checksum. The getter is the one
        /// door all readers use. JP found by masked-byte match of the
        /// getter+setter pair (BLs wildcarded), unique hit.
        name_flag_read: u32,
        /// The Net Battle screen's touch gate, and the three branches
        /// the two consoles need from it.
        net_touch_gate: u32,
        net_designate: u32,
        net_list_update: u32,
        net_pick_row: u32,
        /// The two-button chooser's touch gate and its two answers.
        /// `chooser_first` lands where the first button's code goes;
        /// `chooser_second` lands on that code's own comparison, which
        /// is why [`SECOND_CODE`] has to arrive in a register.
        chooser_touch_gate: u32,
        chooser_first: u32,
        chooser_second: u32,
    }

    /// The game's own variables, in main RAM: what the walk reads to
    /// know where it is, and the few bytes it writes.
    struct RAMOffsets {
        /// Which module is running. Zero is the attract movie, which
        /// ignores everything except a request to stop — and none of
        /// the sites above are reachable until it does, so left alone
        /// it loops forever. Its battle value is the walk's finish
        /// line.
        module: u32,
        /// The game's own newly-pressed halfword.
        ///
        /// The save the game insists on before it will open the board
        /// is a real write to the cartridge, and the game only performs
        /// it when something confirms. Redirecting the dialogue past
        /// its waits answers the prompts but writes **nothing** — the
        /// screen says the save was made and the cart is untouched. So
        /// the confirm is given rather than faked: the A bit goes into
        /// the pad state the game has just built, and the game saves
        /// for itself.
        pressed: u32,
        /// The two screens' fade blocks, at their per-frame step field.
        /// The fade engine subtracts the step from a 0x100-range level
        /// every frame, so the boot's fades are as long as their steps
        /// are small — 8 for the half-minute ones. Holding the step at
        /// 0x100 while the boot runs finishes every fade the frame it
        /// starts: an instant cut instead of a ramp, which nothing
        /// downstream distinguishes because everything polls the
        /// engine's done flag rather than counting.
        fade_steps: [u32; 2],
        /// The script engine's per-character delay: instance byte +8,
        /// which its printer reloads the countdown at +9 from after
        /// every character it draws. The step handler prints, reloads,
        /// and loops within the tick while the countdown reads zero —
        /// so holding this byte at zero makes the game's own printer
        /// lay out each box whole the tick it opens, the fade-step
        /// idiom applied to text. The walk cares because every comm
        /// screen past the board narrates itself through a mugshot box
        /// first, at a character every other frame: the box, not the
        /// wireless, was most of the board half's clock.
        text_delay: u32,
        /// The flags the script sets while it is holding for a confirm.
        /// Gating on these keeps the confirm to the moments the game is
        /// actually asking for one.
        wait_flags: u32,
        /// The touch hit code — the halfword saying which button was
        /// hit. **Every** screen past the board reads this one, so the
        /// board's load site is the only one the walk has to write at.
        hit_code: u32,
        /// The screen object's sub-state word. Gating on it is what
        /// keeps each answer to its own screen, and what makes the walk
        /// a set of standing answers rather than a schedule.
        substate: u32,
        /// The host list, and its row count. The count's low byte is
        /// how the joiner tells a list that has found the host from one
        /// that has not; the object itself is what the row pick wants
        /// loaded.
        list_object: u32,
        list_count: u32,
        /// The game's three RNG state words — the GBA family's pair
        /// with one more, running the identical recurrence
        /// `x' = (rol1(x) + 1) ^ 0x873ca9e5` (the accessors sit at
        /// `0x02001118..0x020011d4` in the US ARM9, byte-identical in
        /// JP; found by searching the ARM9 for the GBA constant).
        /// `[0]` is GBA bn5's rng1, the draw stream, stepped once per
        /// unpaused frame by the main loop's master tick; `[1]` is
        /// rng2, the on-demand battle stream, which the game-init call
        /// at power-on resets to `0xa338244f` (its one reset — a
        /// single caller in the whole binary, run long before the save
        /// select); `[2]` is the port's own addition, on-demand with
        /// two callers. The walk writes all three at the field's START
        /// dispatch: the CONTINUE load re-derives every word as it
        /// runs (measured — a seed written at the load's own confirm
        /// is gone six ticks later), and the overworld standing is the
        /// proof the load is done. From there nothing but the
        /// accessors touches them, so the seeds stand — and the comm
        /// bring-up's own settings generation draws from them, which
        /// the wireless exchange then agrees on for real.
        rngs: [u32; 3],
    }

    /// The save-select screen object's chosen row, as a byte offset into
    /// the object the handler is running on (`r5`). Both hit-test
    /// branches write the row here and the confirm reads it back, so
    /// writing it *is* choosing a row — no touch to fabricate.
    const SAVE_ROW_FIELD: u32 = 6;

    /// Which save-select row a console's cartridge should be walked
    /// into. The screen keeps a fixed row per save file rather than
    /// listing whatever exists, so a save is only reachable at its own
    /// row — and the [`PlayedFile`](crate::dataview::save::PlayedFile)
    /// session payload *is* that row: the file-select slot the
    /// committing save view was on, carried through the netplay commit
    /// and the replay metadata so both peers and every future playback
    /// resolve the identical row.
    ///
    /// A console without one — a recording from before payloads
    /// existed (whose rewritten session cart holds only the played
    /// file), a probe fed raw dumps — lands on the file the cartridge
    /// itself calls current; a payload naming a file the cartridge
    /// doesn't hold reads the same as none. Row 0, the row the game's
    /// cursor starts on, for save memory the dataview cannot read.
    fn save_row(payload: Option<&dyn tango_match::SessionPayload>, save: &[u8]) -> u8 {
        let Ok(set) = crate::dataview::save::SaveSet::parse(save) else {
            return 0;
        };
        payload
            .and_then(|p| (p as &dyn std::any::Any).downcast_ref::<crate::dataview::save::PlayedFile>())
            .map(|file| file.0)
            .filter(|slot| set.slots().contains(slot))
            .unwrap_or_else(|| set.current().slot())
    }

    /// One console's three RNG seeds off the negotiated match seed —
    /// the mgba backend's `core_rng_seed` derivation carried over:
    /// identical on both peers (both walk both consoles), distinct
    /// between the consoles and the streams, exactly the situation the
    /// vanilla wireless protocol is built for (two real consoles never
    /// share RNG state — the games' own link exchange synchronizes
    /// whatever the battle needs agreed on). The recurrence has no
    /// stuck state, so no lane needs a zero guard.
    fn console_rng_seeds(rng_seed: &[u8; 16], console: usize) -> [u32; 3] {
        std::array::from_fn(|stream| {
            let lane = (console * 3 + stream) as u32;
            let i = lane as usize * 4 % rng_seed.len();
            let v = u32::from_le_bytes(rng_seed[i..i + 4].try_into().unwrap());
            // Perturb by lane so identical seed words still land
            // distinct streams.
            v ^ 0x9e37_79b9u32.wrapping_mul(lane + 1)
        })
    }

    /// The US build (`A5TE`), where all of this was found.
    #[rustfmt::skip]
    pub static US: Layout = Layout {
        tag: "bn5ds",
        code: CodeOffsets {
            logo_hold:                0x0206_4dd0,
            logo_expired:             0x0206_4dda,
            title_arming_gate:        0x0202_cad2,
            title_press_check:        0x0202_cad4,
            save_select_gate:         0x0203_9ad4,
            save_select_confirm:      0x0203_9aec,
            continue_gate:            0x0203_9914,
            continue_confirm:         0x0203_9924,
            field_start_gate:         0x0208_7df4,
            field_start_menu_open:    0x0208_7e18,
            start_menu_gate:          0x0208_4d68,
            start_menu_network:       0x0208_5044,
            script_timer_gate:        0x0209_b362,
            script_timer_expired:     0x0209_b36c,
            script_box_gate:          0x0209_ae68,
            script_box_dismissed:     0x0209_ae8a,
            pad_refresh_ret:          0x0200_0ce8,
            board_touch_gate:         0x021e_0c88,
            board_touch_taken:        0x021e_0c98,
            board_code_load:          0x021e_0ca4,
            net_touch_gate:           0x021e_30c0,
            name_registered_test:     0x021d_f020,
            name_flag_read:           0x0200_1d84,
            net_designate:            0x021e_3398,
            net_list_update:          0x021e_33f4,
            net_pick_row:             0x021e_3334,
            chooser_touch_gate:       0x021d_de14,
            chooser_first:            0x021d_de40,
            chooser_second:           0x021d_de30,
        },
        ram: RAMOffsets {
            module:                   0x0216_f71c,
            pressed:                  0x0215_f3d6,
            fade_steps:              [0x0216_fbc8, 0x0216_fbe8],
            text_delay:               0x0217_1594,
            wait_flags:               0x0216_f6e8,
            hit_code:                 0x021c_5db8,
            substate:                 0x021f_66ec,
            list_object:              0x021c_6260,
            list_count:               0x021c_688c,
            rngs:                    [0x0216_bb1c, 0x0216_f230, 0x0216_bb20],
        },
    };

    /// The JP build (`A5TJ`, Rockman EXE 5 DS), the same walk
    /// relocated.
    ///
    /// Found by matching the US code byte-for-byte into the JP ARM9
    /// (main-RAM pointers and branch displacements masked): each
    /// function carries its own shift, but within a function the US
    /// spacing holds, so each pair below is a matched gate plus the US
    /// pair's distance. The pad refresh did not move — the main loop
    /// sits at the same address in both builds.
    ///
    /// The RAM addresses are **not** one uniform shift. The boot half's
    /// are the US ones minus 0x7260, the BSS block's; the screen
    /// objects the board half reads live in a differently-placed block,
    /// so those were read out of the JP literal pool at each matched
    /// site instead. The ARM7 sites below the layouts hold as-is: the
    /// two ARM7 binaries are byte-identical.
    #[rustfmt::skip]
    pub static JP: Layout = Layout {
        tag: "exe5ds",
        code: CodeOffsets {
            logo_hold:                0x0206_4b90,
            logo_expired:             0x0206_4b9a,
            title_arming_gate:        0x0202_c8de,
            title_press_check:        0x0202_c8e0,
            save_select_gate:         0x0203_98ac,
            save_select_confirm:      0x0203_98c4,
            continue_gate:            0x0203_96ec,
            continue_confirm:         0x0203_96fc,
            field_start_gate:         0x0208_7b00,
            field_start_menu_open:    0x0208_7b24,
            start_menu_gate:          0x0208_4a74,
            start_menu_network:       0x0208_4d50,
            script_timer_gate:        0x0209_b022,
            script_timer_expired:     0x0209_b02c,
            script_box_gate:          0x0209_ab28,
            script_box_dismissed:     0x0209_ab4a,
            pad_refresh_ret:          0x0200_0ce8,
            board_touch_gate:         0x021d_98fc,
            board_touch_taken:        0x021d_990c,
            board_code_load:          0x021d_9918,
            net_touch_gate:           0x021d_bd3c,
            name_registered_test:     0x021d_7d60,
            name_flag_read:           0x0200_1d4c,
            net_designate:            0x021d_c014,
            net_list_update:          0x021d_c070,
            net_pick_row:             0x021d_bfb0,
            chooser_touch_gate:       0x021d_6b54,
            chooser_first:            0x021d_6b80,
            chooser_second:           0x021d_6b70,
        },
        ram: RAMOffsets {
            module:                   0x0216_84bc,
            pressed:                  0x0215_8176,
            fade_steps:              [0x0216_8968, 0x0216_8988],
            text_delay:               0x0216_a334,
            wait_flags:               0x0216_8488,
            hit_code:                 0x021b_eb04,
            substate:                 0x021e_f06c,
            list_object:              0x021b_efa0,
            list_count:               0x021b_f5cc,
            rngs:                    [0x0216_48bc, 0x0216_7fd0, 0x0216_48c0],
        },
    };

    /// The module id the attract movie runs under, which is what
    /// `RAMOffsets::module` reads until the save is loaded.
    const INTRO: u32 = 0;
    /// The module id the Network board and every comm screen over it
    /// run under. The walk's finish line is *leaving* it: which module
    /// a battle boots as is not one id — the identical Single/Practice
    /// pick loads module 0x1f from a save this build wrote, but 0x3b
    /// from a save the other build wrote (both real, linked battles;
    /// the whole route is pixel-identical up to the fade) — so arrival
    /// is untestable and departure is not. A stalled flow never leaves
    /// the board module, and the game's own comm-error exits tear the
    /// association down first, so "left it with the link still up" is
    /// exactly "the battle transition has started".
    const BOARD: u32 = 0x17;

    /// A within the game's newly-pressed halfword.
    const CONFIRM: u8 = 0x01;
    /// START, which is what the attract movie listens for.
    const SKIP: u8 = 0x08;
    /// A fade step that finishes any fade the frame it starts.
    const FADE_INSTANT: u16 = 0x0100;
    /// What `RAMOffsets::wait_flags` reads while the script is
    /// holding for a confirm.
    const WAITING: u32 = 0x0000_0088;

    /// The hit code for the board's Net Battle entry. The board maps
    /// codes `0x60..=0x66` onto its six buttons and its bottom bar, and
    /// this is the first of them.
    const NET_BATTLE: u16 = 0x0060;
    /// The code the chooser's second button compares against, and the
    /// register it has to arrive in — `CodeOffsets::chooser_second`
    /// lands on the comparison rather than before it.
    const SECOND_CODE: u32 = 0x83;
    const CODE_REG: u32 = 0;

    /// What the joiner's row pick needs loaded: the list object, and
    /// the row as the branch's own dispatch counts it. The first row is
    /// the host — the joiner only ever sees the one console
    /// advertising.
    const LIST_REG: u32 = 0;
    const ROW_REG: u32 = 7;
    const FIRST_ROW: u32 = 1;

    /// What `RAMOffsets::substate` reads while the joiner sits on the
    /// Net Battle screen with a list it has not filled yet. The host's
    /// screen idles at the same value until its designation sticks, so
    /// both consoles' answers gate on it.
    const JOINER_LIST_IDLE: u32 = 0x0000_0203;
    /// How long the joiner's answers wait before asking again, in
    /// frames of the gate polling its screen. Longer than a scan's
    /// full round trip, so a retry can never restart a scan that is
    /// still underway; short enough for several tries inside
    /// [`BATTLE_BUDGET`]. A clean run never waits this out — its pick
    /// lands on the first scan's report.
    const RETRY_COOLDOWN: u32 = 300;
    /// What the count byte at `RAMOffsets::list_count` reads once the
    /// list holds the host — one entry, because the joiner only ever
    /// sees the one console advertising. The count byte ALONE: the
    /// word around it once served as a cheaper whole-word test
    /// (`0x0001_0101`), but its top byte turned out to be a real
    /// neighboring field that reads 1 for some host×joiner save
    /// pairings (records/registration-adjacent — the row draws a
    /// Results panel and an extra glyph by the host's name exactly
    /// when it is set), and it holds that value for as long as the
    /// screen does. Comparing the whole word wedged those pairings:
    /// the list held the host, the game sat healthy on the screen, and
    /// the pick never fired — the "file 1 vs file 2 white screen until
    /// the connection times out" stall, long misattributed to flash
    /// wear geometry because rewriting the cart happened to change
    /// what the two saves knew about each other.
    const LIST_HAS_HOST: u8 = 1;
    /// What `RAMOffsets::substate` reads while a chooser waits for
    /// its answer. The joiner meets all three in this order — the mode,
    /// then Practice, then "connect using these options?" — and the
    /// host's single prompt reads the first of them, which costs
    /// nothing to share because the two consoles get their own traps.
    const CHOOSING: [u32; 3] = [0x0102_0303, 0x0104_0303, 0x0106_0303];
    // These words are matched EXACTLY, top byte included. An
    // unregistered-challenger pairing once ran the comm screens in a
    // variant reading these low bytes under a 0 top byte — the host's
    // accept as a real prompt, the joiner's as non-interactive mirrors
    // whose "answers" desync the negotiation into a battle the game
    // itself interrupts. The name-flag trap (see
    // `CodeOffsets::name_flag_read`) suppresses that whole variant at
    // its source, so no chooser answer for it exists anymore; if a new
    // variant ever shows up, learn its exact word — don't mask.

    /// The ARM7 side of the save: its backup server's flash wait, at
    /// the function's entry. r0 arrives holding the mandatory pre-poll
    /// delay — 395 scanlines per 0x100-byte page, a real flash chip's
    /// program time — and r1 the poll timeout. The emulated flash is
    /// ready the moment it is asked, so the delay is the entire cost of
    /// the save: zeroing r0 keeps the timeout-and-poll path, which
    /// still decides, and the wait that was most of "Saving..." simply
    /// isn't waited.
    const ARM7_FLASH_WAIT: u32 = 0x0380_33e4;
    /// The wait's poll loop sleeps once before it will even look at the
    /// status register; jumping the sleep call to the poll asks first.
    const ARM7_FLASH_POLL_SLEEP: u32 = 0x0380_3468;
    const ARM7_FLASH_POLL: u32 = 0x0380_3470;

    /// How long the boot half takes. Nothing is pressed and nothing
    /// branches on timing, so both consoles run the identical
    /// deterministic path every time: the cartridge is written and the
    /// board stands clear of its last message box by about frame 420,
    /// and this carries margin on top.
    ///
    /// Watching the cartridge instead would be the more obvious finish
    /// line, but it is a false one — the game writes a little
    /// bookkeeping to the cart early, well before the save the board is
    /// waiting on.
    ///
    /// What remains is what redirects cannot buy: the opening movie's
    /// prebuffer (real cart reads feeding real state — forcing its
    /// ready-check boots into a glitched stall), the title card (its
    /// length is its jingle's length, decided inside the sound stream's
    /// own data), and the save's residual ~45 frames (the ARM9 sleeping
    /// a beat per page on the ARM7's replies). The save used to be ten
    /// times that: the ARM7's flash wait — see [`ARM7_FLASH_WAIT`] —
    /// spent 395 scanlines per page on a program delay the emulated
    /// flash never needs, which is also why making the SPI instant
    /// changed nothing. The bus was never what the wait was made of.
    const BUDGET: u32 = 490;

    /// How many confirms the boot half needs before the board stands:
    /// the two save prompts and the notice afterwards. Fewer means the
    /// walk went somewhere else — most likely a save with no NetBattle
    /// unlocked, which never reaches the Network menu at all.
    const CONFIRMS_EXPECTED: u32 = 3;

    /// How long the board half is given. It takes about 260 frames:
    /// roughly fifty of joiner scan, thirty of host designation, and
    /// the screens around them. It was 630 before the text hold — the
    /// association was never the wait; the mugshot boxes narrating each
    /// comm screen at a character every other frame were. The budget
    /// carries several times the measured route's margin.
    const BATTLE_BUDGET: u32 = 2400;

    impl Layout {
        /// The game's module word, for the phase read above — which
        /// screen the game is running is the walk's business to know
        /// and the telemetry's business to watch.
        pub(super) fn module_word(&self) -> u32 {
            self.ram.module
        }

        /// One console's priming traps, in lifecycle order: boot, the
        /// Network board, the Net Battle screen, then the choosers.
        ///
        /// `host` picks which half of the Net Battle screen this console
        /// drives, and `second` whether its chooser takes the mode
        /// screen's second button. `confirms` counts the save prompts
        /// the run answers, and is only handed to one console — the two
        /// run the same route, so counting both would just double it.
        ///
        /// One set carries the whole route: nothing has to be swapped
        /// over part-way, because each answer's site is unreachable
        /// until its own screen is up. Each answer past the board fires
        /// once — the list scan reports back asynchronously, so an
        /// answer that fires again before the previous one lands just
        /// restarts the scan and the list never settles.
        ///
        /// These are host state rather than console state, so none of it
        /// is simulation the peers could disagree about: both install
        /// the same set, and from identical saves both take the same
        /// branches.
        fn traps(
            &'static self,
            host: bool,
            second: bool,
            save_row: u8,
            rng_seeds: [u32; 3],
            confirms: Option<std::sync::Arc<std::sync::atomic::AtomicU32>>,
        ) -> Vec<(u32, Box<dyn FnMut(&mut Nds)>)> {
            let code = &self.code;
            let ram = &self.ram;

            vec![
                // ----- boot to the Network board -----
                (
                    // The Capcom logo's hold, into its expired branch. The
                    // logo module fades in, counts a 120-frame dwell down
                    // while the jingle plays, and fades out; the gate is
                    // where the tick picks the counter up once the fade-in
                    // is done. The dwell and the jingle go together — the
                    // jingle's trigger is the countdown's first tick.
                    code.logo_hold,
                    Box::new(move |nds: &mut Nds| nds.jump_here(code.logo_expired)),
                ),
                (
                    // The title screen's arming delay (Thumb). Its master
                    // tick holds a countdown that doubles as the demo-movie
                    // timeout, and only consults the press-START check once
                    // it has fallen 0x2f below its starting point — 47
                    // frames of the title asking not to be interrupted. The
                    // check it releases still reads the pad itself, so this
                    // arms the question rather than answering it.
                    code.title_arming_gate,
                    Box::new(move |nds: &mut Nds| nds.jump_here(code.title_press_check)),
                ),
                (
                    // The save select's test-A, into its own confirm
                    // branch — pointed at `save_row` first. The screen
                    // keeps a fixed row per save file, so the row the
                    // cursor starts on reads NO DATA on a cartridge whose
                    // save lives in the other one: confirming it would
                    // mean NEW GAME, and the boot would run into name
                    // entry. Writing the row the confirm is about to read
                    // is the same answer a touch on that row would have
                    // left behind.
                    code.save_select_gate,
                    Box::new(move |nds: &mut Nds| {
                        let object = nds.reg(5);
                        nds.write8(object + SAVE_ROW_FIELD, save_row);
                        nds.jump_here(code.save_select_confirm)
                    }),
                ),
                (
                    // The CONTINUE / NEW GAME submenu's test-A, into the
                    // confirm its cursor dispatch hangs off. The cursor
                    // already reads CONTINUE: it carries the save slot the
                    // branch above chose.
                    code.continue_gate,
                    Box::new(move |nds: &mut Nds| nds.jump_here(code.continue_confirm)),
                ),
                (
                    // The overworld field dispatcher, which compares the
                    // pressed word against START, into the branch that
                    // opens the START menu. The rng seeds ride this
                    // confirm (see [`RAMOffsets::rngs`]): the overworld
                    // standing means the CONTINUE load's own reseeds are
                    // done — seeded any earlier they'd be re-derived away
                    // — and from here nothing but the accessors touches
                    // the words, so what's written here is what the comm
                    // screens' own settings generation draws from.
                    code.field_start_gate,
                    Box::new(move |nds: &mut Nds| {
                        for (&addr, &seed) in ram.rngs.iter().zip(&rng_seeds) {
                            nds.write32(addr, seed);
                        }
                        nds.jump_here(code.field_start_menu_open)
                    }),
                ),
                (
                    // The START menu's read of the pad, into the accepted
                    // path of its Network entry — past the cursor entirely,
                    // so no rows are walked. (That grid is indexed
                    // `col * 3 + row`; Network is entry 6.)
                    code.start_menu_gate,
                    Box::new(move |nds: &mut Nds| nds.jump_here(code.start_menu_network)),
                ),
                (
                    // The script engine's timed waits, into the branch a
                    // spent timer takes. Most of the save is the game
                    // pacing its own message box, and this is that pacing:
                    // the board comes up 250 frames sooner for it. The save
                    // still completes — it is written by the time the board
                    // stands either way — because what the write waits on
                    // is the cartridge, not these counters.
                    code.script_timer_gate,
                    Box::new(move |nds: &mut Nds| nds.jump_here(code.script_timer_expired)),
                ),
                (
                    // Opcode 0xE7, the wait that holds a message box open
                    // until it is dismissed, into the branch a dismissal
                    // takes. This is the one the confirm below cannot
                    // answer: it sets the script's own flags rather than the
                    // global ones the confirm watches, so left alone the "I
                    // made the save" box sits on the board forever.
                    code.script_box_gate,
                    Box::new(move |nds: &mut Nds| nds.jump_here(code.script_box_dismissed)),
                ),
                {
                    // The main loop's own pad refresh, where the walk writes
                    // instead of jumping — it has to be here rather than at
                    // the waiting code, because the screen reads input
                    // earlier in the frame than the script does, so writing
                    // any later is writing after everyone has already
                    // looked. Three jobs: stop the attract movie, answer
                    // the save prompts so the game performs its own save,
                    // and hold the text printer at full speed.
                    let mut pressed = false;
                    (
                        code.pad_refresh_ret,
                        Box::new(move |nds: &mut Nds| {
                            // Held for the whole walk, not just the boot:
                            // the board half's screens narrate through the
                            // same script engine, and their boxes were most
                            // of its clock. The trap coming off is what
                            // ends the hold, and no screen the session can
                            // reach reads it again — the battle talks
                            // through its own machinery.
                            nds.write8(ram.text_delay, 0);
                            if nds.read32(ram.module) == INTRO {
                                nds.write8(ram.pressed, SKIP);
                                // Any fade the boot has started finishes
                                // now. The step is rewritten by each fade's
                                // starter, so this holds only for as long as
                                // the boot does — the match's own fades keep
                                // their pace.
                                for addr in ram.fade_steps {
                                    nds.write16(addr, FADE_INSTANT);
                                }
                                return;
                            }
                            let waiting = nds.read32(ram.wait_flags) == WAITING;
                            if waiting {
                                nds.write8(ram.pressed, CONFIRM);
                            }
                            // One count per prompt rather than per frame:
                            // the flags stay set for as long as the game is
                            // asking.
                            if waiting && !pressed {
                                if let Some(confirms) = &confirms {
                                    confirms.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                            pressed = waiting;
                        }),
                    )
                },
                // ----- the Network board -----
                (
                    // Its touch gate, into the branch that runs once a touch
                    // exists. Unreachable until the board stands, so it
                    // costs nothing to have been installed since power-on.
                    code.board_touch_gate,
                    Box::new(move |nds: &mut Nds| nds.jump_here(code.board_touch_taken)),
                ),
                (
                    // The hit code the gate above is about to read: Net
                    // Battle. Writing the selection and letting the game's
                    // own handler act on it is the same idiom as the save
                    // confirm. It stops mattering by itself once the screen
                    // changes.
                    code.board_code_load,
                    Box::new(move |nds: &mut Nds| nds.write16(ram.hit_code, NET_BATTLE)),
                ),
                (
                    // Report the name and comment as already registered,
                    // so the module picks the screen after the name entry
                    // rather than the entry itself. The game's own
                    // comparison and its own branch do the choosing; this
                    // only answers what the flag was asked. See
                    // [`CodeOffsets::name_registered_test`].
                    code.name_registered_test,
                    Box::new(move |nds: &mut Nds| nds.set_reg(0, 1)),
                ),
                (
                    // Every OTHER asker of the same question — the
                    // CONTINUE load, the file rows, the comm profile —
                    // asks through one tiny getter, and this answers it
                    // at its return: registered. The trap sits on the
                    // getter's `pop {pc}`, firing before it executes,
                    // so r0 carries the answer home. See
                    // [`CodeOffsets::name_flag_read`] for why the flag
                    // byte itself can't just be cleared.
                    code.name_flag_read,
                    Box::new(move |nds: &mut Nds| nds.set_reg(0, 0)),
                ),
                // ----- the Net Battle screen -----
                if host {
                    // The host has one thing to do: put itself up as the
                    // host and wait to be found. What follows is the game's
                    // real wireless protocol — and like the joiner's scan
                    // below, its effect lands asynchronously, so this is a
                    // gated retry rather than a one-shot: the screen idles
                    // at the same sub-state as the joiner's list until the
                    // designation sticks, so firing while it reads that and
                    // holding a cooldown in between keeps a designation the
                    // radio wasn't ready for from being the walk's only
                    // try. A clean run is untouched — its screen already
                    // reads the idle sub-state on the gate's first poll, so
                    // the first firing lands on the same frame it always
                    // did — which is what keeps existing recordings
                    // aligned.
                    let mut cd = 0u32;
                    (
                        code.net_touch_gate,
                        Box::new(move |nds: &mut Nds| {
                            cd = cd.saturating_sub(1);
                            if cd == 0 && nds.read32(ram.substate) == JOINER_LIST_IDLE {
                                cd = RETRY_COOLDOWN;
                                nds.jump_here(code.net_designate);
                            }
                        }),
                    )
                } else {
                    // The joiner has two, and which one is due is a question
                    // its own list answers: refresh it while it is empty,
                    // then pick the host out of it once it is not.
                    //
                    // Neither answer is a one-shot. The scan reports back
                    // asynchronously, so an answer must not re-fire before
                    // its effect lands — that just restarts the scan — but
                    // an answer that can never fire again turns one missed
                    // beacon into a permanent stall: a scan the host's
                    // advertisement slipped past leaves the list empty, the
                    // screen idle, and the walk out of moves for the rest
                    // of the budget. So each answer instead holds its own
                    // cooldown, longer than a scan's round trip: the first
                    // firing lands on the same frame it always did — a
                    // clean run's tick count is untouched, which is what
                    // keeps existing recordings aligned — and a run whose
                    // scan came back empty asks again instead of dying.
                    let (mut pick_cd, mut refresh_cd) = (0u32, 0u32);
                    (
                        code.net_touch_gate,
                        Box::new(move |nds: &mut Nds| {
                            pick_cd = pick_cd.saturating_sub(1);
                            refresh_cd = refresh_cd.saturating_sub(1);
                            if pick_cd == 0 && nds.read8(ram.list_count) == LIST_HAS_HOST {
                                pick_cd = RETRY_COOLDOWN;
                                nds.set_reg(ROW_REG, FIRST_ROW);
                                nds.set_reg(LIST_REG, ram.list_object);
                                nds.jump_here(code.net_pick_row);
                            } else if refresh_cd == 0 && nds.read32(ram.substate) == JOINER_LIST_IDLE {
                                refresh_cd = RETRY_COOLDOWN;
                                nds.jump_here(code.net_list_update);
                            }
                        }),
                    )
                },
                // ----- the choosers: the mode, Practice, the connects -----
                {
                    // One widget serves all of them, so one answer does too:
                    // which screen is asking is the sub-state's to say, and
                    // only the mode screen has a second button worth taking.
                    // The rest are Practice and Yes, both the first —
                    // Practice deliberately, since Real Thing spends the
                    // players' own records on the result.
                    let waits: &'static [u32] = if host { &CHOOSING[..1] } else { &CHOOSING };
                    let mut spent = [false; CHOOSING.len()];
                    (
                        code.chooser_touch_gate,
                        Box::new(move |nds: &mut Nds| {
                            let Some(i) = waits.iter().position(|&w| w == nds.read32(ram.substate)) else {
                                return;
                            };
                            if std::mem::replace(&mut spent[i], true) {
                                return;
                            }
                            if second && !host && i == 0 {
                                nds.set_reg(CODE_REG, SECOND_CODE);
                                nds.jump_here(code.chooser_second);
                            } else {
                                nds.jump_here(code.chooser_first);
                            }
                        }),
                    )
                },
            ]
        }

        /// The ARM7's traps, which are the same on both builds. These
        /// are the only ones on that processor, and they answer the one
        /// wait no ARM9 redirect can reach: the backup server's per-page
        /// flash delay, which the emulated flash never needs.
        fn traps7() -> Vec<(u32, Box<dyn FnMut(&mut Nds)>)> {
            vec![
                (ARM7_FLASH_WAIT, Box::new(|nds: &mut Nds| nds.arm7_set_reg(0, 0))),
                (
                    ARM7_FLASH_POLL_SLEEP,
                    Box::new(|nds: &mut Nds| nds.arm7_jump_here(ARM7_FLASH_POLL)),
                ),
            ]
        }

        /// Install the walk on both consoles, sharing one count of how
        /// many confirms the run has needed.
        ///
        /// Traps run the console interpreted — both processors, since
        /// either side having any holds the JIT off — so they come off
        /// again the moment priming is done and the match runs compiled.
        fn install(
            &'static self,
            link: &mut Link,
            second: bool,
            session_payloads: [Option<&dyn tango_match::SessionPayload>; 2],
            rng_seed: [u8; 16],
        ) -> std::sync::Arc<std::sync::atomic::AtomicU32> {
            let confirms = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            for seat in 0..2 {
                let host = seat == 0;
                let save_row = save_row(session_payloads[seat], &link.console(seat).save_memory());
                let rng_seeds = console_rng_seeds(&rng_seed, seat);
                link.console(seat)
                    .set_traps(self.traps(host, second, save_row, rng_seeds, host.then(|| confirms.clone())));
                link.console(seat).set_traps7(Self::traps7());
            }
            confirms
        }

        /// Take the traps back off, handing both processors to the JIT.
        fn uninstall(&self, link: &mut Link) {
            for seat in 0..2 {
                link.console(seat).set_traps(Vec::new());
                link.console(seat).set_traps7(Vec::new());
            }
        }

        /// Run both consoles from power-on into the agreed mode's link
        /// battle. `session_payloads` are the consoles' session
        /// payloads in seat order — each console's save-select row (see
        /// [`save_row`]); `rng_seed` is the negotiated match seed the
        /// walk reseeds the game's rngs from (see [`console_rng_seeds`]).
        /// Flipping `cancel` fails the walk with
        /// [`Cancelled`](tango_match::Error::Cancelled) instead of
        /// finishing it — replay boots run on host worker threads whose
        /// teardown joins them.
        pub fn walk(
            &'static self,
            link: &mut Link,
            match_type: (u8, u8),
            session_payloads: [Option<&dyn tango_match::SessionPayload>; 2],
            rng_seed: [u8; 16],
            cancel: Option<&std::sync::atomic::AtomicBool>,
        ) -> Result<(), tango_match::Error> {
            let started = std::time::Instant::now();
            let before = link.console(0).save_memory();
            // The registration lists Single first and Triple second, so
            // the mode is only which of the chooser's two buttons the
            // joiner takes.
            let counter = self.install(link, match_type.0 != 0, session_payloads, rng_seed);

            // The boot half, which is over when the board stands: it
            // answers nothing that depends on the other console, so it
            // runs to a frame count and is checked afterwards.
            for _ in 0..BUDGET {
                if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                    self.uninstall(link);
                    return Err(tango_match::Error::Cancelled);
                }
                link.tick([HostInput::default(); 2]);
            }
            let saved = link.console(0).save_memory() != before;
            let confirms = counter.load(std::sync::atomic::Ordering::Relaxed);
            log::info!(
                "{} priming: board at {BUDGET} frames in {:.1?}, {confirms} confirms, saved={saved}",
                self.tag,
                started.elapsed()
            );
            if !saved {
                log::warn!(
                    "{} priming never saw the cartridge written; the board will not be open",
                    self.tag
                );
                self.uninstall(link);
                return Err(tango_match::Error::PrimeTimeout(BUDGET));
            }
            if confirms < CONFIRMS_EXPECTED {
                log::warn!(
                    "{} priming saw {confirms} confirms, expected {CONFIRMS_EXPECTED}: \
                     does this save have NetBattle unlocked?",
                    self.tag
                );
                self.uninstall(link);
                return Err(tango_match::Error::PrimeTimeout(BUDGET));
            }

            // The board half, which is over when the battle transition
            // starts: both consoles leave the board module with the
            // wireless association still up — see [`BOARD`] for why
            // the battle module itself is not testable. Most of the
            // wait is the two consoles' wireless associating, so it
            // waits on the game rather than on a count. Requiring the
            // board to have *stood* first keeps a slow boot from
            // reading its last menu module as a departure.
            let mut frames = 0;
            let mut stood = false;
            let battled = loop {
                if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                    self.uninstall(link);
                    return Err(tango_match::Error::Cancelled);
                }
                let modules = [
                    link.console(0).read32(self.ram.module),
                    link.console(1).read32(self.ram.module),
                ];
                stood = stood || modules == [BOARD; 2];
                if stood && modules.iter().all(|&m| m != BOARD) && link.connected() {
                    break true;
                }
                if frames >= BATTLE_BUDGET {
                    break false;
                }
                link.tick([HostInput::default(); 2]);
                frames += 1;
            };
            self.uninstall(link);

            if !battled {
                // Enough state to place the stall without a debugger:
                // the substate says which comm screen each console is
                // parked on, the list word whether the joiner ever saw
                // the host advertise.
                log::warn!(
                    "{} priming: no battle {frames} frames past the board \
                     (stood={stood}, connected={}, modules {:#x}/{:#x}, \
                     substates {:#010x}/{:#010x}, list {:#010x}/{:#010x})",
                    self.tag,
                    link.connected(),
                    link.console(0).read32(self.ram.module),
                    link.console(1).read32(self.ram.module),
                    link.console(0).read32(self.ram.substate),
                    link.console(1).read32(self.ram.substate),
                    link.console(0).read32(self.ram.list_count),
                    link.console(1).read32(self.ram.list_count),
                );
                return Err(tango_match::Error::PrimeTimeout(BUDGET + frames));
            }
            log::info!(
                "{} priming: match type {match_type:?}, battle transition {frames} frames past the board, {:.1?} total",
                self.tag,
                started.elapsed()
            );
            Ok(())
        }
    }
}

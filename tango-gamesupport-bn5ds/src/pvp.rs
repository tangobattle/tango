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
//! **The team route** is the same walk, entered by a different button.
//! Team Battle is its own entry on the Network board, so the subtype
//! only picks which hit code the board's answer carries. It is subtype
//! *zero* of each mode — these games are played as Team, and the lobby
//! defaults to a mode's first subtype — and then one
//! more answer, because that button routes through Navi Select (where
//! a player builds the team they bring) on its way to the comm screen.
//! The board's own tail is what decides that, having already written
//! the battle kind, so answering *its* comparison sends a Team Battle
//! to the comm screen directly: a team match with the team left empty,
//! which is what exiting Navi Select without downloading one gives
//! anyway. Nothing on that screen has to be worked, which matters
//! because it is the one screen on the route a redirect could not
//! answer — its buttons come off the shared widget framework rather
//! than a jump table of its own, so there is no branch a hit would
//! have taken. Driving it is the work a real team pick would need.
//!
//! What lies past the board is the same screen either way, with the
//! same handler and the same designation, list and row pick. Only the
//! joiner's three choosers move: the comm module numbers its
//! sub-screens one higher on this route, so they are gated on their own
//! words (see `CHOOSING_TEAM`). The host's accept does not move.
//!
//! The pair is symmetric, so the seats are assigned rather than
//! negotiated: **console 0 takes the game's host seat and console 1
//! joins it**. Both peers walk both consoles, so both agree without
//! asking and nothing has to cross the wire.
//!
//! **What the cartridge brings** is read out of the cartridge rather
//! than handed to the walk: which of the two in-game files is played
//! (the one the game itself calls most recently saved) and which
//! GBA-slot cross the save carries. The second needs a trap, because
//! the file select clears the cross byte on every boot before it looks
//! for a cartridge in the GBA slot (`CodeOffsets::cross_clear`). Slot 2
//! itself is never emulated: the pick is the player's, made in the save
//! editor, and the game does the rest.
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
    chips: 0x021b_8af8,
};

/// What the JP registration's
/// [`DsBackend`](tango_backend_melonds::DsBackend) closes over.
pub static JP: Pvp = Pvp {
    layout: &priming::JP,
    unit: 0x022c_ee18,
    custom: 0x0215_9732,
    chips: 0x021b_1848,
};

/// The unit record's size, which is also the second slot's offset.
const UNIT_STRIDE: u32 = 0xdc;

/// What `RAMOffsets::substate` reads for the whole link battle: the
/// last step of the connect exchange, which the game then holds
/// through every round and every interlude between them. Reaching it
/// on both consoles is the walk's finish line, and leaving it with the
/// unit block dead is the end of the match — the game is back on its
/// comm screens (`0x0001_0202` as they come up, `0x0001_0602` on the
/// result message) with the wireless still up.
///
/// Neither test may be written against `RAMOffsets::scene`, which is
/// what both used to be. That word reads the **overworld area the save
/// is standing in** whenever the game is on the field or the comm
/// screens drawn over it, so its value is a property of the cartridge
/// rather than of the game state: two saves of one cart negotiate and
/// then park under `0x17` and `0x1b` at the identical moments (a
/// bedroom and the street outside — and two different bedrooms both
/// read `0x17`, so it is not even one id per room), and a save parked
/// anywhere else reads something else again. A whitelist of ids can
/// never be complete, and the one that used to gate the match end
/// silently never fired for the second save on the cart. This substate
/// is one value for every save: measured on saves of both kinds and in
/// both match modes, it stands from the pre-battle exchange to the
/// moment the scene word returns to the field.
const BATTLE_SESSION: u32 = 0x0003_0102;

/// The music a link battle plays, as sequence numbers in the cart's
/// sound archive: `BGM_BATTLE` and `BGM_BOSS`, the archive's own names
/// for them.
///
/// A round starts one of these and holds it to the end. A Triple
/// Battle starts one per round, and **the deciding round is the boss
/// theme** — watched in a recorded three-round set, which ran
/// `BGM_BATTLE` for rounds one and two and `BGM_BOSS` for round three.
///
/// The jingles that cart plays between rounds are deliberately left
/// alone: the same set played `BGM_LOSE` and `BGM_RESULT_2` at its
/// round boundaries and `BGM_LOSE` again at the match end, and those
/// are the wind-down a player watches, not the battle's music — the
/// same line the mgba families draw when they skip only the
/// battle-start call. The comm screens either side keep their music
/// for the same reason.
const BATTLE_THEMES: &[u16] = &[0x15, 0x16];

impl tango_backend_melonds::GameSupport for Pvp {
    /// 1: priming hands off when the battle transition starts (the board
    /// module's departure) instead of when the battle module arrives, a
    /// few frames earlier — recordings made against the old finish line
    /// carry inputs offset by that gap.
    ///
    /// 3: melonDS runs interpreted — the JIT is no longer built (see
    /// melonds-sys's build script). Compiled and interpreted execution
    /// don't cost the same cycles, so every tick of the emulated timeline
    /// lands differently: a match against a JIT build desyncs, and its
    /// recordings replay to a different match here.
    ///
    /// 4: a savestate carries the ARM9's divider and square-root registers
    /// (melonds-rs d4314a5), and a client's default MP reply no longer
    /// puts uninitialised stack on the air (a039993). Neither moves the
    /// timeline the way 3 did — a run from boot lands where it did — but a
    /// *restore* no longer hands the game back arithmetic left over from
    /// the run it took back, so two peers only agree about a rolled-back
    /// tick if both carry the fix.
    ///
    /// 5: the cartridge backup server's pre-poll delay is capped at one
    /// tick rather than zeroed. Returning from that wait without ever
    /// sleeping is what left EXE OSS's comm screens crawling on a cart with
    /// no play on it, and this is the same backup server on the same
    /// emulator; the tick costs the save a handful of frames, so priming
    /// hands over that much later.
    ///
    /// 7: what a console plays now comes out of its own cartridge instead
    /// of a session payload riding beside it. Priming walks the file the
    /// game itself calls most recently saved (a recording made when the
    /// payload named the *other* file re-primes into a different save), and
    /// a save carrying a GBA-slot cross gets it written back at the file
    /// select's own store, which the walk did not touch before.
    ///
    /// (Also 7, landing together: a tick's span of emulated time is filled
    /// in whole frames. A call used to be able to stop partway through one,
    /// and the resumed call — finishing a few scanlines and returning —
    /// handed back nearly a whole span that nothing could ever make up. A
    /// console that did that once ran a frame behind its peer forever,
    /// which is every one of its wireless replies landing outside the
    /// host's poll window: the pairings that would not associate.)
    ///
    /// 8: priming runs through the comm session's battle transition to
    /// the game's own battle-start routine — the new standing round
    /// anchor (`CodeOffsets::round_start`) — so a session, its
    /// recording and its round 1 all begin on the same tick. A
    /// recording made against the earlier handoff carries the
    /// transition tail at its head and its inputs land that far out of
    /// place.
    ///
    /// 9: the match end is the battle flow's own hand-back
    /// (`CodeOffsets::match_end`), ~3 ticks before the comm substate
    /// the old poll watched leaves the session — a session and its
    /// recording now end there.
    ///
    /// 3, 4, 5 and the parenthetical half of 7 were the emulator moving,
    /// not this cart — they belong to `BACKEND_SIM_VERSION` now, which is
    /// where the next one goes.
    fn sim_version(&self) -> u16 {
        9
    }

    fn prime(
        &self,
        link: &mut Link,
        match_type: (u8, u8),
        rng_seed: [u8; 16],
        events: &tango_match::telemetry::EventSink,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<(), tango_match::Error> {
        // The walk installs everything — its own traps AND the
        // standing lifecycle set, one table per CPU, never taken off —
        // and runs to the battle-start trap's firing on both consoles
        // (see `Layout::install`). Round 1's report lands in the sink
        // during the walk and becomes the tick-0 baseline; rounds 2/3,
        // the verdicts and the match end fire live. melonDS traps are
        // host state, so savestates and rollback never disturb them,
        // and their firings re-fire on re-simulation like any other.
        self.layout.walk(link, match_type, rng_seed, events, cancel)
    }

    /// Silent battles: the battle themes' own volumes, turned down to
    /// nothing where the sound driver reads them (see
    /// [`BATTLE_THEMES`] and [`priming::Layout::sound_archive`]).
    ///
    /// The sequence still loads and still starts — which is the whole
    /// point, because on this engine that load is a cartridge read
    /// worth a frame of the game's own clock, and a local setting may
    /// not spend a frame a peer doesn't (see
    /// [`GameSupport::silence_bgm`](tango_backend_melonds::GameSupport::silence_bgm)).
    /// Skipping the start instead was measured doing exactly that: the
    /// game's frame counter stood one ahead for the rest of the match.
    fn silence_bgm(&self, nds: &mut Nds) {
        let Some(archive) = self.layout.sound_archive(nds) else {
            log::warn!("{}: no sound archive loaded; not muting", self.layout.tag());
            return;
        };
        let muted = tango_backend_melonds::mute_sequences(nds, archive, BATTLE_THEMES);
        if muted != BATTLE_THEMES.len() {
            log::warn!(
                "{}: muted {muted} of {} battle themes",
                self.layout.tag(),
                BATTLE_THEMES.len()
            );
        }
    }

    /// The touch screen rides along for Team Battle and not for the
    /// plain subtypes. Same reading of `match_type.1` the walk makes
    /// (`== 0` is the team route off the Network board), so the pane
    /// and the priming route can't disagree about which mode this is.
    ///
    /// A plain battle leaves it dead once priming is past the comm
    /// screens — those are this cart's touch widgets, which is why the
    /// walk fabricates hit codes — so carrying it there spends half
    /// the pane on nothing.
    fn pvp_screens(&self, match_type: (u8, u8)) -> tango_backend_melonds::Screens {
        if match_type.1 == 0 {
            tango_backend_melonds::Screens::BOTH
        } else {
            tango_backend_melonds::Screens::UPPER
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
    /// The poller is LEVELS and chip fires only — the round/verdict/
    /// match lifecycle is the standing traps' (see [`Pvp::prime`]),
    /// exactly the mgba split.
    ///
    /// [`custom_self`]: tango_match::telemetry::CoreObs::custom_self
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<Nds>> {
        use tango_gamesupport_common::telemetry::{HandChipTracker, LoadedChip};
        use tango_match::telemetry::{CoreObs, EventSink, UnitObs};

        #[derive(Clone)]
        struct Poller {
            unit: u32,
            /// This player's own custom flag address (see [`Pvp::custom`]).
            custom: u32,
            /// This player's own hand block (see [`Pvp::chips`]).
            chip_block: u32,
            player: usize,
            chips: HandChipTracker,
        }
        impl tango_match::telemetry::CorePoller<Nds> for Poller {
            fn poll(&mut self, nds: &mut Nds, events: &EventSink, round: u32) -> Option<CoreObs> {
                // The unit block, read once: both slots owned by
                // distinct players is the sample this poller reports.
                // The game zeroes the block outside a live battle —
                // round intros, the interlude, the post-match screens.
                let mut slots = [None, None];
                {
                    let ram = nds.main_ram();
                    let mask = ram.len() - 1;
                    let read8 = |addr: u32| ram[(addr as usize - 0x0200_0000) & mask];
                    let read16 = |addr: u32| u16::from_le_bytes([read8(addr), read8(addr + 1)]);
                    for slot in 0..2 {
                        let base = self.unit + slot * UNIT_STRIDE;
                        let owner = read8(base + 0x16) as usize;
                        if let Some(cell) = slots.get_mut(owner) {
                            *cell = Some(UnitObs {
                                hp: read16(base + 0x24),
                                tile: (read8(base + 0x12), read8(base + 0x13)),
                            });
                        }
                    }
                }
                let live = match slots {
                    [Some(p0), Some(p1)] => Some([p0, p1]),
                    _ => None,
                };
                let units = live?;

                let ram = nds.main_ram();
                let mask = ram.len() - 1;
                let read8 = |addr: u32| ram[(addr as usize - 0x0200_0000) & mask];
                let read16 = |addr: u32| u16::from_le_bytes([read8(addr), read8(addr + 1)]);
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
        }

        Box::new(Poller {
            unit: self.unit,
            custom: self.custom + 0x20 * player as u32,
            chip_block: self.chips + 0x50 * player as u32,
            player,
            chips: Default::default(),
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

    /// Sites in the ARM9's code: what the walk traps and the branches
    /// it redirects into, then the battle's own lifecycle anchors.
    ///
    /// Each `*_gate` is where the game reads input and decides, and the
    /// address under it is the branch the press or touch it was looking
    /// for would have taken. Both sides of a pair sit in the same
    /// function, so the stack is untouched and whatever the handler had
    /// already loaded is still loaded — the redirect only answers the
    /// question the check was about to ask. ARM code except where
    /// noted; a jump keeps whatever instruction set it lands in.
    ///
    /// The three at the end are not the walk's: they are read rather
    /// than answered, and their traps stand for the pair's life (see
    /// [`Layout::lifecycle_traps`]).
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
        /// branch that opens the START menu. This is the **real world's**
        /// dispatcher; a save standing in the net runs the other one
        /// below, and every save runs exactly one of the two.
        field_start_gate: u32,
        field_start_menu_open: u32,
        /// The same pair for the dispatcher that runs while the save is
        /// **jacked in**, which is a separate function reached from a
        /// separate caller — six other functions sit between the two, and
        /// their callers differ (the real world's is the scene
        /// dispatcher's indirect call, this one's is Thumb code
        /// elsewhere). They share only their shape: both load the pressed
        /// halfword through the same pointer, both compare it against
        /// START, and both hand the same context in `r4`. There is no
        /// single site above or below that serves both — the one function
        /// they both call from their taken branch is a per-frame
        /// predicate that runs whether or not anything was pressed, so
        /// trapping it could not open a menu.
        ///
        /// Without this pair a save left in the net primes no further
        /// than the field: nothing else in the walk is reachable, so it
        /// fails with zero confirms and an unwritten cartridge — which
        /// reads exactly like a save that never unlocked NetBattle, and
        /// was long mistaken for one. Such a save opens the Network
        /// board perfectly well by hand.
        ///
        /// Found by covering the frame a scripted START press landed on
        /// and diffing against the same boot without it: the compare
        /// itself runs every frame either way and never shows up, but the
        /// branch it takes does, and it opens the identical
        /// predicate-and-call sequence the real world's does. JP verified
        /// against the cart, and carries its own shift (0x2f8, where the
        /// real world's function takes 0x2f4).
        net_start_gate: u32,
        net_start_menu_open: u32,
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
        /// The comparison deciding whether the button just taken needs
        /// the Navi Select screen before its comm screen. The board's
        /// tail turns the hit code into a selection index, writes the
        /// battle kind that index carries into the screen object's `+8`
        /// unconditionally, and then splits: the two team selections go
        /// to screen 7 (Navi Select, where a player builds their team),
        /// everything else straight to screen 3 (the comm screen). The
        /// site is that `cmp`, so the index's own value is already in
        /// the register the answer replaces — the same shape as
        /// [`name_registered_test`](CodeOffsets::name_registered_test),
        /// and answered the same way.
        ///
        /// Answering it "no" is what lets the team subtypes reuse the
        /// whole plain route: the kind byte is already written by the
        /// time the split is reached, so the comm screen still comes up
        /// as a Team Battle — the walk only declines the detour, having
        /// first written the team the detour would have collected (see
        /// [`RAMOffsets::team`]).
        ///
        /// The screen it declines is where a player chooses that team,
        /// by paging each of the two slots through the roster with the
        /// arrows beside it and committing with DOWNLOAD. Driving it is
        /// what a real pick needs, and it is the one screen on the route
        /// a redirect cannot answer; writing the block it fills is the
        /// way past that, not a way around the screen's own job.
        board_team_screen_test: u32,
        /// The file select's own store of the GBA-slot cross
        /// (`strb r1,[r0,#0x4c]` — the byte at
        /// [`CROSS_OFFSET`](crate::dataview::save::CROSS_OFFSET) of the
        /// loaded save image), which every boot runs *after* the save
        /// loads: the scene resets the cross before deciding what a
        /// cartridge in the GBA slot buys, and with no cartridge — which
        /// is every session here — nothing ever writes it back.
        ///
        /// That is the whole reason a trap is needed. The pick is the
        /// player's and it lives in the save, where the game keeps it;
        /// the game just refuses to carry it across a boot, exactly as
        /// BN4 refuses to carry a link navi. So the walk answers the
        /// store rather than jumping it: `r1` holds the 0 about to be
        /// written, and putting the save's own byte there means the
        /// game's own instruction writes the game's own value into the
        /// game's own field. Everything downstream — the battle build,
        /// the name the HUD prints, the exchange that tells the peer
        /// what it is fighting — is the game's, unmodified.
        ///
        /// A save with no cross picked leaves this alone, so a plain
        /// session's RAM is what it always was.
        cross_clear: u32,
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
        ///
        /// This one site is the whole answer. There was a second trap
        /// beside it that answered the flag's accessor at the source,
        /// for every asker, because an unregistered challenger seemed
        /// to make the game serve the whole comm at a crawl. That was
        /// never the game: it was the emulator reading uninitialised
        /// memory (melonds-rs `3a0c6c9`), and with a fresh console
        /// finally a function of its inputs the extra trap changes
        /// nothing — every host×joiner pairing primes and battles at
        /// the same speed without it, registered or not.
        name_registered_test: u32,
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

        /// The battle-start routine's completion — the mgba families'
        /// `round_start_ret`, on this cart: the per-round init path of
        /// the battle module's state handler (state byte [r5+3] == 0
        /// runs the whole init call chain exactly once per round, on
        /// the round's first frame, on both consoles) at the branch
        /// after its own `state = 4` store. A standing trap here IS the
        /// round lifecycle: the walk runs to its first firing (round 1,
        /// reported during priming into the tick-0 baseline) and rounds
        /// 2/3 fire live. Hunted by differential cover over a KO-forged
        /// triple set — first executed at round 1's init frame, again
        /// at round 2's, never during chip select, battle, or the
        /// interlude — and matched into the JP build by the same
        /// masked byte-match as every site above.
        round_start: u32,

        /// The comm-result verdict setter: `strb r0, [result+1]`, the
        /// one function the battle loop's own KO and judge paths call
        /// to report how the round came out, with the verdict in r0 —
        /// in the console's OWN perspective (each console mirrors the
        /// other's): 1 = this side won, 2 = lost, 3 = the judge's
        /// draw, 4/5/7 = the comm-abnormal exits, deliberately read as
        /// no verdict. The setter belongs to the comm-result globals'
        /// accessor suite (US result block 0x0216_f738, accessors
        /// 0x2097ca4..=0x2097d54; JP block 0x0216_84d8), found by
        /// elimination scan over forced KOs in the July telemetry
        /// work. The standing trap here IS the verdict lifecycle — the
        /// mgba families' round_end_set_* anchors in one site, the
        /// value riding the register instead of picking the site.
        /// Hunted like `round_start` (fires exactly once per decided
        /// round, at the poll-era verdict tick); JP by byte-match with
        /// the JP result literal.
        round_verdict: u32,

        /// The battle flow's hand-back: a tiny wrapper (`movs r0, #5;
        /// bl <set-flow-mode>`) in a cluster of mode setters, called
        /// exactly once — when the game's own battle set is decided and
        /// the comm session starts coming down — and never on the
        /// per-round teardowns. The mgba families'
        /// `comm_menu_end_battle_entry`, on this cart; it runs ~3 ticks
        /// before the substate word the old poll watched actually
        /// leaves BATTLE_SESSION. JP by the wrapper-cluster byte-match.
        match_end: u32,
    }

    /// The game's own variables, in main RAM: what the walk reads to
    /// know where it is, the few bytes it writes, and — last, and not
    /// the walk's — the one word the mute reads.
    struct RAMOffsets {
        /// Which scene is running. Zero is the attract movie, which
        /// ignores everything except a request to stop — and none of
        /// the sites above are reachable until it does, so left alone
        /// it loops forever. That is the only thing anything here reads
        /// it for: it is **not** a screen id, because over the field
        /// and the comm screens drawn on it the value is the overworld
        /// area the save is standing in, which is a property of the
        /// cartridge (see [`BATTLE_SESSION`]). Nothing may compare it
        /// against a constant.
        scene: u32,
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
        /// The team a Team Battle brings: four navi ids as words. The
        /// first two are the save's own and are already standing by the
        /// time the board dispatches; the last two are the pair Navi
        /// Select fills in, and are what the walk writes — the first
        /// two renamed into the space the second two use (see
        /// [`roster_id`]).
        ///
        /// Writing them *is* choosing them, the same way writing the
        /// save-select row is choosing a file: the game reads this block
        /// when it builds the battle and asks nothing about how it got
        /// filled. Nothing else has to be set — the screen touches a
        /// neighbouring flag on its way out, and leaving that alone
        /// changes nothing about the battle that follows.
        ///
        /// It does not survive the session. A cartridge dumped after a
        /// team is downloaded is byte-identical to one dumped after the
        /// screen is left empty, so a team cannot be brought along in a
        /// save and there is nothing here for the walk to read back — it
        /// has to be written every time.
        team: u32,
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

        /// Where the sound library keeps its pointer to the loaded
        /// sound archive — the one word the mute has to be told,
        /// because everything past it the archive names itself.
        ///
        /// The library's own route: this word holds the archive
        /// context, the context holds the INFO block at `+0x88`, and
        /// the block's header says where its sequence table lives.
        /// Taken from the literal pool of the library function that
        /// starts a sequence, which loads exactly this chain before it
        /// looks a sequence's record up. Nothing is searched for, so a
        /// wrong answer here reads as no archive rather than as some
        /// other bytes that happened to match.
        sound_archive_ptr: u32,
    }

    /// The save-select screen object's chosen row, as a byte offset into
    /// the object the handler is running on (`r5`). Both hit-test
    /// branches write the row here and the confirm reads it back, so
    /// writing it *is* choosing a row — no touch to fabricate.
    const SAVE_ROW_FIELD: u32 = 6;

    /// What a console's cartridge says it is playing: the file the game
    /// itself calls most recently saved, and what the walk reads out of
    /// it.
    ///
    /// Both halves come from the same file, so they are read together.
    /// The **row** is the save select's, which keeps a fixed row per
    /// save file rather than listing whatever exists — so a file is
    /// only reachable at its own row. The **cross** is the byte the
    /// game's file select clears on every boot (see
    /// [`CodeOffsets::cross_clear`]).
    ///
    /// Nothing rides beside the cartridge to say which file this is:
    /// the save editor stamps the picked file as the cartridge's newest
    /// (`Save::make_current`), so the bytes a peer commits and a
    /// recording stores already carry the answer, and both peers —
    /// which each hold both cartridges — read the same one. Row 0, the
    /// row the game's cursor starts on, for save memory the dataview
    /// cannot read.
    fn played_file(save: &[u8]) -> (u8, crate::dataview::save::Cross) {
        let Ok(set) = crate::dataview::save::SaveSet::parse(save) else {
            return (0, crate::dataview::save::Cross::None);
        };
        let file = set.current();
        (file.slot(), file.cross())
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
            net_start_gate:           0x0208_7528,
            net_start_menu_open:      0x0208_754c,
            start_menu_gate:          0x0208_4d68,
            start_menu_network:       0x0208_5044,
            script_timer_gate:        0x0209_b362,
            script_timer_expired:     0x0209_b36c,
            script_box_gate:          0x0209_ae68,
            script_box_dismissed:     0x0209_ae8a,
            pad_refresh_ret:          0x0200_0ce8,
            cross_clear:              0x0203_8968,
            board_touch_gate:         0x021e_0c88,
            board_touch_taken:        0x021e_0c98,
            board_code_load:          0x021e_0ca4,
            board_team_screen_test:   0x021e_1050,
            net_touch_gate:           0x021e_30c0,
            name_registered_test:     0x021d_f020,
            net_designate:            0x021e_3398,
            net_list_update:          0x021e_33f4,
            net_pick_row:             0x021e_3334,
            chooser_touch_gate:       0x021d_de14,
            chooser_first:            0x021d_de40,
            chooser_second:           0x021d_de30,
            round_start:              0x0208_e290,
            round_verdict:            0x0209_7cc4,
            match_end:                0x0208_b814,
        },
        ram: RAMOffsets {
            scene:                    0x0216_f71c,
            pressed:                  0x0215_f3d6,
            fade_steps:              [0x0216_fbc8, 0x0216_fbe8],
            text_delay:               0x0217_1594,
            wait_flags:               0x0216_f6e8,
            hit_code:                 0x021c_5db8,
            team:                     0x0216_f290,
            substate:                 0x021f_66ec,
            list_object:              0x021c_6260,
            list_count:               0x021c_688c,
            rngs:                    [0x0216_bb1c, 0x0216_f230, 0x0216_bb20],
            sound_archive_ptr:        0x0216_2d84,
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
            net_start_gate:           0x0208_7230,
            net_start_menu_open:      0x0208_7254,
            start_menu_gate:          0x0208_4a74,
            start_menu_network:       0x0208_4d50,
            script_timer_gate:        0x0209_b022,
            script_timer_expired:     0x0209_b02c,
            script_box_gate:          0x0209_ab28,
            script_box_dismissed:     0x0209_ab4a,
            pad_refresh_ret:          0x0200_0ce8,
            cross_clear:              0x0203_8760,
            board_touch_gate:         0x021d_98fc,
            board_touch_taken:        0x021d_990c,
            board_code_load:          0x021d_9918,
            board_team_screen_test:   0x021d_9cc4,
            net_touch_gate:           0x021d_bd3c,
            name_registered_test:     0x021d_7d60,
            net_designate:            0x021d_c014,
            net_list_update:          0x021d_c070,
            net_pick_row:             0x021d_bfb0,
            chooser_touch_gate:       0x021d_6b54,
            chooser_first:            0x021d_6b80,
            chooser_second:           0x021d_6b70,
            round_start:              0x0208_df98,
            round_verdict:            0x0209_7978,
            match_end:                0x0208_b51c,
        },
        ram: RAMOffsets {
            scene:                    0x0216_84bc,
            pressed:                  0x0215_8176,
            fade_steps:              [0x0216_8968, 0x0216_8988],
            text_delay:               0x0216_a334,
            wait_flags:               0x0216_8488,
            hit_code:                 0x021b_eb04,
            team:                     0x0216_8030,
            substate:                 0x021e_f06c,
            list_object:              0x021b_efa0,
            list_count:               0x021b_f5cc,
            rngs:                    [0x0216_48bc, 0x0216_7fd0, 0x0216_48c0],
            sound_archive_ptr:        0x0215_bb24,
        },
    };

    /// The scene the attract movie runs under, which is what
    /// `RAMOffsets::scene` reads until the save is loaded.
    const INTRO: u32 = 0;

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
    /// What separates the two ways a navi is named.
    ///
    /// Both halves of [`RAMOffsets::team`] name navis chosen on the same
    /// screen, but not from the same origin: the save's own team — the
    /// pair it has carried since the overworld, in the first two words —
    /// sits a fixed distance below the ids Navi Select writes into the
    /// other two. Adding it back is the whole conversion.
    ///
    /// Measured rather than guessed: on a file whose team is GyroMan and
    /// SearchMan, the own words read `0x256` and `0x257`, and picking
    /// those same two navis on the screen and committing with DOWNLOAD
    /// writes `0x278` and `0x279`. Both gaps are this, and the screen's
    /// roster runs consecutively from ProtoMan at `0x277`, so the two
    /// orders agree once the origin does.
    ///
    /// Nothing about the roster's *length* is known here, so a word that
    /// is not a navi at all — an empty slot, most obviously — must not
    /// be converted into one, which is what the zero check below is for.
    const NAVI_ID_OFFSET: u32 = 0x22;

    /// One of the save's own team navis as Navi Select would have named
    /// it, or `None` for a slot the save is not carrying anyone in.
    fn roster_id(own: u32) -> Option<u32> {
        (own != 0).then(|| own + NAVI_ID_OFFSET)
    }

    /// The hit code for the board's Team Battle entry, which is the
    /// next one. The board numbers its buttons down each column rather
    /// than across each row — its code load dispatches through a jump
    /// table, and the entry after Net Battle's is the button drawn
    /// underneath it, not the one beside it.
    const TEAM_BATTLE: u16 = 0x0061;
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
    /// The same three on the team route, which numbers the comm
    /// module's sub-screens one higher: every one of the joiner's
    /// prompts reads `0x04` where the plain route reads `0x03`, with
    /// the rest of the word — and the order they arrive in — identical.
    ///
    /// The **host's** prompt is deliberately not in here. Its accept is
    /// the one screen the two routes share outright, reading
    /// `CHOOSING[0]` whichever button opened the session, so the host
    /// keeps the plain list on both.
    ///
    /// Getting this wrong is quiet rather than loud: with the plain
    /// words on the team route the joiner's three prompts simply go
    /// unanswered, the host's own flow carries the session into a
    /// battle anyway, and the mode falls back to Single — a Triple Team
    /// match that plays one round and looks like a working walk. The
    /// regression test is that Single Team and Triple Team must not
    /// prime to identical RAM.
    const CHOOSING_TEAM: [u32; 3] = [0x0102_0403, 0x0104_0403, 0x0106_0403];
    // These words are matched EXACTLY, top byte included. An
    // unregistered-challenger pairing once appeared to run the comm
    // screens in a variant reading these low bytes under a 0 top byte,
    // and a second name-flag trap was added to suppress it. The variant
    // was the emulator, not the game — a fresh console was reading
    // uninitialised memory (melonds-rs `3a0c6c9`) — and neither the
    // variant nor the trap outlived that fix. If a new one ever does
    // show up, learn its exact word; don't mask.

    /// The ARM7 side of the save: its backup server's flash wait, at
    /// the function's entry. r0 arrives holding the mandatory pre-poll
    /// delay — 395 scanlines per 0x100-byte page, a real flash chip's
    /// program time — and r1 the poll timeout. The emulated flash is
    /// ready the moment it is asked, so the delay is the entire cost of
    /// the save: shortening r0 keeps the timeout-and-poll path, which
    /// still decides, and the wait that was most of "Saving..." simply
    /// isn't waited.
    ///
    /// **Capped at a tick rather than zeroed** (see
    /// [`FLASH_DELAY_CAP`]). On this engine's other cart, returning
    /// from this wait without ever sleeping left the comm route
    /// crawling afterwards — the sound engine's ready bit stopped
    /// settling, and a cartridge with no play on it took 1531 frames of
    /// link half against a played one's 197. One tick in place fixed it
    /// there for eleven frames of save. This is the same backup server
    /// on the same emulator, so it is capped here for the same reason;
    /// the failure has not been *seen* on this cart, which needs a
    /// fresh dump the save picker would accept, and a blank one can't
    /// stand in — it has no NetBattle unlocked, so it never reaches the
    /// board at all.
    const ARM7_FLASH_WAIT: u32 = 0x0380_33e4;

    /// What the pre-poll delay is shortened to — one of whatever it
    /// counts, and deliberately not zero. See [`ARM7_FLASH_WAIT`].
    const FLASH_DELAY_CAP: u32 = 1;
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
        /// The loaded sound archive's INFO block, walked the way the
        /// sound library walks it (see
        /// [`RAMOffsets::sound_archive_ptr`]), or `None` before anything
        /// has loaded one.
        pub(super) fn sound_archive(&self, nds: &mut Nds) -> Option<u32> {
            /// Where an archive context keeps its INFO block.
            const CONTEXT_INFO: u32 = 0x88;
            let context = nds.read32(self.ram.sound_archive_ptr);
            if context == 0 {
                return None;
            }
            let info = nds.read32(context + CONTEXT_INFO);
            (nds.read32(info) == u32::from_le_bytes(*b"INFO")).then_some(info)
        }

        /// Which build this is, for log lines.
        pub(super) fn tag(&self) -> &'static str {
            self.tag
        }

        /// One console's priming traps, in lifecycle order: boot, the
        /// Network board, the Net Battle screen, then the choosers.
        ///
        /// `host` picks which half of the Net Battle screen this console
        /// drives, `second` whether its chooser takes the mode screen's
        /// second button, and `team` which board button opens the route
        /// — the one screen that differs between them stands between the
        /// board and a comm screen both share. `save_row` and `cross`
        /// are what this console's cartridge says it is playing (see
        /// [`played_file`]). `confirms` counts the save prompts the run
        /// answers, and is only handed to one console — the two run the
        /// same route, so counting both would just double it.
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
            team: bool,
            save_row: u8,
            cross: crate::dataview::save::Cross,
            rng_seeds: [u32; 3],
            confirms: Option<std::sync::Arc<std::sync::atomic::AtomicU32>>,
        ) -> Vec<(u32, Box<dyn FnMut(&mut Nds)>)> {
            let code = &self.code;
            let ram = &self.ram;

            // One field dispatcher's START answer. Two of these are
            // installed because the game keeps one dispatcher per world
            // and they are neither the same function nor called from the
            // same place — see `CodeOffsets::net_start_gate`.
            let start_menu = |gate: u32, open: u32| -> (u32, Box<dyn FnMut(&mut Nds)>) {
                (
                    gate,
                    Box::new(move |nds: &mut Nds| {
                        for (&addr, &seed) in ram.rngs.iter().zip(&rng_seeds) {
                            nds.write32(addr, seed);
                        }
                        nds.jump_here(open)
                    }),
                )
            };

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
                    // The cross the save is carrying, put back into the
                    // register the file select is about to store from.
                    // The scene clears the byte on every boot before it
                    // asks what a GBA-slot cartridge buys — and with no
                    // cartridge, nothing asks — so this is what carries
                    // a picked cross across the load, the same shape as
                    // the save-select row above: the game's own store,
                    // answered rather than skipped. A save with no
                    // cross leaves the store alone, and the boot is
                    // byte-for-byte the one it always was.
                    // See [`CodeOffsets::cross_clear`].
                    code.cross_clear,
                    Box::new(move |nds: &mut Nds| {
                        if cross != crate::dataview::save::Cross::None {
                            nds.set_reg(1, cross.raw() as u32);
                        }
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
                // Both field dispatchers, each compared against START and
                // sent into the branch that opens the START menu. The rng
                // seeds ride this confirm (see [`RAMOffsets::rngs`]): the
                // overworld standing means the CONTINUE load's own reseeds
                // are done — seeded any earlier they'd be re-derived away —
                // and from here nothing but the accessors touches the
                // words, so what's written here is what the comm screens'
                // own settings generation draws from. Whichever world the
                // save is standing in answers; seeding is idempotent, so
                // nothing rests on it being only one.
                start_menu(code.field_start_gate, code.field_start_menu_open),
                start_menu(code.net_start_gate, code.net_start_menu_open),
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
                            if nds.read32(ram.scene) == INTRO {
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
                    // Battle, or Team Battle for the team subtype — the
                    // one place the two routes part. Writing the
                    // selection and letting the game's own handler act
                    // on it is the same idiom as the save confirm. It
                    // stops mattering by itself once the screen changes.
                    code.board_code_load,
                    Box::new(move |nds: &mut Nds| {
                        nds.write16(ram.hit_code, if team { TEAM_BATTLE } else { NET_BATTLE })
                    }),
                ),
                (
                    // Fill the team in, then decline the screen that
                    // would have filled it. The board's tail has already
                    // written the battle kind by the time it asks, so
                    // both halves of a Team Battle are settled here:
                    // the team the save is already carrying, renamed
                    // into the space the picks are written in (see
                    // [`roster_id`]), and the answer that sends the
                    // route to the comm screen the way Net Battle
                    // goes. The selection
                    // index arrives in r0 as "distance past the first
                    // team button"; anything above 1 is a selection
                    // that needs no team screen, which is the answer
                    // every non-team button gives for itself. Installed
                    // only on the team route — the plain route's
                    // buttons answer this correctly without help. See
                    // [`CodeOffsets::board_team_screen_test`].
                    code.board_team_screen_test,
                    Box::new(move |nds: &mut Nds| {
                        if !team {
                            return;
                        }
                        for slot in 0..2 {
                            if let Some(navi) = roster_id(nds.read32(ram.team + 4 * slot)) {
                                nds.write32(ram.team + 8 + 4 * slot, navi);
                            }
                        }
                        nds.set_reg(0, 2)
                    }),
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
                    //
                    // The team route renumbers the joiner's three (see
                    // [`CHOOSING_TEAM`]); the host's accept is the same
                    // screen on both, so it keeps the plain list.
                    let waits: &'static [u32] = match (host, team) {
                        (true, _) => &CHOOSING[..1],
                        (false, true) => &CHOOSING_TEAM,
                        (false, false) => &CHOOSING,
                    };
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

        /// The standing lifecycle set: the game's own battle start,
        /// verdict setter and battle-flow hand-back, in lifecycle
        /// order. Unlike the walk's answers above, these live for the
        /// pair's life — they sit in the battle's own code, which is
        /// what they are about.
        ///
        /// Starts and verdicts are console 0's reports (its local
        /// player is player 0, the game's host seat); the match end is
        /// reported from both consoles and the store dedups the second
        /// firing. `fired` is this console's handoff latch, which the
        /// walk runs until — the mgba primed latch, on this cart.
        fn lifecycle_traps(
            &'static self,
            host: bool,
            events: &tango_match::telemetry::EventSink,
            fired: std::sync::Arc<std::sync::atomic::AtomicBool>,
        ) -> Vec<(u32, Box<dyn FnMut(&mut Nds)>)> {
            let start_sink = host.then(|| events.clone());
            let verdict_sink = host.then(|| events.clone());
            let end_sink = events.clone();
            vec![
                (
                    // The game's own battle start: the priming handoff
                    // and, for console 0, the round lifecycle signal —
                    // round 1's firing lands during the walk and
                    // becomes the tick-0 baseline, and the later
                    // rounds' fire live.
                    self.code.round_start,
                    Box::new(move |_nds: &mut Nds| {
                        if let Some(sink) = &start_sink {
                            sink.round_started();
                        }
                        fired.store(true, std::sync::atomic::Ordering::Relaxed);
                    }) as Box<dyn FnMut(&mut Nds)>,
                ),
                (
                    // The verdict, from the game's own reporting call —
                    // the setter runs with it in r0; anything outside
                    // 1..=3 is the comm-abnormal path and deliberately
                    // reads as no verdict.
                    self.code.round_verdict,
                    Box::new(move |nds: &mut Nds| {
                        use tango_match::telemetry::Outcome;
                        let Some(sink) = &verdict_sink else { return };
                        match nds.reg(0) {
                            1 => sink.round_outcome(Outcome::P0Win),
                            2 => sink.round_outcome(Outcome::P1Win),
                            3 => sink.round_outcome(Outcome::Draw),
                            _ => {}
                        }
                    }),
                ),
                (
                    // The game's own match end: the battle flow's
                    // hand-back, run once when the set is decided.
                    self.code.match_end,
                    Box::new(move |_nds: &mut Nds| end_sink.match_ended()),
                ),
            ]
        }

        /// The ARM7's traps, which are the same on both builds. These
        /// are the only ones on that processor, and they answer the one
        /// wait no ARM9 redirect can reach: the backup server's per-page
        /// flash delay, which the emulated flash never needs.
        fn traps7() -> Vec<(u32, Box<dyn FnMut(&mut Nds)>)> {
            vec![
                (
                    ARM7_FLASH_WAIT,
                    Box::new(|nds: &mut Nds| {
                        let delay = nds.arm7_reg(0);
                        nds.arm7_set_reg(0, delay.min(FLASH_DELAY_CAP))
                    }),
                ),
                (
                    ARM7_FLASH_POLL_SLEEP,
                    Box::new(|nds: &mut Nds| nds.arm7_jump_here(ARM7_FLASH_POLL)),
                ),
            ]
        }

        /// Install the walk and the standing lifecycle set on both
        /// consoles, one table per CPU, sharing one count of how many
        /// confirms the run has needed.
        fn install(
            &'static self,
            link: &mut Link,
            second: bool,
            team: bool,
            rng_seed: [u8; 16],
            events: &tango_match::telemetry::EventSink,
            fired: &[std::sync::Arc<std::sync::atomic::AtomicBool>; 2],
        ) -> std::sync::Arc<std::sync::atomic::AtomicU32> {
            let confirms = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            for seat in 0..2 {
                let host = seat == 0;
                let (save_row, cross) = played_file(&link.console(seat).save_memory());
                let rng_seeds = console_rng_seeds(&rng_seed, seat);
                let mut traps = self.traps(
                    host,
                    second,
                    team,
                    save_row,
                    cross,
                    rng_seeds,
                    host.then(|| confirms.clone()),
                );
                traps.extend(self.lifecycle_traps(host, events, fired[seat].clone()));
                link.console(seat).set_traps(traps);
                link.console(seat).set_traps7(Self::traps7());
            }
            confirms
        }

        /// Retire the walk's answers at its finish line, leaving the
        /// standing lifecycle set — and nothing on the ARM7, whose
        /// traps are the save's alone.
        ///
        /// This is the one place this engine cannot copy the mgba
        /// families, whose primer traps simply stay: a GBA cart's menu
        /// address is menu code forever, where a DS cart's comm screens
        /// live in an **overlay the battle replaces**. Once the battle
        /// loads, a board gate's address is battle code, and answering
        /// it there would wreck the battle — measured doing exactly
        /// that on the sibling cart.
        fn retire_walk(
            &'static self,
            link: &mut Link,
            events: &tango_match::telemetry::EventSink,
            fired: &[std::sync::Arc<std::sync::atomic::AtomicBool>; 2],
        ) {
            for seat in 0..2 {
                link.console(seat)
                    .set_traps(self.lifecycle_traps(seat == 0, events, fired[seat].clone()));
                link.console(seat).set_traps7(Vec::new());
            }
        }

        /// Run both consoles from power-on into the agreed mode's link
        /// battle. Which file each console plays, and what cross it
        /// brings, come out of that console's own cartridge (see
        /// [`played_file`]); `rng_seed` is the negotiated match seed the
        /// walk reseeds the game's rngs from (see [`console_rng_seeds`]).
        /// Flipping `cancel` fails the walk with
        /// [`Cancelled`](tango_match::Error::Cancelled) instead of
        /// finishing it — replay boots run on host worker threads whose
        /// teardown joins them.
        pub fn walk(
            &'static self,
            link: &mut Link,
            match_type: (u8, u8),
            rng_seed: [u8; 16],
            events: &tango_match::telemetry::EventSink,
            cancel: Option<&std::sync::atomic::AtomicBool>,
        ) -> Result<(), tango_match::Error> {
            let started = std::time::Instant::now();
            let before = link.console(0).save_memory();
            // The handoff latches: set by the standing battle-start
            // trap on each console — what the walk runs until, exactly
            // the mgba primed latch.
            let fired = [
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ];
            // The registration lists Single first and Triple second, so
            // the mode is only which of the chooser's two buttons the
            // joiner takes — and the subtype, Team first and plain
            // second, only which board button opened the route. The
            // chooser is the same two buttons either way, which is what
            // makes the two independent.
            //
            // Team leads because it is what this pairing is for: the
            // lobby defaults to a mode's first subtype, and both of
            // these games are played as Team.
            let counter = self.install(link, match_type.0 != 0, match_type.1 == 0, rng_seed, events, &fired);

            // The boot half, which is over when the board stands: it
            // answers nothing that depends on the other console, so it
            // runs to a frame count and is checked afterwards.
            for _ in 0..BUDGET {
                if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
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
                return Err(tango_match::Error::PrimeTimeout(BUDGET));
            }
            if confirms < CONFIRMS_EXPECTED {
                log::warn!(
                    "{} priming saw {confirms} confirms, expected {CONFIRMS_EXPECTED}: \
                     does this save have NetBattle unlocked?",
                    self.tag
                );
                return Err(tango_match::Error::PrimeTimeout(BUDGET));
            }

            // The board half, which is over when both consoles have
            // reached the link battle's own session state — the last
            // step of the connect exchange, which the game holds from
            // there through the whole battle (see [`BATTLE_SESSION`],
            // and why no screen id may be compared against a constant
            // here). The wireless has to still be up with it: the
            // game's own comm-error exits tear the association down, so
            // a torn-down link is a stall however far the substate got.
            // Most of the wait is the two consoles associating, so this
            // waits on the game rather than on a count.
            let mut frames = 0;
            let battled = loop {
                if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                    return Err(tango_match::Error::Cancelled);
                }
                let substates = [
                    link.console(0).read32(self.ram.substate),
                    link.console(1).read32(self.ram.substate),
                ];
                if substates == [super::BATTLE_SESSION; 2] && link.connected() {
                    break true;
                }
                if frames >= BATTLE_BUDGET {
                    break false;
                }
                link.tick([HostInput::default(); 2]);
                frames += 1;
            };

            if !battled {
                // Enough state to place the stall without a debugger:
                // the substate says which comm screen each console is
                // parked on, the list word whether the joiner ever saw
                // the host advertise.
                log::warn!(
                    "{} priming: no battle {frames} frames past the board \
                     (connected={}, scenes {:#x}/{:#x}, \
                     substates {:#010x}/{:#010x}, list {:#010x}/{:#010x})",
                    self.tag,
                    link.connected(),
                    link.console(0).read32(self.ram.scene),
                    link.console(1).read32(self.ram.scene),
                    link.console(0).read32(self.ram.substate),
                    link.console(1).read32(self.ram.substate),
                    link.console(0).read32(self.ram.list_count),
                    link.console(1).read32(self.ram.list_count),
                );
                return Err(tango_match::Error::PrimeTimeout(BUDGET + frames));
            }
            // The comm session stands; the battle-start routine runs a
            // transition tail later. Prime through to its firing on
            // both consoles, so the session opens exactly where round 1
            // does.
            let mut tail = 0;
            while !(fired[0].load(std::sync::atomic::Ordering::Relaxed)
                && fired[1].load(std::sync::atomic::Ordering::Relaxed))
            {
                if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                    return Err(tango_match::Error::Cancelled);
                }
                if tail >= BATTLE_BUDGET {
                    log::warn!(
                        "{} priming: battle start never fired {tail} frames past the comm session",
                        self.tag
                    );
                    return Err(tango_match::Error::PrimeTimeout(BUDGET + frames + tail));
                }
                link.tick([HostInput::default(); 2]);
                tail += 1;
            }
            self.retire_walk(link, events, &fired);
            log::info!(
                "{} priming: match type {match_type:?}, battle transition {frames}+{tail} frames past the board, {:.1?} total",
                self.tag,
                started.elapsed()
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use tango_backend_melonds::{GameSupport, Screens};

    /// The subtype decides the pane, not the mode. Both registrations
    /// list Single first and Triple second with a Team and a plain
    /// subtype each, so Triple Team has to reach the touch screen for
    /// the same reason Single Team does — reading `match_type.0` here
    /// would give Triple Team a half-blind pane and leave the walk
    /// priming a team battle the player couldn't see.
    #[test]
    fn the_touch_screen_follows_the_team_subtype_in_either_mode() {
        for support in [&super::US, &super::JP] {
            assert_eq!(support.pvp_screens((0, 0)), Screens::BOTH, "single team");
            assert_eq!(support.pvp_screens((1, 0)), Screens::BOTH, "triple team");
            assert_eq!(support.pvp_screens((0, 1)), Screens::UPPER, "single");
            assert_eq!(support.pvp_screens((1, 1)), Screens::UPPER, "triple");
        }
    }

    /// Team is subtype 0 in both modes, which is what makes it the one
    /// the lobby offers first and defaults to.
    #[test]
    fn the_team_subtype_leads_each_mode() {
        for support in [&super::US, &super::JP] {
            for mode in 0..2u8 {
                assert_eq!(support.pvp_screens((mode, 0)), Screens::BOTH, "mode {mode} subtype 0");
            }
        }
    }

    /// A plain battle's pane is the upper screen alone, and a team
    /// battle's is the pair with the stylus target named. Pinned on
    /// the layouts rather than the selections because that is what a
    /// session sizes its framebuffer from and what the host places its
    /// stylus area by.
    #[test]
    fn the_plain_subtypes_present_one_screen_and_team_presents_two() {
        let plain = super::US.pvp_screens((0, 1)).layout();
        assert_eq!(plain.screens.len(), 1);
        assert_eq!(plain.touch, None);

        let team = super::US.pvp_screens((0, 0)).layout();
        assert_eq!(team.screens.len(), 2);
        assert_eq!(team.touch, Some(1));
    }
}

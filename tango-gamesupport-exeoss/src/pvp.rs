//! PvP-engine support: the priming walk.
//!
//! Nothing here touches the link protocol — the two consoles negotiate
//! for real over emulated local wireless. Priming is PC-redirects into
//! the game's own transition code, exactly as BN5DS's is: every menu
//! state, every sfx, every byte of the save is written by the game
//! itself, and **nothing below presses a button or is timed against the
//! clock**. Each answer is a standing one, unreachable until its own
//! screen is up, so the set is installed once at power-on and comes off
//! once the battle stands.
//!
//! Where BN5DS's comm screens are touch widgets — which is why its walk
//! has to fabricate hit codes — this cart's are all key-driven menus,
//! and every one of them reads the same newly-pressed halfword through
//! the same global context pointer. So every gate below is the same
//! shape: the `tst` a menu does against that halfword, redirected into
//! the branch the press would have taken. Where the branch acts on a
//! cursor, the cursor is set with it — writing the row *is* choosing
//! it, the same way BN5DS's save select is answered.
//!
//! The route is: the opening movie (skipped), the title's PRESS START,
//! the title menu's CONTINUE, the field's START, the START menu's
//! Network entry, the Network menu's own save, its Net Battle
//! (Practice) row, the parent/child seat pick, and — for the child —
//! the host's row in the list it scans. Every yes/no box along the way
//! is one shared widget, so one answer serves the save prompt, both
//! "start DS wireless?" prompts and the host's accept.
//!
//! Practice deliberately: the Real Thing row spends the players' own
//! win/loss records on the result, which is not netplay's to spend.
//!
//! The pair is symmetric, so the seats are assigned rather than
//! negotiated: **console 0 takes the game's parent seat and console 1
//! joins it**. Both peers walk both consoles, so both agree without
//! asking and nothing has to cross the wire.

use tango_backend_melonds::{Link, Nds};

/// The game's engine support. One release, so one set of addresses —
/// the `layout` indirection BN5DS needs for its two builds would be a
/// field with one value here.
pub struct Pvp;

/// What the registration's [`DsBackend`](tango_backend_melonds::DsBackend)
/// closes over.
pub static JP: Pvp = Pvp;

impl tango_backend_melonds::GameSupport for Pvp {
    fn prime(
        &self,
        link: &mut Link,
        match_type: (u8, u8),
        _session_payloads: [Option<&dyn tango_match::SessionPayload>; 2],
        rng_seed: [u8; 16],
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<(), tango_match::Error> {
        let _ = match_type;
        priming::walk(link, rng_seed, cancel)
    }

    /// The upper screen alone, in the one mode this cart has. Its
    /// netbattle plays entirely above: once priming has walked past
    /// the Network menus, nothing the player does reaches the touch
    /// screen, and carrying it would spend half the pane on a dead
    /// one. Regular play still gets both, since the same cart is a
    /// stylus game everywhere outside a link battle.
    fn pvp_screens(&self, _match_type: (u8, u8)) -> tango_backend_melonds::Screens {
        tango_backend_melonds::Screens::UPPER
    }

    /// The match lifecycle, as RAM facts — this engine's stand-in for
    /// the mgba families' trap anchors. Console 0 carries it, exactly
    /// as BN5DS's does.
    ///
    /// What it watches is the game's own scene byte, the same one
    /// priming's finish line reads: entering the battle's id is the
    /// battle starting, and coming back to the Network module's is the
    /// match over. Deliberately not *leaving* the battle's — see
    /// [`SCENE_NETWORK`](priming::SCENE_NETWORK): the DELETED banner,
    /// its jingle and the fade out all play after the battle scene has
    /// gone, and they are the end of the match a player watches.
    ///
    /// The LEVELS are both players' HP and where they stand, off the
    /// battle's own unit records ([`UNITS`](priming::UNITS)).
    /// Read on every console, since the record a console calls its own
    /// is the one it can be sure of; the two agree at every settled
    /// tick, which is what a rollback pair requires of anything it
    /// records.
    ///
    /// The chip select comes off this console's own battle phase (see
    /// [`BATTLE_PHASE`](priming::BATTLE_PHASE)), which is the
    /// one it can answer for: the two players commit independently and
    /// the field waits for the later of them.
    ///
    /// Chip fires come off the record the battle keeps of the use in
    /// flight (see [`CHIP_USE`](priming::CHIP_USE)), and each console
    /// reports only its own player's, so a use lands exactly once.
    ///
    /// The verdict is console 0's too, off the byte the game writes
    /// when it decides (see [`RESULT`](priming::RESULT)) — read through
    /// console 0's own player being player 0, the game's host seat.
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<Nds>> {
        use tango_match::telemetry::{CoreObs, EventSink, UnitObs};

        /// Console 0's watch: which scene has the screen and how the
        /// battle came out, both reported on their edges against last
        /// tick's readings.
        #[derive(Clone)]
        struct Lifecycle {
            /// Last tick's scene byte. `None` before the first, so the
            /// tick priming hands over on is the battle starting rather
            /// than a level with no edge.
            was: Option<u8>,
            /// Last tick's result byte, for the same reason — and here
            /// the edge is load-bearing rather than tidy, since the
            /// byte stops meaning the verdict a hundred frames later
            /// (see [`RESULT`](priming::RESULT)).
            result: Option<u8>,
        }

        impl Lifecycle {
            fn tick(&mut self, nds: &mut Nds, scene: u8, events: &EventSink) {
                match self.was {
                    None if scene == priming::SCENE_BATTLE => events.round_started(),
                    // The comm screen coming back, with everything the
                    // battle had left to play already played.
                    Some(was) if was != priming::SCENE_NETWORK && scene == priming::SCENE_NETWORK => {
                        events.match_ended()
                    }
                    _ => {}
                }
                self.was = Some(scene);

                // The verdict, as console 0 reads it — and console 0's
                // own player is player 0, the game's host seat, which
                // is what turns "I won" into an absolute outcome. Only
                // out of `0`, and only the two values there are: this
                // game's netbattle has no draw, so `Outcome::Draw` is
                // never reported and anything but a win or a loss is a
                // round that announced no verdict — which stays none
                // rather than being guessed at from HP.
                let result = nds.read8(priming::RESULT);
                if self.result == Some(0) {
                    match result {
                        1 => events.round_outcome(tango_match::telemetry::Outcome::P0Win),
                        2 => events.round_outcome(tango_match::telemetry::Outcome::P1Win),
                        _ => {}
                    }
                }
                self.result = Some(result);
            }
        }

        #[derive(Clone)]
        struct Poller {
            /// Which player this console's own navi is.
            player: usize,
            /// Console 0's alone (see [`Lifecycle`]).
            lifecycle: Option<Lifecycle>,
            /// The chip use standing last tick, as `(id, is this
            /// console's own player's)` — the edge against it is one
            /// chip fired.
            chip: Option<(u16, bool)>,
        }

        impl tango_match::telemetry::CorePoller<Nds> for Poller {
            fn poll(&mut self, nds: &mut Nds, events: &EventSink, _round: u32) -> Option<CoreObs> {
                let scene = nds.read8(priming::SCENE_BYTE);
                // Every tick, live or not: the lifecycle's whole job is
                // the scenes the read below bails out of.
                if let Some(lifecycle) = &mut self.lifecycle {
                    lifecycle.tick(nds, scene, events);
                }
                if scene != priming::SCENE_BATTLE {
                    return None;
                }

                use priming::{chip_use, unit};
                let mut slots = [None, None];
                for slot in 0..2 {
                    let base = priming::UNITS + slot * unit::STRIDE;
                    // The block is still whatever the last battle left
                    // in it for the first frames the battle scene has
                    // the screen — the fade in runs ahead of the units
                    // being built. A max HP of zero is the block saying
                    // it isn't one yet.
                    if nds.read16(base + unit::MAX_HP) == 0 {
                        continue;
                    }
                    // The records sit in a fixed order, but which of
                    // them a console drives is the console's own
                    // business, so each says so rather than being told.
                    let owner = if nds.read8(base + unit::IS_REMOTE) == 0 {
                        self.player
                    } else {
                        1 - self.player
                    };
                    slots[owner] = Some(UnitObs {
                        hp: nds.read16(base + unit::HP),
                        tile: (nds.read8(base + unit::TILE_X), nds.read8(base + unit::TILE_Y)),
                    });
                }
                let [Some(p0), Some(p1)] = slots else {
                    // Half a reading is no reading: an uninitialised
                    // block has both records claiming to be this
                    // console's, which lands them both in one slot.
                    return None;
                };

                // A chip fire, off the record the battle keeps of the
                // use in flight: an id standing where none stood is one
                // chip used. Only this console's own player's, since
                // the peer's console reports the peer's and a use
                // landing twice would double it.
                let chip = (nds.read8(priming::CHIP_USE + chip_use::LIVE) != 0)
                    .then(|| nds.read16(priming::CHIP_USE + chip_use::ID))
                    .filter(|&id| id != 0)
                    .map(|id| (id, nds.read8(priming::CHIP_USE + chip_use::IS_REMOTE) == 0));
                if chip != self.chip {
                    if let Some((id, true)) = chip {
                        events.chip_used(self.player, id);
                    }
                }
                self.chip = chip;

                Some(CoreObs {
                    units: [p0, p1],
                    // This console's own player's chip select, which is
                    // the one it can answer for — the two commit
                    // independently and the field waits for the later.
                    custom_self: nds.read8(priming::BATTLE_PHASE) == priming::PHASE_CUSTOM,
                })
            }
        }

        Box::new(Poller {
            player,
            lifecycle: (player == 0).then(|| Lifecycle {
                was: None,
                result: None,
            }),
            chip: None,
        })
    }
}

pub mod priming {
    use tango_backend_melonds::{Link, Nds};
    use tango_match::{HostInput, Link as _};

    /// Sites in the ARM9's code: what the walk traps, and the branches
    /// it redirects into. Every one of these is **Thumb**, and every
    /// `*_gate` sits in the same function as the address under it, so
    /// the stack is untouched and whatever the handler had already
    /// loaded is still loaded — the redirect only answers the question
    /// the check was about to ask.
    ///
    /// The addresses past the field split into two bands. The `0x0205`
    /// ones are the ARM9's static half — resident from power-on,
    /// whatever is on screen. The `0x021a` ones live in **overlay**
    /// memory, so each is only that function while its own module is
    /// loaded; each is nonetheless unreachable until its own screen is
    /// up, which is what makes installing the whole set at power-on
    /// safe.
    mod code {
        /// The opening movie's press-to-skip check, and the branch a
        /// press takes. The movie plays before the title and ignores
        /// everything else, so left alone the walk waits it out.
        pub const MOVIE_SKIP_GATE: u32 = 0x021a_54fe;
        pub const MOVIE_SKIPPED: u32 = 0x021a_5504;

        /// The title card's PRESS START check, and the branch that
        /// opens the menu under it. The card also counts a 3600-frame
        /// idle down into a demo loop; answering here lands long before
        /// that.
        pub const TITLE_PRESS_GATE: u32 = 0x021a_4816;
        pub const TITLE_PRESSED: u32 = 0x021a_482a;

        /// The title menu's confirm (it takes A or START), and the
        /// branch that runs the row under the cursor.
        pub const TITLE_MENU_GATE: u32 = 0x021a_492c;
        pub const TITLE_MENU_CONFIRM: u32 = 0x021a_493c;

        /// The overworld field dispatcher's compare against the
        /// menu-opening keys, and the branch that opens the START menu.
        pub const FIELD_START_GATE: u32 = 0x0205_da60;
        pub const FIELD_START_MENU_OPEN: u32 = 0x0205_da6c;

        /// The START menu's test-A, and the accepted path its row
        /// dispatch hangs off. The accept is past the menu's own
        /// availability check, which denies the Network and Save rows
        /// while the save is jacked into the net — the same
        /// two-dispatcher problem BN5DS solves with a second pair of
        /// sites, solved here by jumping the check the accept is
        /// gated behind rather than by finding its twin.
        pub const START_MENU_GATE: u32 = 0x021a_6dd6;
        pub const START_MENU_ACCEPT: u32 = 0x021a_6e00;

        /// The comparison deciding whether the Network screens have to
        /// collect a handle name first. The module asks its own
        /// "is a name registered" predicate (the name buffer's first
        /// halfword against the charset's empty marker) and, when the
        /// answer is no, walks into a touch keyboard the walk could not
        /// answer. The site is the `cmp` one instruction past the call,
        /// so the predicate's own value is already in the register the
        /// answer replaces.
        ///
        /// The registration is the *player's* to make and a match does
        /// not need it — but the game does, one screen later: the host
        /// advertises its name, and the child's list treats a row whose
        /// name starts with the empty marker as an empty row and
        /// refuses to pick it. So an unregistered save is also given a
        /// name to advertise (see [`super::PLACEHOLDER_NAME`]).
        pub const NAME_REGISTERED_TEST: u32 = 0x021a_8420;

        /// The Network menu's test-A, and the branch that runs the row
        /// under the cursor. The row arrives in `r4`, already loaded
        /// before the gate, which is why the answer sets the register
        /// rather than the cursor byte.
        pub const NET_MENU_GATE: u32 = 0x021a_85d6;
        pub const NET_MENU_ACCEPT: u32 = 0x021a_85de;

        /// The parent/child screen's test-A and its accepted path.
        /// Same shape: the row is in `r4`.
        pub const SEAT_GATE: u32 = 0x021a_9590;
        pub const SEAT_ACCEPT: u32 = 0x021a_9596;

        /// The child's host-list test-A. The answer here **sets the
        /// register rather than jumping**, so the game's own guard
        /// still runs: the branch it guards refuses a row whose name
        /// reads empty, which is exactly what an unfilled list looks
        /// like. Answering the question instead of skipping it is what
        /// lets this stand every frame the list screen is up and fire
        /// for real on the scan that finds the host.
        pub const LIST_PICK_GATE: u32 = 0x021a_9fa8;

        /// The shared yes/no box's poll, and the branch it takes once
        /// the box has been answered. One widget serves every prompt on
        /// the route — the save, both consoles' "start DS wireless?",
        /// and the parent's accept — and every one of them opens with
        /// its cursor on YES, which is the answer this hands back: the
        /// branch reads the box's own selection rather than being told
        /// one.
        pub const DIALOG_GATE: u32 = 0x021a_7c66;
        pub const DIALOG_ANSWERED: u32 = 0x021a_7c7e;

        /// The tail of the game's own RNG reset, one instruction after
        /// it has stored its baked constant. See
        /// [`RNG`](super::ram::RNG).
        pub const RNG_RESET_RET: u32 = 0x0205_fb12;

        /// **On the ARM7**: the entry of the cartridge backup server's
        /// wait-for-ready, which is where the save spends its time.
        /// `r0` arrives holding a mandatory pre-poll delay — a real
        /// flash chip's program time, scaled by the bytes just written
        /// — and `r1` a poll timeout.
        ///
        /// The emulated flash is ready the moment it is asked, so that
        /// delay is the entire cost of the save: the write loop passes
        /// a timeout of **zero**, which means the function sleeps the
        /// delay and never polls at all, and zeroing `r0` makes it
        /// return immediately instead. The whole 40-page bank then
        /// writes in 9 frames rather than 58, byte for byte the same
        /// cartridge. Any other caller — one that does pass a timeout —
        /// keeps its poll: with the delay zeroed the function still
        /// falls into the polling path, which is the half that actually
        /// decides.
        ///
        /// Found by covering the ARM7 across the save and diffing
        /// against a window without one (`--cover7`), which is the same
        /// recipe as the ARM9 sites. It is the same Nitro backup server
        /// BN5DS has, relocated: that cart's is at `0x038033e4`, and
        /// nothing at that address here is the wait.
        pub const ARM7_FLASH_WAIT: u32 = 0x0380_272c;
    }

    /// The game's own variables in main RAM: what the walk reads to
    /// know where it is, and the few bytes it writes.
    mod ram {
        /// The Network module's state, as three bytes: `+0` the module
        /// itself, `+1` which of its eight sub-screens is running, and
        /// `+2` that sub-screen's own step. Read as one word, which is
        /// what [`HOST_IN_BATTLE`](super::HOST_IN_BATTLE) compares.
        /// Only the stall log reads it now — the finish line is
        /// [`SCENE`] — but it is what says *where* on the comm route a
        /// walk that never got there stopped.
        pub const NET_STATE: u32 = 0x020b_b6c0;

        /// Which scene is running: `0xff` while one is loading, `0x02`
        /// the movie and the field, `0x32` the title, `0x09` the
        /// Network module, **`0x0f` a battle**, `0x12` the winner's
        /// post-KO banner. One byte, and it is the game's own statement
        /// of what has taken the screen over.
        pub const SCENE: u32 = 0x0202_4b39;

        /// The battle's phase, on **this console** — `0x04` while its
        /// own chip select is up, `0x08` once its player has committed
        /// and the field is theirs again. (`0x00` for the few frames
        /// either side of a KO, which the scene byte has already taken
        /// the telemetry out of.)
        ///
        /// One console's answer about one player, which is exactly what
        /// [`CoreObs::custom_self`](tango_match::telemetry::CoreObs) is
        /// for: with the two players' commits staggered, this flips on
        /// each console at its *own* player's commit — f142 and f358
        /// for confirms at f84 and f300 — where every shared phase byte
        /// nearby flips on both at once.
        ///
        /// Found by running two battles, one with the commits together
        /// and one staggered, and keeping the bytes that hold one value
        /// across every chip select (including the one reopened
        /// mid-battle) and another across every stretch of fighting,
        /// on both consoles, *and* move at the staggered commits.
        pub const BATTLE_PHASE: u32 = 0x020b_42bd;

        /// How the battle came out, **from this console's point of
        /// view**: `1` its own player won, `2` its own player lost, `0`
        /// undecided. Those are all the values there are — a netbattle
        /// here cannot be drawn. Set the frame the game decides, which
        /// is the same frame the winner's [`SCENE`] flips to its
        /// banner.
        ///
        /// **It does not stay the verdict.** About a hundred frames on,
        /// as the field fades, the game reuses the byte for the next
        /// screen's business — the winner's drops to `0` and the
        /// loser's to `1`. So it is read on its edge out of `0` and
        /// nowhere else; a report is standing until the round closes,
        /// which is exactly what the telemetry wants of it.
        ///
        /// Found by forcing a KO each way round and keeping the bytes
        /// whose two consoles' readings *swap* between the two runs.
        pub const RESULT: u32 = 0x0202_4b30;

        /// The chip a navi is using right now — one record, shared by
        /// both of them, holding the most recent use for as long as it
        /// plays out.
        ///
        /// Found by pressing A once mid-battle and diffing the frames
        /// around it: the id lands here two frames after the flag, and
        /// the byte before both says whose use it is.
        pub const CHIP_USE: u32 = 0x020b_4356;

        /// Fields within [`CHIP_USE`].
        pub mod chip_use {
            /// Zero when the console reading it is the one whose player
            /// used the chip — the same local/remote convention as
            /// [`unit::IS_REMOTE`](super::unit::IS_REMOTE), and the
            /// reason each console reports only its own player's fires:
            /// the peer's console reports the peer's, so a use lands
            /// exactly once.
            pub const IS_REMOTE: u32 = 0x00;
            /// Set while a use is playing out, cleared between — which
            /// is what makes a rising edge here one chip fired.
            pub const LIVE: u32 = 0x01;
            /// The chip's library id (u16), the same numbering the ROM
            /// assets' table uses, so the folder's name applies. Still
            /// zero for the first frames of a use, so a fire is only
            /// counted once this reads.
            pub const ID: u32 = 0x02;
        }

        /// The battle's two unit records, back to back — the levels the
        /// telemetry reports. There is a second pair at `0x020b7a80`
        /// (`+0x84` apart, HP at `+0x0c`) that mirrors the HP and was
        /// mapped first; it is **not** what the damage path writes, and
        /// zeroing it does nothing at all. These are the live ones:
        /// zeroing [`unit::HP`] here deletes a navi, banner and all.
        ///
        /// Found by dumping main RAM mid-battle and keeping every
        /// `u16 == 1000` beside a second `u16 == 1000` — seven pairs
        /// cart-wide, of which one kills.
        pub const UNITS: u32 = 0x020b_44f0;

        /// Fields within a unit record, from the head of the mapped
        /// part (the record's true start is somewhere before it, and
        /// nothing needs to know where).
        pub mod unit {
            /// One record to the next. The pair is in a fixed order,
            /// but [`IS_REMOTE`] is what says whose is whose.
            pub const STRIDE: u32 = 0xcc;
            /// Zero on the record this console drives, one on the
            /// peer's — so it reads the *opposite* way on the two
            /// consoles of a pair, which is exactly what makes it the
            /// thing to read: each console is certain about its own.
            pub const IS_REMOTE: u32 = 0x06;
            /// Where the unit stands, 1-based over the whole field:
            /// x 1..=6 left to right, y 1..=3 top to bottom — already
            /// the convention [`UnitObs`](tango_match::telemetry::UnitObs)
            /// wants. Verified by stepping one console's navi right,
            /// then up, and watching only that record move on *both*
            /// consoles.
            pub const TILE_X: u32 = 0x08;
            pub const TILE_Y: u32 = 0x09;
            /// In-battle HP, and the maximum it started at — the
            /// second doubling as the record's own "I am a battle"
            /// flag, since it is zero until the units are built.
            pub const HP: u32 = 0x10;
            pub const MAX_HP: u32 = 0x12;
        }

        /// The handle name the host advertises and the child's list
        /// shows: bytes in the game's own charset, terminated by
        /// [`NAME_EMPTY`](super::NAME_EMPTY) — which is also what the
        /// first byte reads when no name has ever been registered.
        pub const NAME: u32 = 0x0202_71a8;

        /// The game's single RNG word. It runs the GBA family's
        /// recurrence `x' = (rol1(x) + 1) ^ 0x873ca9e5` (found by
        /// searching the ARM9 for that constant), and unlike the GBA
        /// games' free-running pair it is **reset to a baked
        /// `0xa338244f` when a battle is set up** — so a pair that
        /// boots bit-identically every match would otherwise deal the
        /// identical battle every match. The reset runs exactly twice
        /// per session, at power-on and at the battle's setup, and
        /// never between rounds; writing the negotiated seed over it at
        /// its own tail is what makes each match its own.
        pub const RNG: u32 = 0x020b_99c4;
    }

    /// The title menu's cursor, as a byte offset into the object the
    /// handler is running on (`r5`). Its confirm reads the row back out
    /// of here, so writing it is choosing it.
    const TITLE_ROW_FIELD: u32 = 0x3ae;
    /// CONTINUE — the third row, after the prologue movie and NEW GAME.
    /// It is where a cart with a save already leaves the cursor; the
    /// walk writes it anyway, because confirming NEW GAME on a cart
    /// whose cursor sat elsewhere would run into name entry.
    const TITLE_ROW_CONTINUE: u8 = 2;

    /// The START menu's cursor, as a byte offset into its own object
    /// (`r5` again). The accept below stores a state the row dispatch
    /// reads later, so the cursor has to be right when it runs — which
    /// is why this is written into the object rather than into a
    /// register.
    const START_MENU_ROW_FIELD: u32 = 6;
    /// NETWORK — the sixth row, after Chip Folder, Data Library,
    /// MegaMan, E-Mail and Item, and before Save. The menu wraps at 7,
    /// which is its EXIT.
    const START_MENU_ROW_NETWORK: u8 = 5;

    /// Which register the two row menus have their row in by the time
    /// their gate is reached, and the rows the walk takes.
    const ROW_REG: u32 = 4;
    /// The same register at the child's list gate, where it holds the
    /// newly-pressed halfword the gate is about to test rather than a
    /// row — that gate is answered rather than jumped, so what goes in
    /// is the press.
    const PRESSED_REG: u32 = 4;
    /// Net Battle (Practice) — the second row of the Network menu,
    /// after Trade and before Net Battle (Real Thing) and Change Name.
    const NET_MENU_ROW_PRACTICE: u32 = 1;
    /// The parent row and the child row of the seat screen, in the
    /// order it lists them (the third row is "enter a number", which
    /// this route does not use).
    const SEAT_ROW_PARENT: u32 = 0;
    const SEAT_ROW_CHILD: u32 = 1;
    /// What the child's list gate has to see to run its pick: A, in the
    /// register its own `tst` is about to read.
    const PRESSED_A: u32 = 1;

    /// The charset's empty marker, which is also the name buffer's
    /// terminator. A save that has never registered a handle name has
    /// this as its first byte.
    const NAME_EMPTY: u8 = 0xe7;

    /// What an unregistered save advertises: `タンゴ` in the game's own
    /// charset (`0x00` space, `0x01..=0x0a` the digits, `0x0b` onward
    /// the katakana in gojūon order with the voiced forms following
    /// `ン`; all read off the game's own name field).
    ///
    /// Written into RAM only, and only once the cart's own save is
    /// already behind the walk, so a player's cartridge never comes
    /// back from a match carrying a name they didn't choose. It stands
    /// for the session and is gone with it; registering a real one is
    /// the player's to do, in the game's own name entry.
    const PLACEHOLDER_NAME: [u8; 4] = [0x1a, 0x38, 0x3f, NAME_EMPTY];

    /// What [`ram::SCENE`] reads once the battle has taken the screen
    /// over. Reaching it on both consoles is the walk's finish line.
    ///
    /// **This is deliberately later than the connect exchange
    /// finishing.** The comm screens hand off to the battle across a
    /// fade to white that takes about 85 frames, and the scene only
    /// flips at the top of it — so ending here means the first frame a
    /// player sees is the white the battle fades in from, rather than
    /// the Network menu they never chose to look at. The wireless is
    /// long up by then; what those frames cost is emulation, not
    /// waiting.
    pub(super) const SCENE_BATTLE: u8 = 0x0f;

    /// What [`ram::SCENE`] reads back on the Network menu — the comm
    /// screen the players came from, and the one the game returns them
    /// to when the battle is finally done with them. **This, not
    /// leaving [`SCENE_BATTLE`], is the match's end.**
    ///
    /// Leaving the battle scene is far too early, because what follows
    /// it is the part a player came to see. A KO runs: the DELETED
    /// banner and its jingle, then the field fading out, then the load
    /// back to the menu. Measured against a forced KO, that is the
    /// scene going `0x0f` → [`SCENE_RESULT`] at the banner → `0xff`
    /// (loading) about 105 frames later at the fade → `0x09` about 50
    /// frames after that. Ending on the first of those cuts the banner,
    /// the jingle and the whole fade out of the session and out of the
    /// recording.
    ///
    /// It also has to be this rather than the result scene, because the
    /// result scene is **one-sided**: only the console that won flips
    /// to it. The loser's stays on the battle until the fade. The two
    /// consoles reach `0xff` and `0x09` on the same tick, so anchoring
    /// here is the same instant for both players however the match went
    /// — which a per-seat anchor could not be.
    pub(super) const SCENE_NETWORK: u8 = 0x09;

    /// The winner's post-KO scene: the DELETED banner and the jingle
    /// over the frozen field. Named for the record rather than read —
    /// see [`SCENE_NETWORK`] for why the anchor is not here.
    #[allow(dead_code)]
    pub(super) const SCENE_RESULT: u8 = 0x12;

    /// [`ram::SCENE`], for the telemetry watch — what has the screen is
    /// the walk's business to know and the telemetry's business to
    /// watch.
    pub(super) const SCENE_BYTE: u32 = ram::SCENE;

    /// The battle's unit records, its phase and its chip uses, for the
    /// same reason: the walk found where they are, the telemetry is
    /// what reads them.
    pub(super) use ram::{chip_use, unit, BATTLE_PHASE, CHIP_USE, RESULT, UNITS};

    /// What [`ram::BATTLE_PHASE`] reads while this console's own chip
    /// select is up.
    pub(super) const PHASE_CUSTOM: u8 = 0x04;

    /// What [`ram::NET_STATE`] reads once the connect exchange is done,
    /// per seat: the module (`0x14`), its sub-screen (parent `4`, child
    /// `5`) and that screen's last step (`6`). The finish line used to
    /// be this; it is kept because a walk that stalls stalls *here*,
    /// and the two words say which side got how far.
    const HOST_CONNECTED: u32 = 0x0006_0414;
    const JOINER_CONNECTED: u32 = 0x0006_0514;

    /// How long the boot half is given — power-on to the cartridge
    /// write the Network menu insists on. Nothing is pressed and
    /// nothing branches on timing, so both consoles run the identical
    /// deterministic path every time: the save lands by about frame
    /// 470, and this carries a fifth again on top.
    ///
    /// What remains is what redirects cannot buy: the Capcom logo and
    /// the movie's prebuffer to about frame 190, the cart reads behind
    /// the save's load to about 390, and the save's own residual — nine
    /// frames of it, once the ARM7's flash delay is out of the way (see
    /// [`ARM7_FLASH_WAIT`](code::ARM7_FLASH_WAIT); it was fifty-eight
    /// before).
    const BOOT_BUDGET: u32 = 560;

    /// How long the link half is given. It takes about 200 frames from
    /// the save — roughly seventy of the parent standing itself up and
    /// the child scanning for it, and the screens around them. The
    /// budget carries several times that.
    const LINK_BUDGET: u32 = 2400;

    /// One console's seeds off the negotiated match seed — the mgba
    /// backend's `core_rng_seed` derivation carried over: identical on
    /// both peers (both walk both consoles), distinct between the
    /// consoles, exactly the situation the vanilla wireless protocol is
    /// built for. The recurrence has no stuck state, so no lane needs a
    /// zero guard.
    fn console_rng_seed(rng_seed: &[u8; 16], console: usize) -> u32 {
        let i = console * 4 % rng_seed.len();
        let v = u32::from_le_bytes(rng_seed[i..i + 4].try_into().unwrap());
        // Perturb by seat so an all-zero match seed still lands the two
        // consoles on distinct streams.
        v ^ 0x9e37_79b9u32.wrapping_mul(console as u32 + 1)
    }

    /// One console's priming traps, in lifecycle order: the movie, the
    /// title, the field, the START menu, the Network module, then the
    /// link screens.
    ///
    /// `host` picks which seat this console takes, which is the only
    /// thing that differs between them. These are host state rather
    /// than console state, so none of it is simulation the peers could
    /// disagree about: both install the same set, and from identical
    /// saves both take the same branches.
    fn traps(host: bool, rng_seed: u32) -> Vec<(u32, Box<dyn FnMut(&mut Nds)>)> {
        let mut traps: Vec<(u32, Box<dyn FnMut(&mut Nds)>)> = vec![
            (
                // The opening movie's skip. It stands for as long as the
                // movie does and answers it the first frame the check
                // runs.
                code::MOVIE_SKIP_GATE,
                Box::new(|nds: &mut Nds| nds.jump_here(code::MOVIE_SKIPPED)),
            ),
            (
                // The title card's PRESS START.
                code::TITLE_PRESS_GATE,
                Box::new(|nds: &mut Nds| nds.jump_here(code::TITLE_PRESSED)),
            ),
            (
                // The title menu, pointed at CONTINUE first: the row is
                // read back out of the object the confirm is running on,
                // so writing it is the same answer a press on that row
                // would have left behind.
                code::TITLE_MENU_GATE,
                Box::new(|nds: &mut Nds| {
                    let object = nds.reg(5);
                    nds.write8(object + TITLE_ROW_FIELD, TITLE_ROW_CONTINUE);
                    nds.jump_here(code::TITLE_MENU_CONFIRM)
                }),
            ),
            (
                // The field's START, into the branch that opens the menu.
                code::FIELD_START_GATE,
                Box::new(|nds: &mut Nds| nds.jump_here(code::FIELD_START_MENU_OPEN)),
            ),
            (
                // The START menu, pointed at NETWORK. The accept only
                // records that a row was taken; what runs it reads the
                // cursor a state later, so this one has to land in the
                // object rather than in a register.
                code::START_MENU_GATE,
                Box::new(|nds: &mut Nds| {
                    let object = nds.reg(5);
                    nds.write8(object + START_MENU_ROW_FIELD, START_MENU_ROW_NETWORK);
                    nds.jump_here(code::START_MENU_ACCEPT)
                }),
            ),
            (
                // Report the handle name as registered, so the module
                // picks the screen after the name entry rather than the
                // keyboard. The game's own comparison and its own branch
                // do the choosing; this only answers what its predicate
                // was asked.
                code::NAME_REGISTERED_TEST,
                Box::new(|nds: &mut Nds| nds.set_reg(0, 1)),
            ),
            (
                // The Network menu, pointed at Net Battle (Practice) —
                // and, for a save with no handle name of its own, the
                // one place a name can be given without the cartridge
                // carrying it away: the module's own save is already
                // behind this by the time the row is taken.
                code::NET_MENU_GATE,
                Box::new(|nds: &mut Nds| {
                    if nds.read8(ram::NAME) == NAME_EMPTY {
                        for (i, &byte) in PLACEHOLDER_NAME.iter().enumerate() {
                            nds.write8(ram::NAME + i as u32, byte);
                        }
                    }
                    nds.set_reg(ROW_REG, NET_MENU_ROW_PRACTICE);
                    nds.jump_here(code::NET_MENU_ACCEPT)
                }),
            ),
            (
                // The seat pick, which is the one answer that differs
                // between the two consoles.
                code::SEAT_GATE,
                Box::new(move |nds: &mut Nds| {
                    nds.set_reg(ROW_REG, if host { SEAT_ROW_PARENT } else { SEAT_ROW_CHILD });
                    nds.jump_here(code::SEAT_ACCEPT)
                }),
            ),
            (
                // Every yes/no box on the route, answered with the
                // selection it opens on — which is YES on all of them.
                code::DIALOG_GATE,
                Box::new(|nds: &mut Nds| nds.jump_here(code::DIALOG_ANSWERED)),
            ),
            (
                // The negotiated seed over the constant the game's own
                // reset has just stored (see [`ram::RNG`]). Standing
                // rather than one-shot: the reset runs at power-on and
                // again as the battle is set up, and it is the second
                // one that decides what the battle draws.
                code::RNG_RESET_RET,
                Box::new(move |nds: &mut Nds| nds.write32(ram::RNG, rng_seed)),
            ),
        ];
        if !host {
            traps.push((
                // The child's pick of the parent out of the list it
                // scans. This one answers rather than jumps, so the
                // game's own "is this row real" guard still runs: it
                // stands every frame the list is up, denies itself while
                // the list is empty, and takes the first row the frame
                // the scan reports the parent — which is the only
                // console advertising.
                code::LIST_PICK_GATE,
                Box::new(|nds: &mut Nds| nds.set_reg(PRESSED_REG, PRESSED_A)),
            ));
        }
        traps
    }

    /// The ARM7's traps, which are the one wait no ARM9 redirect can
    /// reach: the backup server's per-page flash delay, which the
    /// emulated flash never needs. See
    /// [`ARM7_FLASH_WAIT`](code::ARM7_FLASH_WAIT).
    fn traps7() -> Vec<(u32, Box<dyn FnMut(&mut Nds)>)> {
        vec![(code::ARM7_FLASH_WAIT, Box::new(|nds: &mut Nds| nds.arm7_set_reg(0, 0)))]
    }

    /// Install the walk on both consoles.
    ///
    /// The walk is all they are for: a trap set is a dispatch check the
    /// console pays for as long as it is installed, so both processors'
    /// sets come off again the moment priming is done and the match
    /// itself runs with none.
    fn install(link: &mut Link, rng_seed: [u8; 16]) {
        for seat in 0..2 {
            link.console(seat)
                .set_traps(traps(seat == 0, console_rng_seed(&rng_seed, seat)));
            link.console(seat).set_traps7(traps7());
        }
    }

    fn uninstall(link: &mut Link) {
        for seat in 0..2 {
            link.console(seat).set_traps(Vec::new());
            link.console(seat).set_traps7(Vec::new());
        }
    }

    /// Run both consoles from power-on into the link battle.
    /// `rng_seed` is the negotiated match seed the walk reseeds the
    /// game's rng from (see [`console_rng_seed`]). Flipping `cancel`
    /// fails the walk with [`Cancelled`](tango_match::Error::Cancelled)
    /// instead of finishing it — replay boots run on host worker
    /// threads whose teardown joins them.
    pub fn walk(
        link: &mut Link,
        rng_seed: [u8; 16],
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<(), tango_match::Error> {
        let started = std::time::Instant::now();
        let before = link.console(0).save_memory();
        install(link, rng_seed);

        let cancelled = |link: &mut Link| {
            if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                uninstall(link);
                return true;
            }
            false
        };

        // The boot half, which is over when the cartridge has been
        // written: the Network menu insists on a save before it will do
        // anything, and that write is the one observable saying the
        // menu was reached. It answers nothing that depends on the other
        // console, so it runs to a frame count and is checked
        // afterwards.
        for _ in 0..BOOT_BUDGET {
            if cancelled(link) {
                return Err(tango_match::Error::Cancelled);
            }
            link.tick([HostInput::default(); 2]);
        }
        let saved = link.console(0).save_memory() != before;
        log::info!(
            "exeoss priming: boot half at {BOOT_BUDGET} frames in {:.1?}, saved={saved}",
            started.elapsed()
        );
        if !saved {
            log::warn!("exeoss priming never saw the cartridge written; the Network menu was not reached");
            uninstall(link);
            return Err(tango_match::Error::PrimeTimeout(BOOT_BUDGET));
        }

        // The link half, which is over when the battle has taken the
        // screen over on both consoles (see [`SCENE_BATTLE`]). The
        // wireless has to still be up with it: the game's own comm-error
        // exits tear the association down, so a torn-down link is a
        // stall however far the scene got.
        let mut frames = 0;
        let battled = loop {
            if cancelled(link) {
                return Err(tango_match::Error::Cancelled);
            }
            let scenes = [link.console(0).read8(ram::SCENE), link.console(1).read8(ram::SCENE)];
            if scenes == [SCENE_BATTLE; 2] && link.connected() {
                break true;
            }
            if frames >= LINK_BUDGET {
                break false;
            }
            link.tick([HostInput::default(); 2]);
            frames += 1;
        };
        uninstall(link);

        if !battled {
            // Enough state to place the stall without a debugger: the
            // scene says what each console has on screen, and the
            // module word which comm screen it is parked on — the two
            // together separate "never connected" from "connected but
            // never reached the battle".
            log::warn!(
                "exeoss priming: no battle {frames} frames past the save \
                 (connected={}, scenes {:#04x}/{:#04x}, states {:#010x}/{:#010x}, \
                 wanted scene {SCENE_BATTLE:#04x} and states {HOST_CONNECTED:#010x}/{JOINER_CONNECTED:#010x})",
                link.connected(),
                link.console(0).read8(ram::SCENE),
                link.console(1).read8(ram::SCENE),
                link.console(0).read32(ram::NET_STATE),
                link.console(1).read32(ram::NET_STATE),
            );
            return Err(tango_match::Error::PrimeTimeout(BOOT_BUDGET + frames));
        }
        log::info!(
            "exeoss priming: battle transition {frames} frames past the save, {:.1?} total",
            started.elapsed()
        );
        Ok(())
    }
}

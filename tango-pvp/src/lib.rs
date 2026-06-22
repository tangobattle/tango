//! Rollback netcode for the PvP modes shared by all Battle Network games this
//! project supports. See `ARCHITECTURE.md` at the crate root for the full
//! map: glossary, the three drive loops (primary / shadow / stepper), the
//! engine state machines, determinism invariants, the error/panic policy,
//! and the test-coverage map. The toplevel pieces:
//!
//! - [`battle`]: connection-level `Match` and per-round `Round`. The live
//!   primary emulator drives these; `Match::start_round` allocates a Round,
//!   `Match::record_first_commit` snapshots its initial state, and
//!   `Match::end_round` tears it down. `Match::run` is the network receive
//!   loop that ferries remote inputs into the in-progress round, gated on a
//!   single watch channel that bumps on every round lifecycle event.
//!
//! - [`shadow`]: a second mgba core that mirrors the remote peer locally so
//!   the primary fastforwarder can ask "what was their packet at tick T?"
//!   without waiting on the network. `Shadow::apply_input` advances it one
//!   step; `advance_until_first_committed_state` / `advance_until_round_end`
//!   skip past round boundaries.
//!
//! - [`stepper`]: the per-frame battle simulator that replays inputs through the
//!   per-game stepper traps (`install_on_stepper`). A single re-sim core
//!   (`Stepper`) advances one tick at a time, capturing a snapshot each tick; it
//!   resumes forward in steady state and only reloads (`restore`) when the
//!   rollback engine rewinds to re-simulate a mispredicted tail. `State` drives
//!   replay-mode playback from boot. All share `InnerState` and the stepper trap
//!   set.
//!
//! - [`hooks`]: the trap framework. ROM PCs are registered with closures
//!   (`Trap`) that fire on hit.
//!
//! Per-game trap registrations (bn1..bn6, exe45) live outside this crate,
//! in the `tango-gamesupport-<game>::pvp_hooks` modules. Each game supplies
//! `common`/`primary`/`shadow`/`stepper` trap sets via the [`hooks::Hooks`]
//! trait this crate defines.
//!
//! - [`input`]: input plumbing. Confirmed input pairs are plain
//!   `(local, remote)` tuples — both same-typed (two
//!   [`PartialInput`](input::PartialInput)s) at the engine boundary, or a
//!   committed [`Input`](input::Input) against a remote
//!   [`PartialInput`](input::PartialInput) on the shadow/stepper paths.
//!
//! - [`replay`]: replay file format and replay-export pipeline.
//!
//! - [`net`]: `Sender` / `Receiver` traits for the in-flight network
//!   abstraction.
//!
//! Per-frame data flow on the primary:
//!
//! 1. The live core hits `main_read_joyflags`. The per-game primary trap
//!    fires; on first commit it calls `Match::record_first_commit` to set
//!    up the initial save state, then calls
//!    `Round::add_local_input_and_fastforward` every frame.
//! 2. `add_local_input_and_fastforward` sends the local input over the
//!    network, runs the Fastforwarder over committable + predicted-remote
//!    input pairs (asking the Shadow for predicted packets), commits the
//!    newly-confirmed remote inputs to the replay, loads the dirty state
//!    back into the live core, and updates the FPS target.
//! 3. The live core hits `comm_menu_send_and_receive_call` /
//!    `handle_input_*_send_and_receive_call`. Per-game shadow / stepper
//!    traps shovel the right RX packets into game RAM via the munger.
//! 4. On round end, `round_set_ending` / `round_ending_entry` fires and
//!    the per-game trap calls `Match::end_round`, which drops the round
//!    and advances the shadow past its matching round end.

pub mod battle;
pub mod hooks;
pub mod input;
pub mod net;
pub mod replay;
pub mod shadow;
pub mod stepper;

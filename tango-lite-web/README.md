# tango-lite-web

Tango, phone-sized, in a browser tab. Load a ROM, play it, or dial a
link code and play someone — patches included.

Everything below the UI is the workspace's own crates.
[`tango-session`] was written so a session is *driven* by its host
rather than running itself, [`tango-netplay`] so that bringing a
connection up is one linear future and the lobby is a plain state
machine, and [`tango-library`] so that all of its I/O goes through two
traits a frontend supplies. This crate is the browser end of those three
contracts and little else — the one thing it needed adding was
`tango_session::pvp::ReplayStore`, because recording a match was the
last place the live session still assumed a filesystem.

## What it is

| | |
|---|---|
| Play | Single-player, and live rollback netplay over the matchmaking server |
| Games | All seven families, same registry and the same `gamesupport-*` features as the desktop |
| Patches | The real `.tangopatch` catalog: index, install, apply, and auto-fetch what your opponent brings |
| Replays | Every match records itself; play back, scrub, import and export |
| Storage | ROMs, saves, patch packages and config, kept on the device |
| Input | gbaroll's slide-aware touch overlay, a keyboard, and gamepads |

Deliberately absent: the ROM scanner, the save editor, the results
screen, Discord presence, and any localization of the app's own words —
though the *games* are named properly, because every family already
ships Fluent fragments and it would be silly not to.

## Building

```sh
export WASI_SDK_PATH=/path/to/wasi-sdk    # mgba's C, compiled for wasm32
export LIBCLANG_PATH=$(brew --prefix llvm)/lib   # bindgen needs a wasm-aware libclang
./build.sh
python3 -m http.server -d dist 8080
```

`dist/` is a flat drop of static files — no server-side anything, and no
cross-origin isolation headers (nothing here uses `SharedArrayBuffer`).

The crate is a workspace member but **not** a default member: it only
builds for `wasm32-unknown-unknown`, and a host-target build stops at one
`compile_error!` rather than a page of unrelated noise. A plain root
`cargo build` skips it.

`build.sh` uses plain cargo + `wasm-bindgen` rather than `dx`. The Dioxus
CLI's value is hot reload and asset processing; this crate uses neither
(the stylesheet is a copied file, and the two worker scripts are
`include_str!`), so it is one fewer tool to have installed. `wasm-opt`
is used if present — the unoptimised module is ~14MB, most of it mgba.

## How it fits together

    library.rs   the user's ROMs/saves/patches — tango-library, arranged
      storage.rs   its Storage seam: a memory image mirrored to IndexedDB
      http.rs      its Http seam: fetch, read off the response stream
    loadout.rs   the current pick, and what it takes to run it
    engine.rs    the pump — three tick sources, one canvas
      audio.rs     an AudioWorklet, fed by a push pump
      input.rs     touch + keyboard + gamepad, folded into one joyflag word
    link.rs      netplay: spawn the connect future, pump the lobby, hand off
    recording.rs a match's ReplayStore: buffer, then one atomic put
    playback.rs  opening a recording: resolve its ROMs, boot, hand to the pump
    lang.rs      which locale the game names come out in
    app.rs       the shell, and the one place polling becomes reactivity
    ui/          the four screens, and the touch overlay

Six decisions are worth knowing about before changing anything.

**The engine, the library and the netplay state machine live in
thread-locals, not signals.** None of them is `Clone`, the engine is
touched sixty times a second, and what the UI wants off them is a
handful of numbers. So `app.rs` polls them at 10Hz and writes into a
signal only when the value actually changed. The corollary bites: a
component that reads one of them and takes no changing prop is memoized
and will happily draw stale data forever — which is why the revision
counter is threaded into every card on the library screen.

**Storage is a memory image with a persistence mirror, and that's why
it's IndexedDB rather than OPFS.** `tango_library::Storage` is
synchronous, because `apply_patch` and every session-construction path
read through it. The backend that could honour that natively is OPFS's
`createSyncAccessHandle()` — but that is worker-only, and this app is
main-thread-only. On the main thread OPFS is as asynchronous as anything
else and buys nothing IndexedDB doesn't. If emulation ever moves into a
worker, OPFS becomes the right backend and `storage.rs` is the only
thing that changes.

**Audio is pushed, not pulled.** The worklet processor runs on the audio
thread and can't reach this wasm module, so `Sink::pump` estimates how
far the worklet's ring has fallen below the latency target and posts
exactly that many frames. The iOS ringer switch is handled the way it
has to be: claim the `playback` audio session category where the API
exists (16.4+), and fall back to a looping silent media element where it
doesn't.

**Three things drive the pump, and that is not redundancy.**
`requestAnimationFrame` stops dead in a hidden tab; main-thread timers
get clamped to ~1Hz there; an `AudioContext` that never saw a user
gesture is suspended and reports nothing. So the guaranteed heartbeat is
a worker timer, with rAF for the visible case and the audio queue report
riding along for free. This is not hypothetical — with only rAF, a
backgrounded netplay tab stalls, and a stalled simulation isn't a local
inconvenience: it backs the peer's input queue up until their supervisor
gives the link up for dead. `pump_now` advances by elapsed wall clock,
so three uncoordinated callers drive one loop correctly.

**A recording is bytes to the host, not a file.** `ReplayStore::create`
names a recording and hands back somewhere to write it; the desktop
opens a file, and `recording.rs` buffers it and does one atomic put when
the match lets go. That release is the only moment available — the
writer is a `Box<dyn Write + Send>` held for the whole match, so there
is no completion callback, but there is a `Drop`, and it runs on both
the finished path and the abandoned one.

**The pad is gbaroll's overlay, and the D-pad is one control rather than
four buttons.** Where a finger lands inside its circle is the direction,
a dead zone in the middle is neutral, and `pointermove` re-steers
mid-press — so left → up-left → up is one continuous slide, which is how
a thumb moves and what a grid of nine buttons can't do. It floats over
the stage (`pointer-events: none` on the container, `auto` on the
controls) so the picture gets the whole viewport, which is also why a
phone held upright renders the session landscape and rotates it onto the
tall screen: the pad's corners have to land where fingers actually rest.
The touch maths survives that rotation because `offsetX`/`offsetY` map
through the inverse transform.

## Known gaps

- **No match-stats sidecar.** The desktop caches a cooked
  `MatchStats` next to each replay so its Replays tab doesn't
  re-simulate; there is no results screen here to feed, so
  `ReplayStore::root` returns `None` and nothing is written.
- **No picture-in-picture** on playback, and no clip export. The inset
  is a second screen's worth of pixels on a display that hasn't room
  for the first.
- **No signaling-free direct connections.** They need a UDP socket of
  their own, which a browser won't give out.
- **Priming blocks.** Booting a match primes both games to the link
  screen — seconds of emulation, with no thread to hide it on, so the
  page really does stop responding. The lobby says so rather than
  looking broken.
- **Mid-match reconnect is only lightly exercised** here. It is the same
  code the desktop runs, and it demonstrably fires and recovers, but a
  real lossy mobile link hasn't been through it.

[`tango-session`]: ../tango-session
[`tango-netplay`]: ../tango-netplay
[`tango-library`]: ../tango-library

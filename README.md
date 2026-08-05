# Tango

Tango is rollback netplay for Mega Man Battle Network.

## Building

```sh
cargo build --release --features=gamesupport-all
```

`gamesupport-all` turns on every game; a build can also take them one at
a time (`--features=gamesupport-bn6`, and so on) — see the feature list
in [`tango/Cargo.toml`](tango/Cargo.toml).

## Layout

The workspace splits along two seams: what the games are, and what runs
them.

Game support lives in the `tango-gamesupport-*` crates, one family per
game, each holding the ROM and save knowledge that family needs — a
`-dataview` crate for reading its saves and ROM assets, and a `-ui`
crate for its save editor. `tango-gamesupport` is the interface they all
implement, and the machinery they share splits the same way:
`tango-gamesupport-common-dataview` (the parsing substrate),
`tango-gamesupport-common-ui` (the editor shell), and
`tango-gamesupport-common` (the shared telemetry trackers). They
join the workspace as path dependencies rather than as members, so a
plain `cargo build` at the root skips their probe examples.

The engine is `tango-match` — a match over a pair of emulated consoles —
on top of a backend: `tango-backend-mgba` for the Game Boy Advance games
and `tango-backend-melonds` for the Nintendo DS one. `tango-session`
drives a running game, whether that is netplay, a replay or training,
and `tango` is the app around it. `tango-ui` is the look and feel shared
between frontends, and `tango-lobby` is the matchmaking server.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).

Tango links [melonDS](https://melonds.kuribo64.net/), which is GPL
licensed, so Tango as a whole is distributed under the GPL. That covers
the game-support crates as well: they used to live in a separate
repository under all-rights-reserved terms, and now they are here under
the same license as everything else.

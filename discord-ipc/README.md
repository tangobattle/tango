# discord-ipc

A small async **Discord rich-presence IPC client** in Rust — the local-RPC
transport that talks to a running Discord desktop client, split out of Tango as
a self-contained crate.

Discord exposes a local IPC endpoint — a Unix domain socket on Linux/macOS, a
named pipe on Windows — that desktop apps use to drive the "Playing …" rich
presence card and to receive join requests. This crate speaks that protocol:
the framed binary envelope, the handshake, the JSON command/event opcodes, and
the request/response nonce matching. It does **not** depend on Discord's native
`discord_game_sdk` — it is a pure Rust reimplementation of the wire protocol
over `tokio`.

The crate is deliberately thin: it owns the connection and the protocol, and
nothing above it. There is no reconnect loop, no global singleton, and no
opinion about *when* you set your activity — you bring the `tokio` runtime and
the policy.

## API

```rust
// 1. Connect to the local Discord client, identifying your application.
let (client, mut events) = discord_ipc::Client::connect(my_app_id).await?;

// 2. Subscribe to the events you care about.
client.subscribe(discord_ipc::Event::ActivityJoin).await?;

// 3. Push a rich-presence card.
client
    .set_activity(&discord_ipc::activity::Activity {
        details: Some("Match in progress".into()),
        state: Some("1-1".into()),
        ..Default::default()
    })
    .await?;

// 4. React to events (join secrets, etc.) off the channel.
while let Some((event, data)) = events.recv().await {
    // ...
}
```

- [`Client`] owns the connection. A background task started by `connect` reads
  incoming frames, answers Discord's pings, routes subscribed events to the
  `mpsc::Receiver` returned alongside the client, and matches command responses
  to their in-flight request by nonce. When the connection drops, the next call
  returns a `NotConnected` error — reconnection policy is the caller's.
- [`activity`] mirrors the shape Discord expects in a `SET_ACTIVITY` frame.
  Every optional field is skipped when `None` so unused fields drop out of the
  JSON rather than being sent as `null` (which Discord rejects).
- [`Event`] is the subset of dispatch events surfaced today (`Ready`,
  `ActivityJoin`, `Error`).

## Scope

Implemented: handshake, `SET_ACTIVITY`, `SUBSCRIBE`, ping/pong keepalive,
`ActivityJoin` dispatch. The protocol surface is driven by what Tango needs;
adding further commands/events is mechanical — extend the `Command`/`Event`
enums and add a setter mirroring `set_activity`.

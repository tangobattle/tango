import { tango } from "./proto/signaling";

const Packet = tango.signaling.Packet;
const AbortReason = Packet.Abort.Reason;

const X_SESSION_EXPIRES_AT_HEADER = "X-Session-Expires-At";
const EXPECTED_PROTOCOL_VERSION = 0x3b;
const SESSION_TTL_SECONDS = 60 * 60;

interface Attachment {
  isOfferer?: boolean;
}

async function getICEServers(
  env: Env,
): Promise<tango.signaling.Packet.Hello.IICEServer[]> {
  if (
    !env.CLOUDFLARE_TURN_SERVICE_ID ||
    !env.CLOUDFLARE_TURN_SERVICE_API_TOKEN
  ) {
    return DEFAULT_ICE_SERVERS;
  }
  try {
    const resp = await fetch(
      `https://rtc.live.cloudflare.com/v1/turn/keys/${env.CLOUDFLARE_TURN_SERVICE_ID}/credentials/generate`,
      {
        method: "POST",
        headers: {
          Authorization: `Bearer ${env.CLOUDFLARE_TURN_SERVICE_API_TOKEN}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ ttl: 86400 }),
      },
    );
    if (!resp.ok) {
      throw new Error(
        `TURN credentials generation error ${resp.status}: ${await resp.text()}`,
      );
    }
    const data = (await resp.json()) as {
      iceServers: { urls: string[]; credential?: string; username?: string };
    };
    return data.iceServers.urls.map((url) => ({
      credential: url.startsWith("stun:")
        ? null
        : (data.iceServers.credential ?? null),
      username: url.startsWith("stun:")
        ? null
        : (data.iceServers.username ?? null),
      urls: [url],
    }));
  } catch (e) {
    console.error("failed to request ICE servers:", e);
    return DEFAULT_ICE_SERVERS;
  }
}

const DEFAULT_ICE_SERVERS: tango.signaling.Packet.Hello.IICEServer[] = [
  "stun:stun.l.google.com:19302",
  "stun:stun1.l.google.com:19302",
  "stun:stun2.l.google.com:19302",
  "stun:stun3.l.google.com:19302",
  "stun:stun4.l.google.com:19302",
].map((uri) => ({ credential: null, username: null, urls: [uri] }));

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/ok") {
      return new Response("ok");
    }

    if (url.pathname !== "/") {
      return new Response("not found", { status: 404 });
    }

    const protocolVersionHeader = request.headers.get(
      "X-Tango-Protocol-Version",
    );
    if (protocolVersionHeader !== null) {
      const protocolVersion = parseInt(protocolVersionHeader, 16);
      if ((protocolVersion & 0xff) !== EXPECTED_PROTOCOL_VERSION) {
        return new Response(
          Packet.encode({
            abort: {
              reason:
                (protocolVersion & 0xff) < EXPECTED_PROTOCOL_VERSION
                  ? AbortReason.REASON_PROTOCOL_VERSION_TOO_OLD
                  : AbortReason.REASON_PROTOCOL_VERSION_TOO_NEW,
            },
          }).finish(),
          { status: 400 },
        );
      }
    }

    const sessionId = url.searchParams.get("session_id");
    if (!sessionId) {
      return new Response(
        Packet.encode({
          abort: { reason: AbortReason.REASON_MISSING_SESSION_ID },
        }).finish(),
        { status: 400 },
      );
    }

    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response(
        Packet.encode({
          abort: { reason: AbortReason.REASON_NOT_UPGRADE },
        }).finish(),
        { status: 400 },
      );
    }

    const expiresAt = Date.now() + SESSION_TTL_SECONDS * 1000;

    let doId: DurableObjectId;
    const existing = await env.SESSION_KV.get(sessionId);
    if (existing) {
      doId = env.MATCHMAKING.idFromString(existing);
    } else {
      doId = env.MATCHMAKING.newUniqueId();
      await env.SESSION_KV.put(sessionId, doId.toString(), {
        expiration: Math.floor(expiresAt / 1000),
      });
    }

    const stub = env.MATCHMAKING.get(doId);
    const doRequest = new Request(request, {
      headers: new Headers(request.headers),
    });
    doRequest.headers.set(X_SESSION_EXPIRES_AT_HEADER, expiresAt.toString());
    return stub.fetch(doRequest);
  },
} satisfies ExportedHandler<Env>;

export class MatchmakingSession implements DurableObject {
  state: DurableObjectState;
  env: Env;

  constructor(state: DurableObjectState, env: Env) {
    this.state = state;
    this.env = env;
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const sessionId = url.searchParams.get("session_id");
    if (sessionId == null) {
      throw new Error("missing session_id in url");
    }
    await this.state.storage.put("sessionId", sessionId);

    const xSessionExpiresAt = request.headers.get(X_SESSION_EXPIRES_AT_HEADER);
    if (xSessionExpiresAt == null) {
      throw new Error("missing X-Session-Expires-At header");
    }
    await this.state.storage.put("expiresAt", parseInt(xSessionExpiresAt, 10));

    const [client, server] = Object.values(new WebSocketPair());
    this.state.acceptWebSocket(server);

    const iceServers = await getICEServers(this.env);
    server.send(Packet.encode({ hello: { iceServers } }).finish());

    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(
    ws: WebSocket,
    message: string | ArrayBuffer,
  ): Promise<void> {
    if (typeof message === "string") {
      ws.close(1003, "unexpected text message");
      return;
    }

    let packet: tango.signaling.Packet;
    try {
      packet = Packet.decode(new Uint8Array(message));
    } catch {
      ws.close(1008, "invalid packet");
      return;
    }

    switch (packet.which) {
      case "start":
        await this.handleStart(ws, packet.start!);
        break;
      case "answer":
        await this.handleAnswer(ws, packet.answer!);
        break;
    }
  }

  async handleStart(
    ws: WebSocket,
    start: tango.signaling.Packet.IStart,
  ): Promise<void> {
    if (start.protocolVersion !== EXPECTED_PROTOCOL_VERSION) {
      ws.send(
        Packet.encode({
          abort: {
            reason:
              start.protocolVersion == null ||
              start.protocolVersion < EXPECTED_PROTOCOL_VERSION
                ? AbortReason.REASON_PROTOCOL_VERSION_TOO_OLD
                : AbortReason.REASON_PROTOCOL_VERSION_TOO_NEW,
          },
        }).finish(),
      );
      ws.close(1000);
      return;
    }

    const offerSdp = await this.state.storage.get<string>("offerSdp");
    if (offerSdp === undefined) {
      await this.state.storage.put("offerSdp", start.offerSdp ?? "");
      ws.serializeAttachment({ isOfferer: true } satisfies Attachment);
      const expiresAt = await this.state.storage.get<number>("expiresAt");
      await this.state.storage.setAlarm(
        expiresAt ?? Date.now() + SESSION_TTL_SECONDS * 1000,
      );
      return;
    }

    const sessionId = await this.state.storage.get<string>("sessionId");
    if (sessionId) {
      await this.env.SESSION_KV.delete(sessionId);
    }

    ws.send(
      Packet.encode({
        offer: { sdp: offerSdp },
      }).finish(),
    );
  }

  async handleAnswer(
    ws: WebSocket,
    answer: tango.signaling.Packet.IAnswer,
  ): Promise<void> {
    const offererWs = this.state.getWebSockets().find((s) => {
      const attachment = s.deserializeAttachment() as Attachment | null;
      return attachment?.isOfferer;
    });
    if (!offererWs) {
      ws.close(1008, "unexpected answer");
      return;
    }

    try {
      offererWs.send(
        Packet.encode({
          answer: { sdp: answer.sdp },
        }).finish(),
      );
      offererWs.close(1000);
    } catch {}

    ws.close(1000);
    await this.state.storage.deleteAll();
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    await this.cleanupWs(ws);
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    await this.cleanupWs(ws);
  }

  async alarm(): Promise<void> {
    for (const ws of this.state.getWebSockets()) {
      try {
        ws.close(1000);
      } catch {}
    }
    const sessionId = await this.state.storage.get<string>("sessionId");
    if (sessionId) {
      await this.env.SESSION_KV.delete(sessionId);
    }
    await this.state.storage.deleteAll();
  }

  async cleanupWs(ws: WebSocket): Promise<void> {
    const attachment = ws.deserializeAttachment() as Attachment | null;
    if (!attachment?.isOfferer) {
      return;
    }
    const sessionId = await this.state.storage.get<string>("sessionId");
    if (sessionId) {
      await this.env.SESSION_KV.delete(sessionId);
    }
    await this.state.storage.deleteAll();
  }
}

import { DurableObject } from "cloudflare:workers";
import { tango } from "./proto/signaling";

const Packet = tango.signaling.Packet;
const AbortReason = Packet.Abort.Reason;

// const EXPECTED_PROTOCOL_VERSION = 0x3b;
const HUB_SINGLETON_NAME = "global";

interface Attachment {
  sessionId: string;
  offerSdp?: string;
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

    const stub = env.MATCHMAKING.get(
      env.MATCHMAKING.idFromName(HUB_SINGLETON_NAME),
    );
    return stub.fetch(request);
  },
} satisfies ExportedHandler<Env>;

function wsTag(sessionId: string): string {
  return `s:${sessionId}`;
}

function findOfferer(
  ctx: DurableObjectState,
  sessionId: string,
): { ws: WebSocket; attachment: Attachment } | null {
  for (const ws of ctx.getWebSockets(wsTag(sessionId))) {
    const attachment = ws.deserializeAttachment() as Attachment | null;
    if (attachment?.offerSdp != null) {
      return { ws, attachment };
    }
  }
  return null;
}

export class MatchmakingHub extends DurableObject<Env> {
  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const sessionId = url.searchParams.get("session_id");
    if (sessionId == null) {
      throw new Error("missing session_id in url");
    }

    const [client, server] = Object.values(new WebSocketPair());
    this.ctx.acceptWebSocket(server, [wsTag(sessionId)]);
    server.serializeAttachment({ sessionId } satisfies Attachment);

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

    const attachment = ws.deserializeAttachment() as Attachment | null;
    if (!attachment) {
      ws.close(1011, "missing session attachment");
      return;
    }

    switch (packet.which) {
      case "start":
        this.handleStart(ws, attachment, packet.start!);
        break;
      case "answer":
        this.handleAnswer(ws, attachment, packet.answer!);
        break;
      case "ping":
        ws.send(Packet.encode({ ping: {} }).finish());
        break;
    }
  }

  handleStart(
    ws: WebSocket,
    attachment: Attachment,
    start: tango.signaling.Packet.IStart,
  ): void {
    const offerer = findOfferer(this.ctx, attachment.sessionId);
    if (offerer == null) {
      ws.serializeAttachment({
        ...attachment,
        offerSdp: start.offerSdp ?? "",
      } satisfies Attachment);
      return;
    }

    ws.send(
      Packet.encode({ offer: { sdp: offerer.attachment.offerSdp! } }).finish(),
    );
  }

  handleAnswer(
    ws: WebSocket,
    attachment: Attachment,
    answer: tango.signaling.Packet.IAnswer,
  ): void {
    const offerer = findOfferer(this.ctx, attachment.sessionId);
    if (offerer == null) {
      ws.close(1008, "unexpected answer");
      return;
    }

    try {
      offerer.ws.send(
        Packet.encode({ answer: { sdp: answer.sdp } }).finish(),
      );
      offerer.ws.close(1000);
    } catch {}

    ws.close(1000);
  }
}

import { DurableObject } from "cloudflare:workers";
import { tango } from "./proto/signaling";

const Packet = tango.signaling.Packet;
const AbortReason = Packet.Abort.Reason;

// const EXPECTED_PROTOCOL_VERSION = 0x3b;
const HUB_SINGLETON_NAME = "global";

interface Attachment {
  sessionId: string;
  offerSdp?: string;
  // Hex-encoded `connection_id` of the offer this socket is holding, if any.
  // Lets us recognize a reconnecting offerer (same id, fresh offer) and replace
  // its stale offer rather than treating the new socket as the answering peer.
  connectionId?: string;
}

// Hex-encode a `connection_id`, treating an empty/absent value as "none" so it
// never matches a real id.
function encodeConnectionId(
  connectionId: Uint8Array | null | undefined,
): string | undefined {
  if (connectionId == null || connectionId.length === 0) {
    return undefined;
  }
  let hex = "";
  for (const byte of connectionId) {
    hex += byte.toString(16).padStart(2, "0");
  }
  return hex;
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
  // Params are left unannotated so `satisfies ExportedHandler<Env>` types
  // `request` as an incoming request — that's what gives `request.cf` its
  // `tlsClientAuth` (mTLS client-certificate) shape below.
  async fetch(request, env): Promise<Response> {
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

    // mTLS client identity. When the matchmaking hostname has mTLS enabled,
    // Cloudflare populates `cf.tlsClientAuth` from the self-signed certificate
    // Tango presents on the websocket (see the client's `identity` module).
    // Log its SHA-256 fingerprint — the client's persistent identity — so a
    // connection can be tied back to an install. `certVerified` reads
    // "FAILED:self signed certificate" for these certs (there's no CA chain to
    // validate, by design); we want the fingerprint, not verification.
    // `tlsClientAuth` is absent entirely when mTLS isn't configured on the
    // hostname, and `certPresented` is "0" when a client connects without one.
    const tlsClientAuth = request.cf?.tlsClientAuth;
    if (tlsClientAuth?.certPresented === "1") {
      console.log(
        `session ${sessionId}: client cert fingerprint sha256=${tlsClientAuth.certFingerprintSHA256} (verify: ${tlsClientAuth.certVerified})`,
      );
    } else {
      console.log(`session ${sessionId}: no client certificate presented`);
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
    const connectionId = encodeConnectionId(start.connectionId);
    const offerer = findOfferer(this.ctx, attachment.sessionId);

    if (offerer == null) {
      // No one is waiting yet: become the offerer.
      ws.serializeAttachment({
        ...attachment,
        offerSdp: start.offerSdp ?? "",
        connectionId,
      } satisfies Attachment);
      return;
    }

    if (connectionId != null && connectionId === offerer.attachment.connectionId) {
      // Same `connection_id` as the offer already on file: this is the offerer
      // reconnecting with a fresh offer (its previous socket dropped before the
      // peer arrived). Replace the stale offer with this one and evict the old
      // socket, so the answering peer is handed the live offer.
      ws.serializeAttachment({
        ...attachment,
        offerSdp: start.offerSdp ?? "",
        connectionId,
      } satisfies Attachment);
      if (offerer.ws !== ws) {
        // Clear the stale socket's offer first so it can't be picked as the
        // offerer during the brief window before its close completes.
        offerer.ws.serializeAttachment({
          sessionId: offerer.attachment.sessionId,
        } satisfies Attachment);
        try {
          offerer.ws.close(1000);
        } catch {}
      }
      return;
    }

    // A different peer: hand it the offerer's SDP so it can answer.
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

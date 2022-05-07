import { subscribe } from "event-iterator/lib/dom";
import fetch from "node-fetch";

import {
    CreateStreamToClientMessage, CreateStreamToServerMessage, GameInfo, JoinStreamToClientMessage,
    JoinStreamToServerMessage, Patch, QueryRequest, QueryResponse, Settings
} from "./protos/lobby";

export { GameInfo, Settings, QueryResponse };

async function* wrapMessageStream(ws: WebSocket) {
  for await (const msg of subscribe.call(ws, "message")) {
    yield (msg as MessageEvent<ArrayBuffer>).data;
  }
}

interface OpponentInfo {
  opponentId: string;
  gameInfo: GameInfo;
}

interface NegotiatedSession {
  sessionId: string;
  saveData: Uint8Array;
}

interface LobbyJoinHandle {
  opponentInfo: OpponentInfo;
  negotiatedSession: Promise<NegotiatedSession | null>;
}

export async function join(
  addr: string,
  identityToken: string,
  lobbyId: string,
  gameInfo: GameInfo,
  saveData: Uint8Array,
  { signal }: { signal?: AbortSignal } = {}
): Promise<LobbyJoinHandle> {
  const ws = new WebSocket(`${addr}/join`);
  if (signal != null) {
    signal.onabort = () => {
      ws.close();
    };
  }
  ws.binaryType = "arraybuffer";
  ws.onopen = () => {
    ws.send(
      JoinStreamToServerMessage.encode({
        joinReq: {
          identityToken,
          lobbyId,
          gameInfo,
          saveData,
        },
      }).finish()
    );
  };
  const stream = wrapMessageStream(ws);
  ws.onclose = () => {
    stream.return();
  };
  const { value: raw, done } = await stream.next();
  if (done) {
    throw "stream ended early";
  }
  const resp = JoinStreamToClientMessage.decode(new Uint8Array(raw));
  if (resp.joinResp == null) {
    throw `unexpected response: ${JoinStreamToClientMessage.toJSON(resp)}`;
  }

  if (resp.joinResp.gameInfo == null) {
    throw "missing game info";
  }

  const opponentInfo = {
    opponentId: resp.joinResp.opponentId,
    gameInfo: resp.joinResp.gameInfo,
  };

  return {
    opponentInfo,

    negotiatedSession: (async () => {
      const { value: raw, done } = await stream.next();
      if (done) {
        return null;
      }

      const resp = JoinStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.acceptInd == null) {
        throw `unexpected response: ${JoinStreamToClientMessage.toJSON(resp)}`;
      }

      return {
        sessionId: resp.acceptInd.sessionId,
        saveData: resp.acceptInd.saveData,
      };
    })(),
  };
}

interface LobbyCreateHandle {
  lobbyId: string;
  nextOpponent(): Promise<OpponentInfo | null>;
  accept(opponentId: string): Promise<NegotiatedSession>;
  reject(opponentId: string): Promise<void>;
}

export async function create(
  addr: string,
  identityToken: string,
  gameInfo: GameInfo,
  availablePatches: Patch[],
  settings: Settings,
  saveData: Uint8Array,
  { signal }: { signal?: AbortSignal } = {}
): Promise<LobbyCreateHandle> {
  const ws = new WebSocket(`${addr}/create`);
  if (signal != null) {
    signal.onabort = () => {
      ws.close();
    };
  }
  ws.binaryType = "arraybuffer";
  ws.onopen = () => {
    ws.send(
      CreateStreamToServerMessage.encode({
        createReq: {
          identityToken,
          gameInfo,
          availablePatches,
          settings,
          saveData,
        },
        acceptReq: undefined,
        rejectReq: undefined,
      }).finish()
    );
  };
  const stream = wrapMessageStream(ws);
  ws.onclose = () => {
    stream.return();
  };
  const { value: raw, done } = await stream.next();
  if (done) {
    throw "stream ended early";
  }
  const resp = CreateStreamToClientMessage.decode(new Uint8Array(raw));
  if (resp.createResp == null) {
    throw `unexpected response: ${CreateStreamToClientMessage.toJSON(resp)}`;
  }

  const lobbyId = resp.createResp.lobbyId;

  return {
    lobbyId,

    async nextOpponent() {
      const { value: raw, done } = await stream.next();
      if (done) {
        return null;
      }

      const resp = CreateStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.joinInd == null) {
        throw `unexpected response: ${CreateStreamToClientMessage.toJSON(
          resp
        )}`;
      }

      if (resp.joinInd.gameInfo == null) {
        throw "missing game info";
      }

      return {
        opponentId: resp.joinInd.opponentId,
        gameInfo: resp.joinInd.gameInfo,
      };
    },

    async accept(opponentId: string) {
      ws.send(
        CreateStreamToServerMessage.encode({
          createReq: undefined,
          acceptReq: {
            opponentId,
          },
          rejectReq: undefined,
        }).finish()
      );

      const { value: raw, done } = await stream.next();
      if (done) {
        throw "stream ended early";
      }

      const resp = CreateStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.acceptResp == null) {
        throw `unexpected response: ${CreateStreamToClientMessage.toJSON(
          resp
        )}`;
      }

      return {
        sessionId: resp.acceptResp.sessionId,
        saveData: resp.acceptResp.saveData,
      };
    },

    async reject(opponentId: string) {
      ws.send(
        CreateStreamToServerMessage.encode({
          createReq: undefined,
          acceptReq: undefined,
          rejectReq: {
            opponentId,
          },
        }).finish()
      );

      const { value: raw, done } = await stream.next();
      if (done) {
        throw "stream ended early";
      }

      const resp = CreateStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.rejectResp == null) {
        throw `unexpected response: ${CreateStreamToClientMessage.toJSON(
          resp
        )}`;
      }
    },
  };
}

export async function query(
  addr: string,
  identityToken: string,
  lobbyId: string,
  { signal }: { signal?: AbortSignal } = {}
) {
  const httpResp = await fetch(`${addr}/query`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-protobuf",
    },
    body: Buffer.from(QueryRequest.encode({ identityToken, lobbyId }).finish()),
    signal,
  });

  if (httpResp.status != 200) {
    throw `unexpected status: ${httpResp.status}`;
  }

  return QueryResponse.decode(new Uint8Array(await httpResp.arrayBuffer()));
}

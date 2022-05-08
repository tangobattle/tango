import { subscribe } from "event-iterator/lib/dom";
import { promisify } from "util";
import * as zlib from "zlib";

import {
    CreateStreamToClientMessage, CreateStreamToServerMessage, GameInfo, GetInfoRequest,
    GetInfoResponse, JoinRequest, JoinResponse, Settings
} from "./protos/lobby";

export { GameInfo, Settings };

function wrapMessageStream(ws: WebSocket) {
  const stream = (async function* () {
    for await (const msg of subscribe.call(ws, "message")) {
      yield (msg as MessageEvent<ArrayBuffer>).data;
    }
  })();
  ws.onclose = () => {
    stream.return();
  };
  ws.onerror = (e) => {
    stream.throw((e as any).code);
  };
  return stream;
}

interface OpponentInfo {
  nickname: string;
  gameInfo: GameInfo;
}

interface NegotiatedSession {
  sessionId: string;
  opponentSaveData: Uint8Array;
}

export async function join(
  addr: string,
  lobbyId: string,
  nickname: string,
  gameInfo: GameInfo,
  saveData: Uint8Array,
  { signal }: { signal?: AbortSignal } = {}
): Promise<NegotiatedSession | null> {
  const httpResp = await fetch(`${addr}/join`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-protobuf",
    },
    body: Buffer.from(
      JoinRequest.encode({ lobbyId, nickname, gameInfo, saveData }).finish()
    ),
    signal,
  });

  if (httpResp.status != 200) {
    throw `unexpected status: ${httpResp.status}`;
  }

  const resp = JoinResponse.decode(
    new Uint8Array(await httpResp.arrayBuffer())
  );

  return {
    sessionId: resp.sessionId,
    opponentSaveData: await promisify(zlib.brotliDecompress)(
      resp.opponentSaveData
    ),
  };
}

interface LobbyCreateHandle {
  lobbyId: string;
  waitForOpponent(): Promise<OpponentInfo | null>;
  accept(saveData: Uint8Array): Promise<NegotiatedSession | null>;
}

export async function create(
  addr: string,
  nickname: string,
  gameInfo: GameInfo,
  availableGames: GameInfo[],
  settings: Settings,
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
          nickname,
          gameInfo,
          availableGames,
          settings,
        },
        acceptReq: undefined,
      }).finish()
    );
  };
  const stream = wrapMessageStream(ws);
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

    async waitForOpponent() {
      const { value: raw, done } = await stream.next();
      if (done) {
        return null;
      }

      const resp = CreateStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.proposeInd == null) {
        throw `unexpected response: ${CreateStreamToClientMessage.toJSON(
          resp
        )}`;
      }

      if (resp.proposeInd.gameInfo == null) {
        throw "missing game info";
      }

      return {
        nickname: resp.proposeInd.opponentNickname,
        gameInfo: resp.proposeInd.gameInfo,
      };
    },

    async accept(saveData: Uint8Array) {
      ws.send(
        CreateStreamToServerMessage.encode({
          createReq: undefined,
          acceptReq: {
            saveData: await promisify(zlib.brotliCompress)(saveData),
          },
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
        opponentSaveData: await promisify(zlib.brotliDecompress)(
          resp.acceptResp.opponentSaveData
        ),
      };
    },
  };
}

export async function getInfo(
  addr: string,
  lobbyId: string,
  { signal }: { signal?: AbortSignal } = {}
) {
  const httpResp = await fetch(`${addr}/query`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-protobuf",
    },
    body: Buffer.from(GetInfoRequest.encode({ lobbyId }).finish()),
    signal,
  });

  if (httpResp.status != 200) {
    throw `unexpected status: ${httpResp.status}`;
  }

  return GetInfoResponse.decode(new Uint8Array(await httpResp.arrayBuffer()));
}

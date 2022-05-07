import { subscribe } from "event-iterator/lib/dom";
import { promisify } from "util";
import * as zlib from "zlib";

import {
    CreateStreamToClientMessage, CreateStreamToServerMessage, GameInfo, JoinStreamToClientMessage,
    JoinStreamToServerMessage, Settings
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
  return stream;
}

interface OpponentInfo {
  id: number;
  nickname: string;
  gameInfo: GameInfo;
}

interface LobbyJoinHandle {
  propose(
    nickname: string,
    gameInfo: GameInfo,
    settings: Settings
  ): Promise<void>;
}

export async function join(
  addr: string,
  lobbyId: string,
  { signal }: { signal?: AbortSignal } = {}
): Promise<string | null> {
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
          lobbyId,
        },
        proposeReq: undefined,
      }).finish()
    );
  };
  const stream = wrapMessageStream(ws);

  {
    const { value: raw, done } = await stream.next();
    if (done) {
      throw "stream ended early";
    }
    const resp = JoinStreamToClientMessage.decode(new Uint8Array(raw));
    if (resp.joinResp == null) {
      throw `unexpected response: ${JoinStreamToClientMessage.toJSON(resp)}`;
    }

    if (
      resp.joinResp.status != JoinStreamToClientMessage_JoinResponse_Status.OK
    ) {
      return null;
    }
  }

  const { value: raw, done } = await stream.next();
  if (done) {
    return null;
  }

  const resp = JoinStreamToClientMessage.decode(new Uint8Array(raw));
  if (resp.acceptInd != null) {
    return resp.acceptInd.sessionId;
  } else if (resp.disconnectInd != null) {
    return null;
  } else {
    throw `unexpected response: ${JoinStreamToClientMessage.toJSON(resp)}`;
  }
}

interface LobbyCreateHandle {
  lobbyId: string;
  nextOpponent(): Promise<{
    info: OpponentInfo;
    saveData: Uint8Array;
  } | null>;
  accept(id: number): Promise<string>;
  reject(id: number): Promise<void>;
}

export async function create(
  addr: string,
  nickname: string,
  gameInfo: GameInfo,
  availableGames: GameInfo[],
  settings: Settings,
  saveData: Uint8Array,
  { signal }: { signal?: AbortSignal } = {}
): Promise<LobbyCreateHandle> {
  saveData = await promisify(zlib.brotliCompress)(saveData);

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
          saveData,
        },
        acceptReq: undefined,
        rejectReq: undefined,
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

    async nextOpponent() {
      const { value: raw, done } = await stream.next();
      if (done) {
        return null;
      }

      const resp = CreateStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.disconnectInd != null) {
        // TODO: Have a better message here.
        throw `disconnected: ${CreateStreamToClientMessage.toJSON(resp)}`;
      }

      if (resp.joinInd == null) {
        throw `unexpected response: ${CreateStreamToClientMessage.toJSON(
          resp
        )}`;
      }

      if (resp.joinInd.gameInfo == null) {
        throw "missing game info";
      }

      return {
        info: {
          id: resp.joinInd.opponentId,
          nickname: resp.joinInd.opponentNickname,
          gameInfo: resp.joinInd.gameInfo,
        },
        saveData: await promisify(zlib.brotliDecompress)(resp.joinInd.saveData),
      };
    },

    async accept(opponentId: number) {
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

      return resp.acceptResp.sessionId;
    },

    async reject(opponentId: number) {
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

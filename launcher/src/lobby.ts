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

interface NegotiatedSession {
  sessionId: string;
  saveData: Uint8Array;
}

interface LobbyJoinHandle {
  creatorNickname: string;
  gameInfo: GameInfo;
  availableGames: GameInfo[];
  settings: Settings;
  propose(
    nickname: string,
    gameInfo: GameInfo,
    saveData: Uint8Array
  ): Promise<NegotiatedSession | null>;
}

export async function join(
  addr: string,
  lobbyId: string,
  { signal }: { signal?: AbortSignal } = {}
): Promise<LobbyJoinHandle | null> {
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

  const { value: raw, done } = await stream.next();
  if (done) {
    throw "stream ended early";
  }
  const resp = JoinStreamToClientMessage.decode(new Uint8Array(raw));
  if (resp.joinResp == null) {
    throw `unexpected response: ${JoinStreamToClientMessage.toJSON(resp)}`;
  }

  if (resp.joinResp.info == null) {
    return null;
  }

  if (resp.joinResp.info.gameInfo == null) {
    throw "join response missing game_info";
  }

  if (resp.joinResp.info.settings == null) {
    throw "join response missing settings";
  }

  return {
    creatorNickname: resp.joinResp.info.opponentNickname,
    gameInfo: resp.joinResp.info.gameInfo,
    availableGames: resp.joinResp.info.availableGames,
    settings: resp.joinResp.info.settings,
    async propose(nickname: string, gameInfo: GameInfo, saveData: Uint8Array) {
      ws.send(
        JoinStreamToServerMessage.encode({
          joinReq: undefined,
          proposeReq: {
            nickname,
            gameInfo,
            saveData: await promisify(zlib.brotliCompress)(saveData),
          },
        }).finish()
      );

      const { value: raw, done } = await stream.next();
      if (done) {
        throw "stream ended early";
      }
      const resp = JoinStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.proposeResp == null) {
        throw `unexpected response: ${JoinStreamToClientMessage.toJSON(resp)}`;
      }

      if (resp.proposeResp.error != null) {
        return null;
      }

      if (resp.proposeResp.ok == null) {
        throw "propose response missing ook";
      }

      return {
        sessionId: resp.proposeResp.ok.sessionId,
        saveData: await promisify(zlib.brotliDecompress)(
          resp.proposeResp.ok.saveData
        ),
      };
    },
  };
}

interface LobbyCreateHandle {
  lobbyId: string;
  nextOpponent(): Promise<OpponentInfo | null>;
  accept(id: number, saveData: Uint8Array): Promise<NegotiatedSession | null>;
  reject(id: number): Promise<void>;
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

      if (resp.proposeInd == null) {
        throw `unexpected response: ${CreateStreamToClientMessage.toJSON(
          resp
        )}`;
      }

      if (resp.proposeInd.gameInfo == null) {
        throw "missing game info";
      }

      return {
        id: resp.proposeInd.opponentId,
        nickname: resp.proposeInd.opponentNickname,
        gameInfo: resp.proposeInd.gameInfo,
      };
    },

    async accept(opponentId: number, saveData: Uint8Array) {
      ws.send(
        CreateStreamToServerMessage.encode({
          createReq: undefined,
          acceptReq: {
            opponentId,
            saveData: await promisify(zlib.brotliCompress)(saveData),
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

      if (resp.acceptResp.error != null) {
        return null;
      }

      if (resp.acceptResp.ok == null) {
        throw "propose response missing ook";
      }

      return {
        sessionId: resp.acceptResp.ok.sessionId,
        saveData: await promisify(zlib.brotliDecompress)(
          resp.acceptResp.ok.saveData
        ),
      };
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

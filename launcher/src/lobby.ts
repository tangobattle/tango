import { subscribe } from "event-iterator/lib/dom";

import {
    CreateStreamToClientMessage, CreateStreamToServerMessage, GameInfo, JoinStreamToClientMessage,
    JoinStreamToServerMessage
} from "./protos/lobby";

export { GameInfo };

async function* wrapMessageStream(ws: WebSocket) {
  for await (const msg of subscribe.call(ws, "message")) {
    yield (msg as MessageEvent<ArrayBuffer>).data;
  }
}

interface OpponentInfo {
  opponentId: string;
  gameInfo: GameInfo;
  saveData: Uint8Array;
}

interface LobbyJoinHandle {
  getOpponentInfo(): OpponentInfo;
  wait(): Promise<string | null>;
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
    saveData: resp.joinResp.saveData,
  };

  return {
    getOpponentInfo() {
      return opponentInfo;
    },

    async wait() {
      const { value: raw, done } = await stream.next();
      if (done) {
        return null;
      }

      const resp = JoinStreamToClientMessage.decode(new Uint8Array(raw));
      if (resp.acceptInd == null) {
        throw `unexpected response: ${JoinStreamToClientMessage.toJSON(resp)}`;
      }

      return resp.acceptInd.sessionId;
    },
  };
}

interface LobbyCreateHandle {
  getLobbyId(): string;
  waitForJoin(): Promise<OpponentInfo | null>;
  accept(opponentId: string): Promise<string>;
  reject(opponentId: string): Promise<void>;
}

export async function create(
  addr: string,
  identityToken: string,
  gameInfo: GameInfo,
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
    getLobbyId() {
      return lobbyId;
    },

    async waitForJoin() {
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
        saveData: resp.joinInd.saveData,
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

      return resp.acceptResp.sessionId;
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

import { subscribe } from "event-iterator/lib/dom";
import fetch from "node-fetch";
import { promisify } from "util";
import * as zlib from "zlib";

import {
    CreateStreamToClientMessage, CreateStreamToServerMessage, GameInfo, GetInfoRequest,
    GetInfoResponse, GetSaveDataRequest, GetSaveDataResponse, JoinStreamToClientMessage,
    JoinStreamToServerMessage, Settings
} from "./protos/lobby";

export { GameInfo, Settings, GetInfoResponse };

async function* wrapMessageStream(ws: WebSocket) {
  for await (const msg of subscribe.call(ws, "message")) {
    yield (msg as MessageEvent<ArrayBuffer>).data;
  }
}

interface OpponentInfo {
  id: number;
  nickname: string;
  gameInfo: GameInfo;
}

interface NegotiatedSession {
  sessionId: string;
}

interface LobbyJoinHandle {
  opponentInfo: OpponentInfo;
  negotiatedSession: Promise<NegotiatedSession | null>;
}

export async function join(
  addr: string,
  nickname: string,
  lobbyId: string,
  gameInfo: GameInfo,
  saveData: Uint8Array,
  { signal }: { signal?: AbortSignal } = {}
): Promise<LobbyJoinHandle> {
  saveData = await promisify(zlib.brotliCompress)(saveData);

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
          nickname,
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
    id: resp.joinResp.opponentId,
    nickname: resp.joinResp.opponentNickname,
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
      };
    })(),
  };
}

interface LobbyCreateHandle {
  lobbyId: string;
  nextOpponent(): Promise<{
    info: OpponentInfo;
    saveData: Uint8Array;
  } | null>;
  accept(id: number): Promise<NegotiatedSession>;
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

      return {
        sessionId: resp.acceptResp.sessionId,
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

export async function getInfo(
  addr: string,
  lobbyId: string,
  { signal }: { signal?: AbortSignal } = {}
) {
  const httpResp = await fetch(`${addr}/get_info`, {
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

export async function getSaveData(
  addr: string,
  lobbyId: string,
  { signal }: { signal?: AbortSignal } = {}
) {
  const httpResp = await fetch(`${addr}/get_save_data`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-protobuf",
    },
    body: Buffer.from(GetSaveDataRequest.encode({ lobbyId }).finish()),
    signal,
  });

  if (httpResp.status != 200) {
    throw `unexpected status: ${httpResp.status}`;
  }

  return await promisify(zlib.brotliDecompress)(
    GetSaveDataResponse.decode(new Uint8Array(await httpResp.arrayBuffer()))
      .saveData
  );
}

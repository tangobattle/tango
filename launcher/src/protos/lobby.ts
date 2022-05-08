/* eslint-disable */
import Long from "long";
import * as _m0 from "protobufjs/minimal";

export const protobufPackage = "tango.lobby";

export interface GameInfo {
  rom: string;
  patch: GameInfo_Patch | undefined;
}

export interface GameInfo_Patch {
  name: string;
  version: string;
}

export interface Settings {
  open: boolean;
  matchType: Settings_MatchType;
}

export enum Settings_MatchType {
  UNKNOWN = 0,
  SINGLE = 1,
  TRIPLE = 2,
  UNRECOGNIZED = -1,
}

export function settings_MatchTypeFromJSON(object: any): Settings_MatchType {
  switch (object) {
    case 0:
    case "UNKNOWN":
      return Settings_MatchType.UNKNOWN;
    case 1:
    case "SINGLE":
      return Settings_MatchType.SINGLE;
    case 2:
    case "TRIPLE":
      return Settings_MatchType.TRIPLE;
    case -1:
    case "UNRECOGNIZED":
    default:
      return Settings_MatchType.UNRECOGNIZED;
  }
}

export function settings_MatchTypeToJSON(object: Settings_MatchType): string {
  switch (object) {
    case Settings_MatchType.UNKNOWN:
      return "UNKNOWN";
    case Settings_MatchType.SINGLE:
      return "SINGLE";
    case Settings_MatchType.TRIPLE:
      return "TRIPLE";
    default:
      return "UNKNOWN";
  }
}

export interface CreateStreamToServerMessage {
  createReq: CreateStreamToServerMessage_CreateRequest | undefined;
  acceptReq: CreateStreamToServerMessage_AcceptRequest | undefined;
  rejectReq: CreateStreamToServerMessage_RejectRequest | undefined;
}

export interface CreateStreamToServerMessage_CreateRequest {
  nickname: string;
  gameInfo: GameInfo | undefined;
  availableGames: GameInfo[];
  settings: Settings | undefined;
}

export interface CreateStreamToServerMessage_AcceptRequest {
  opponentId: number;
  saveData: Uint8Array;
}

export interface CreateStreamToServerMessage_RejectRequest {
  opponentId: number;
}

export interface CreateStreamToClientMessage {
  disconnectInd: CreateStreamToClientMessage_DisconnectIndication | undefined;
  createResp: CreateStreamToClientMessage_CreateResponse | undefined;
  proposeInd: CreateStreamToClientMessage_ProposeIndication | undefined;
  acceptResp: CreateStreamToClientMessage_AcceptResponse | undefined;
  rejectResp: CreateStreamToClientMessage_RejectResponse | undefined;
}

export interface CreateStreamToClientMessage_CreateResponse {
  lobbyId: string;
}

export interface CreateStreamToClientMessage_ProposeIndication {
  opponentId: number;
  opponentNickname: string;
  gameInfo: GameInfo | undefined;
}

export interface CreateStreamToClientMessage_AcceptResponse {
  ok: CreateStreamToClientMessage_AcceptResponse_Ok | undefined;
  error: CreateStreamToClientMessage_AcceptResponse_Error | undefined;
}

export interface CreateStreamToClientMessage_AcceptResponse_Error {
  reason: CreateStreamToClientMessage_AcceptResponse_Error_Reason;
}

export enum CreateStreamToClientMessage_AcceptResponse_Error_Reason {
  UNKNOWN = 0,
  NO_SUCH_OPPONENT = 1,
  UNRECOGNIZED = -1,
}

export function createStreamToClientMessage_AcceptResponse_Error_ReasonFromJSON(
  object: any
): CreateStreamToClientMessage_AcceptResponse_Error_Reason {
  switch (object) {
    case 0:
    case "UNKNOWN":
      return CreateStreamToClientMessage_AcceptResponse_Error_Reason.UNKNOWN;
    case 1:
    case "NO_SUCH_OPPONENT":
      return CreateStreamToClientMessage_AcceptResponse_Error_Reason.NO_SUCH_OPPONENT;
    case -1:
    case "UNRECOGNIZED":
    default:
      return CreateStreamToClientMessage_AcceptResponse_Error_Reason.UNRECOGNIZED;
  }
}

export function createStreamToClientMessage_AcceptResponse_Error_ReasonToJSON(
  object: CreateStreamToClientMessage_AcceptResponse_Error_Reason
): string {
  switch (object) {
    case CreateStreamToClientMessage_AcceptResponse_Error_Reason.UNKNOWN:
      return "UNKNOWN";
    case CreateStreamToClientMessage_AcceptResponse_Error_Reason.NO_SUCH_OPPONENT:
      return "NO_SUCH_OPPONENT";
    default:
      return "UNKNOWN";
  }
}

export interface CreateStreamToClientMessage_AcceptResponse_Ok {
  sessionId: string;
  opponentSaveData: Uint8Array;
}

export interface CreateStreamToClientMessage_RejectResponse {}

export interface CreateStreamToClientMessage_DisconnectIndication {
  reason: CreateStreamToClientMessage_DisconnectIndication_Reason;
}

export enum CreateStreamToClientMessage_DisconnectIndication_Reason {
  UNKNOWN = 0,
  START_TIMEOUT = 1,
  WAIT_TIMEOUT = 2,
  UNRECOGNIZED = -1,
}

export function createStreamToClientMessage_DisconnectIndication_ReasonFromJSON(
  object: any
): CreateStreamToClientMessage_DisconnectIndication_Reason {
  switch (object) {
    case 0:
    case "UNKNOWN":
      return CreateStreamToClientMessage_DisconnectIndication_Reason.UNKNOWN;
    case 1:
    case "START_TIMEOUT":
      return CreateStreamToClientMessage_DisconnectIndication_Reason.START_TIMEOUT;
    case 2:
    case "WAIT_TIMEOUT":
      return CreateStreamToClientMessage_DisconnectIndication_Reason.WAIT_TIMEOUT;
    case -1:
    case "UNRECOGNIZED":
    default:
      return CreateStreamToClientMessage_DisconnectIndication_Reason.UNRECOGNIZED;
  }
}

export function createStreamToClientMessage_DisconnectIndication_ReasonToJSON(
  object: CreateStreamToClientMessage_DisconnectIndication_Reason
): string {
  switch (object) {
    case CreateStreamToClientMessage_DisconnectIndication_Reason.UNKNOWN:
      return "UNKNOWN";
    case CreateStreamToClientMessage_DisconnectIndication_Reason.START_TIMEOUT:
      return "START_TIMEOUT";
    case CreateStreamToClientMessage_DisconnectIndication_Reason.WAIT_TIMEOUT:
      return "WAIT_TIMEOUT";
    default:
      return "UNKNOWN";
  }
}

export interface JoinStreamToServerMessage {
  joinReq: JoinStreamToServerMessage_JoinRequest | undefined;
  proposeReq: JoinStreamToServerMessage_ProposeRequest | undefined;
}

export interface JoinStreamToServerMessage_JoinRequest {
  lobbyId: string;
}

export interface JoinStreamToServerMessage_ProposeRequest {
  nickname: string;
  gameInfo: GameInfo | undefined;
  saveData: Uint8Array;
}

export interface JoinStreamToClientMessage {
  disconnectInd: JoinStreamToClientMessage_DisconnectIndication | undefined;
  joinResp: JoinStreamToClientMessage_JoinResponse | undefined;
  proposeResp: JoinStreamToClientMessage_ProposeResponse | undefined;
}

export interface JoinStreamToClientMessage_JoinResponse {
  info: JoinStreamToClientMessage_JoinResponse_Info | undefined;
}

export interface JoinStreamToClientMessage_JoinResponse_Info {
  opponentNickname: string;
  gameInfo: GameInfo | undefined;
  availableGames: GameInfo[];
  settings: Settings | undefined;
}

export interface JoinStreamToClientMessage_ProposeResponse {
  ok: JoinStreamToClientMessage_ProposeResponse_Ok | undefined;
  error: JoinStreamToClientMessage_ProposeResponse_Error | undefined;
}

export interface JoinStreamToClientMessage_ProposeResponse_Error {
  reason: JoinStreamToClientMessage_ProposeResponse_Error_Reason;
}

export enum JoinStreamToClientMessage_ProposeResponse_Error_Reason {
  UNKNOWN = 0,
  REJECTED = 1,
  UNRECOGNIZED = -1,
}

export function joinStreamToClientMessage_ProposeResponse_Error_ReasonFromJSON(
  object: any
): JoinStreamToClientMessage_ProposeResponse_Error_Reason {
  switch (object) {
    case 0:
    case "UNKNOWN":
      return JoinStreamToClientMessage_ProposeResponse_Error_Reason.UNKNOWN;
    case 1:
    case "REJECTED":
      return JoinStreamToClientMessage_ProposeResponse_Error_Reason.REJECTED;
    case -1:
    case "UNRECOGNIZED":
    default:
      return JoinStreamToClientMessage_ProposeResponse_Error_Reason.UNRECOGNIZED;
  }
}

export function joinStreamToClientMessage_ProposeResponse_Error_ReasonToJSON(
  object: JoinStreamToClientMessage_ProposeResponse_Error_Reason
): string {
  switch (object) {
    case JoinStreamToClientMessage_ProposeResponse_Error_Reason.UNKNOWN:
      return "UNKNOWN";
    case JoinStreamToClientMessage_ProposeResponse_Error_Reason.REJECTED:
      return "REJECTED";
    default:
      return "UNKNOWN";
  }
}

export interface JoinStreamToClientMessage_ProposeResponse_Ok {
  sessionId: string;
  opponentSaveData: Uint8Array;
}

export interface JoinStreamToClientMessage_DisconnectIndication {
  reason: JoinStreamToClientMessage_DisconnectIndication_Reason;
}

export enum JoinStreamToClientMessage_DisconnectIndication_Reason {
  UNKNOWN = 0,
  START_TIMEOUT = 1,
  PROPOSE_TIMEOUT = 2,
  LOBBY_CLOSED = 3,
  UNRECOGNIZED = -1,
}

export function joinStreamToClientMessage_DisconnectIndication_ReasonFromJSON(
  object: any
): JoinStreamToClientMessage_DisconnectIndication_Reason {
  switch (object) {
    case 0:
    case "UNKNOWN":
      return JoinStreamToClientMessage_DisconnectIndication_Reason.UNKNOWN;
    case 1:
    case "START_TIMEOUT":
      return JoinStreamToClientMessage_DisconnectIndication_Reason.START_TIMEOUT;
    case 2:
    case "PROPOSE_TIMEOUT":
      return JoinStreamToClientMessage_DisconnectIndication_Reason.PROPOSE_TIMEOUT;
    case 3:
    case "LOBBY_CLOSED":
      return JoinStreamToClientMessage_DisconnectIndication_Reason.LOBBY_CLOSED;
    case -1:
    case "UNRECOGNIZED":
    default:
      return JoinStreamToClientMessage_DisconnectIndication_Reason.UNRECOGNIZED;
  }
}

export function joinStreamToClientMessage_DisconnectIndication_ReasonToJSON(
  object: JoinStreamToClientMessage_DisconnectIndication_Reason
): string {
  switch (object) {
    case JoinStreamToClientMessage_DisconnectIndication_Reason.UNKNOWN:
      return "UNKNOWN";
    case JoinStreamToClientMessage_DisconnectIndication_Reason.START_TIMEOUT:
      return "START_TIMEOUT";
    case JoinStreamToClientMessage_DisconnectIndication_Reason.PROPOSE_TIMEOUT:
      return "PROPOSE_TIMEOUT";
    case JoinStreamToClientMessage_DisconnectIndication_Reason.LOBBY_CLOSED:
      return "LOBBY_CLOSED";
    default:
      return "UNKNOWN";
  }
}

function createBaseGameInfo(): GameInfo {
  return { rom: "", patch: undefined };
}

export const GameInfo = {
  encode(
    message: GameInfo,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.rom !== "") {
      writer.uint32(10).string(message.rom);
    }
    if (message.patch !== undefined) {
      GameInfo_Patch.encode(message.patch, writer.uint32(18).fork()).ldelim();
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GameInfo {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGameInfo();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.rom = reader.string();
          break;
        case 2:
          message.patch = GameInfo_Patch.decode(reader, reader.uint32());
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): GameInfo {
    return {
      rom: isSet(object.rom) ? String(object.rom) : "",
      patch: isSet(object.patch)
        ? GameInfo_Patch.fromJSON(object.patch)
        : undefined,
    };
  },

  toJSON(message: GameInfo): unknown {
    const obj: any = {};
    message.rom !== undefined && (obj.rom = message.rom);
    message.patch !== undefined &&
      (obj.patch = message.patch
        ? GameInfo_Patch.toJSON(message.patch)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GameInfo>, I>>(object: I): GameInfo {
    const message = createBaseGameInfo();
    message.rom = object.rom ?? "";
    message.patch =
      object.patch !== undefined && object.patch !== null
        ? GameInfo_Patch.fromPartial(object.patch)
        : undefined;
    return message;
  },
};

function createBaseGameInfo_Patch(): GameInfo_Patch {
  return { name: "", version: "" };
}

export const GameInfo_Patch = {
  encode(
    message: GameInfo_Patch,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.name !== "") {
      writer.uint32(10).string(message.name);
    }
    if (message.version !== "") {
      writer.uint32(18).string(message.version);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GameInfo_Patch {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGameInfo_Patch();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.name = reader.string();
          break;
        case 2:
          message.version = reader.string();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): GameInfo_Patch {
    return {
      name: isSet(object.name) ? String(object.name) : "",
      version: isSet(object.version) ? String(object.version) : "",
    };
  },

  toJSON(message: GameInfo_Patch): unknown {
    const obj: any = {};
    message.name !== undefined && (obj.name = message.name);
    message.version !== undefined && (obj.version = message.version);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GameInfo_Patch>, I>>(
    object: I
  ): GameInfo_Patch {
    const message = createBaseGameInfo_Patch();
    message.name = object.name ?? "";
    message.version = object.version ?? "";
    return message;
  },
};

function createBaseSettings(): Settings {
  return { open: false, matchType: 0 };
}

export const Settings = {
  encode(
    message: Settings,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.open === true) {
      writer.uint32(8).bool(message.open);
    }
    if (message.matchType !== 0) {
      writer.uint32(16).int32(message.matchType);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): Settings {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseSettings();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.open = reader.bool();
          break;
        case 2:
          message.matchType = reader.int32() as any;
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): Settings {
    return {
      open: isSet(object.open) ? Boolean(object.open) : false,
      matchType: isSet(object.matchType)
        ? settings_MatchTypeFromJSON(object.matchType)
        : 0,
    };
  },

  toJSON(message: Settings): unknown {
    const obj: any = {};
    message.open !== undefined && (obj.open = message.open);
    message.matchType !== undefined &&
      (obj.matchType = settings_MatchTypeToJSON(message.matchType));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<Settings>, I>>(object: I): Settings {
    const message = createBaseSettings();
    message.open = object.open ?? false;
    message.matchType = object.matchType ?? 0;
    return message;
  },
};

function createBaseCreateStreamToServerMessage(): CreateStreamToServerMessage {
  return { createReq: undefined, acceptReq: undefined, rejectReq: undefined };
}

export const CreateStreamToServerMessage = {
  encode(
    message: CreateStreamToServerMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.createReq !== undefined) {
      CreateStreamToServerMessage_CreateRequest.encode(
        message.createReq,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.acceptReq !== undefined) {
      CreateStreamToServerMessage_AcceptRequest.encode(
        message.acceptReq,
        writer.uint32(18).fork()
      ).ldelim();
    }
    if (message.rejectReq !== undefined) {
      CreateStreamToServerMessage_RejectRequest.encode(
        message.rejectReq,
        writer.uint32(26).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToServerMessage {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToServerMessage();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.createReq = CreateStreamToServerMessage_CreateRequest.decode(
            reader,
            reader.uint32()
          );
          break;
        case 2:
          message.acceptReq = CreateStreamToServerMessage_AcceptRequest.decode(
            reader,
            reader.uint32()
          );
          break;
        case 3:
          message.rejectReq = CreateStreamToServerMessage_RejectRequest.decode(
            reader,
            reader.uint32()
          );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToServerMessage {
    return {
      createReq: isSet(object.createReq)
        ? CreateStreamToServerMessage_CreateRequest.fromJSON(object.createReq)
        : undefined,
      acceptReq: isSet(object.acceptReq)
        ? CreateStreamToServerMessage_AcceptRequest.fromJSON(object.acceptReq)
        : undefined,
      rejectReq: isSet(object.rejectReq)
        ? CreateStreamToServerMessage_RejectRequest.fromJSON(object.rejectReq)
        : undefined,
    };
  },

  toJSON(message: CreateStreamToServerMessage): unknown {
    const obj: any = {};
    message.createReq !== undefined &&
      (obj.createReq = message.createReq
        ? CreateStreamToServerMessage_CreateRequest.toJSON(message.createReq)
        : undefined);
    message.acceptReq !== undefined &&
      (obj.acceptReq = message.acceptReq
        ? CreateStreamToServerMessage_AcceptRequest.toJSON(message.acceptReq)
        : undefined);
    message.rejectReq !== undefined &&
      (obj.rejectReq = message.rejectReq
        ? CreateStreamToServerMessage_RejectRequest.toJSON(message.rejectReq)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<CreateStreamToServerMessage>, I>>(
    object: I
  ): CreateStreamToServerMessage {
    const message = createBaseCreateStreamToServerMessage();
    message.createReq =
      object.createReq !== undefined && object.createReq !== null
        ? CreateStreamToServerMessage_CreateRequest.fromPartial(
            object.createReq
          )
        : undefined;
    message.acceptReq =
      object.acceptReq !== undefined && object.acceptReq !== null
        ? CreateStreamToServerMessage_AcceptRequest.fromPartial(
            object.acceptReq
          )
        : undefined;
    message.rejectReq =
      object.rejectReq !== undefined && object.rejectReq !== null
        ? CreateStreamToServerMessage_RejectRequest.fromPartial(
            object.rejectReq
          )
        : undefined;
    return message;
  },
};

function createBaseCreateStreamToServerMessage_CreateRequest(): CreateStreamToServerMessage_CreateRequest {
  return {
    nickname: "",
    gameInfo: undefined,
    availableGames: [],
    settings: undefined,
  };
}

export const CreateStreamToServerMessage_CreateRequest = {
  encode(
    message: CreateStreamToServerMessage_CreateRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.nickname !== "") {
      writer.uint32(10).string(message.nickname);
    }
    if (message.gameInfo !== undefined) {
      GameInfo.encode(message.gameInfo, writer.uint32(18).fork()).ldelim();
    }
    for (const v of message.availableGames) {
      GameInfo.encode(v!, writer.uint32(26).fork()).ldelim();
    }
    if (message.settings !== undefined) {
      Settings.encode(message.settings, writer.uint32(34).fork()).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToServerMessage_CreateRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToServerMessage_CreateRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.nickname = reader.string();
          break;
        case 2:
          message.gameInfo = GameInfo.decode(reader, reader.uint32());
          break;
        case 3:
          message.availableGames.push(GameInfo.decode(reader, reader.uint32()));
          break;
        case 4:
          message.settings = Settings.decode(reader, reader.uint32());
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToServerMessage_CreateRequest {
    return {
      nickname: isSet(object.nickname) ? String(object.nickname) : "",
      gameInfo: isSet(object.gameInfo)
        ? GameInfo.fromJSON(object.gameInfo)
        : undefined,
      availableGames: Array.isArray(object?.availableGames)
        ? object.availableGames.map((e: any) => GameInfo.fromJSON(e))
        : [],
      settings: isSet(object.settings)
        ? Settings.fromJSON(object.settings)
        : undefined,
    };
  },

  toJSON(message: CreateStreamToServerMessage_CreateRequest): unknown {
    const obj: any = {};
    message.nickname !== undefined && (obj.nickname = message.nickname);
    message.gameInfo !== undefined &&
      (obj.gameInfo = message.gameInfo
        ? GameInfo.toJSON(message.gameInfo)
        : undefined);
    if (message.availableGames) {
      obj.availableGames = message.availableGames.map((e) =>
        e ? GameInfo.toJSON(e) : undefined
      );
    } else {
      obj.availableGames = [];
    }
    message.settings !== undefined &&
      (obj.settings = message.settings
        ? Settings.toJSON(message.settings)
        : undefined);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<CreateStreamToServerMessage_CreateRequest>, I>
  >(object: I): CreateStreamToServerMessage_CreateRequest {
    const message = createBaseCreateStreamToServerMessage_CreateRequest();
    message.nickname = object.nickname ?? "";
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? GameInfo.fromPartial(object.gameInfo)
        : undefined;
    message.availableGames =
      object.availableGames?.map((e) => GameInfo.fromPartial(e)) || [];
    message.settings =
      object.settings !== undefined && object.settings !== null
        ? Settings.fromPartial(object.settings)
        : undefined;
    return message;
  },
};

function createBaseCreateStreamToServerMessage_AcceptRequest(): CreateStreamToServerMessage_AcceptRequest {
  return { opponentId: 0, saveData: new Uint8Array() };
}

export const CreateStreamToServerMessage_AcceptRequest = {
  encode(
    message: CreateStreamToServerMessage_AcceptRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.opponentId !== 0) {
      writer.uint32(8).uint32(message.opponentId);
    }
    if (message.saveData.length !== 0) {
      writer.uint32(18).bytes(message.saveData);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToServerMessage_AcceptRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToServerMessage_AcceptRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.opponentId = reader.uint32();
          break;
        case 2:
          message.saveData = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToServerMessage_AcceptRequest {
    return {
      opponentId: isSet(object.opponentId) ? Number(object.opponentId) : 0,
      saveData: isSet(object.saveData)
        ? bytesFromBase64(object.saveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: CreateStreamToServerMessage_AcceptRequest): unknown {
    const obj: any = {};
    message.opponentId !== undefined &&
      (obj.opponentId = Math.round(message.opponentId));
    message.saveData !== undefined &&
      (obj.saveData = base64FromBytes(
        message.saveData !== undefined ? message.saveData : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<CreateStreamToServerMessage_AcceptRequest>, I>
  >(object: I): CreateStreamToServerMessage_AcceptRequest {
    const message = createBaseCreateStreamToServerMessage_AcceptRequest();
    message.opponentId = object.opponentId ?? 0;
    message.saveData = object.saveData ?? new Uint8Array();
    return message;
  },
};

function createBaseCreateStreamToServerMessage_RejectRequest(): CreateStreamToServerMessage_RejectRequest {
  return { opponentId: 0 };
}

export const CreateStreamToServerMessage_RejectRequest = {
  encode(
    message: CreateStreamToServerMessage_RejectRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.opponentId !== 0) {
      writer.uint32(8).uint32(message.opponentId);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToServerMessage_RejectRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToServerMessage_RejectRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.opponentId = reader.uint32();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToServerMessage_RejectRequest {
    return {
      opponentId: isSet(object.opponentId) ? Number(object.opponentId) : 0,
    };
  },

  toJSON(message: CreateStreamToServerMessage_RejectRequest): unknown {
    const obj: any = {};
    message.opponentId !== undefined &&
      (obj.opponentId = Math.round(message.opponentId));
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<CreateStreamToServerMessage_RejectRequest>, I>
  >(object: I): CreateStreamToServerMessage_RejectRequest {
    const message = createBaseCreateStreamToServerMessage_RejectRequest();
    message.opponentId = object.opponentId ?? 0;
    return message;
  },
};

function createBaseCreateStreamToClientMessage(): CreateStreamToClientMessage {
  return {
    disconnectInd: undefined,
    createResp: undefined,
    proposeInd: undefined,
    acceptResp: undefined,
    rejectResp: undefined,
  };
}

export const CreateStreamToClientMessage = {
  encode(
    message: CreateStreamToClientMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.disconnectInd !== undefined) {
      CreateStreamToClientMessage_DisconnectIndication.encode(
        message.disconnectInd,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.createResp !== undefined) {
      CreateStreamToClientMessage_CreateResponse.encode(
        message.createResp,
        writer.uint32(18).fork()
      ).ldelim();
    }
    if (message.proposeInd !== undefined) {
      CreateStreamToClientMessage_ProposeIndication.encode(
        message.proposeInd,
        writer.uint32(26).fork()
      ).ldelim();
    }
    if (message.acceptResp !== undefined) {
      CreateStreamToClientMessage_AcceptResponse.encode(
        message.acceptResp,
        writer.uint32(34).fork()
      ).ldelim();
    }
    if (message.rejectResp !== undefined) {
      CreateStreamToClientMessage_RejectResponse.encode(
        message.rejectResp,
        writer.uint32(42).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.disconnectInd =
            CreateStreamToClientMessage_DisconnectIndication.decode(
              reader,
              reader.uint32()
            );
          break;
        case 2:
          message.createResp =
            CreateStreamToClientMessage_CreateResponse.decode(
              reader,
              reader.uint32()
            );
          break;
        case 3:
          message.proposeInd =
            CreateStreamToClientMessage_ProposeIndication.decode(
              reader,
              reader.uint32()
            );
          break;
        case 4:
          message.acceptResp =
            CreateStreamToClientMessage_AcceptResponse.decode(
              reader,
              reader.uint32()
            );
          break;
        case 5:
          message.rejectResp =
            CreateStreamToClientMessage_RejectResponse.decode(
              reader,
              reader.uint32()
            );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage {
    return {
      disconnectInd: isSet(object.disconnectInd)
        ? CreateStreamToClientMessage_DisconnectIndication.fromJSON(
            object.disconnectInd
          )
        : undefined,
      createResp: isSet(object.createResp)
        ? CreateStreamToClientMessage_CreateResponse.fromJSON(object.createResp)
        : undefined,
      proposeInd: isSet(object.proposeInd)
        ? CreateStreamToClientMessage_ProposeIndication.fromJSON(
            object.proposeInd
          )
        : undefined,
      acceptResp: isSet(object.acceptResp)
        ? CreateStreamToClientMessage_AcceptResponse.fromJSON(object.acceptResp)
        : undefined,
      rejectResp: isSet(object.rejectResp)
        ? CreateStreamToClientMessage_RejectResponse.fromJSON(object.rejectResp)
        : undefined,
    };
  },

  toJSON(message: CreateStreamToClientMessage): unknown {
    const obj: any = {};
    message.disconnectInd !== undefined &&
      (obj.disconnectInd = message.disconnectInd
        ? CreateStreamToClientMessage_DisconnectIndication.toJSON(
            message.disconnectInd
          )
        : undefined);
    message.createResp !== undefined &&
      (obj.createResp = message.createResp
        ? CreateStreamToClientMessage_CreateResponse.toJSON(message.createResp)
        : undefined);
    message.proposeInd !== undefined &&
      (obj.proposeInd = message.proposeInd
        ? CreateStreamToClientMessage_ProposeIndication.toJSON(
            message.proposeInd
          )
        : undefined);
    message.acceptResp !== undefined &&
      (obj.acceptResp = message.acceptResp
        ? CreateStreamToClientMessage_AcceptResponse.toJSON(message.acceptResp)
        : undefined);
    message.rejectResp !== undefined &&
      (obj.rejectResp = message.rejectResp
        ? CreateStreamToClientMessage_RejectResponse.toJSON(message.rejectResp)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<CreateStreamToClientMessage>, I>>(
    object: I
  ): CreateStreamToClientMessage {
    const message = createBaseCreateStreamToClientMessage();
    message.disconnectInd =
      object.disconnectInd !== undefined && object.disconnectInd !== null
        ? CreateStreamToClientMessage_DisconnectIndication.fromPartial(
            object.disconnectInd
          )
        : undefined;
    message.createResp =
      object.createResp !== undefined && object.createResp !== null
        ? CreateStreamToClientMessage_CreateResponse.fromPartial(
            object.createResp
          )
        : undefined;
    message.proposeInd =
      object.proposeInd !== undefined && object.proposeInd !== null
        ? CreateStreamToClientMessage_ProposeIndication.fromPartial(
            object.proposeInd
          )
        : undefined;
    message.acceptResp =
      object.acceptResp !== undefined && object.acceptResp !== null
        ? CreateStreamToClientMessage_AcceptResponse.fromPartial(
            object.acceptResp
          )
        : undefined;
    message.rejectResp =
      object.rejectResp !== undefined && object.rejectResp !== null
        ? CreateStreamToClientMessage_RejectResponse.fromPartial(
            object.rejectResp
          )
        : undefined;
    return message;
  },
};

function createBaseCreateStreamToClientMessage_CreateResponse(): CreateStreamToClientMessage_CreateResponse {
  return { lobbyId: "" };
}

export const CreateStreamToClientMessage_CreateResponse = {
  encode(
    message: CreateStreamToClientMessage_CreateResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.lobbyId !== "") {
      writer.uint32(10).string(message.lobbyId);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_CreateResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage_CreateResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.lobbyId = reader.string();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage_CreateResponse {
    return {
      lobbyId: isSet(object.lobbyId) ? String(object.lobbyId) : "",
    };
  },

  toJSON(message: CreateStreamToClientMessage_CreateResponse): unknown {
    const obj: any = {};
    message.lobbyId !== undefined && (obj.lobbyId = message.lobbyId);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<CreateStreamToClientMessage_CreateResponse>, I>
  >(object: I): CreateStreamToClientMessage_CreateResponse {
    const message = createBaseCreateStreamToClientMessage_CreateResponse();
    message.lobbyId = object.lobbyId ?? "";
    return message;
  },
};

function createBaseCreateStreamToClientMessage_ProposeIndication(): CreateStreamToClientMessage_ProposeIndication {
  return { opponentId: 0, opponentNickname: "", gameInfo: undefined };
}

export const CreateStreamToClientMessage_ProposeIndication = {
  encode(
    message: CreateStreamToClientMessage_ProposeIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.opponentId !== 0) {
      writer.uint32(8).uint32(message.opponentId);
    }
    if (message.opponentNickname !== "") {
      writer.uint32(18).string(message.opponentNickname);
    }
    if (message.gameInfo !== undefined) {
      GameInfo.encode(message.gameInfo, writer.uint32(26).fork()).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_ProposeIndication {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage_ProposeIndication();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.opponentId = reader.uint32();
          break;
        case 2:
          message.opponentNickname = reader.string();
          break;
        case 3:
          message.gameInfo = GameInfo.decode(reader, reader.uint32());
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage_ProposeIndication {
    return {
      opponentId: isSet(object.opponentId) ? Number(object.opponentId) : 0,
      opponentNickname: isSet(object.opponentNickname)
        ? String(object.opponentNickname)
        : "",
      gameInfo: isSet(object.gameInfo)
        ? GameInfo.fromJSON(object.gameInfo)
        : undefined,
    };
  },

  toJSON(message: CreateStreamToClientMessage_ProposeIndication): unknown {
    const obj: any = {};
    message.opponentId !== undefined &&
      (obj.opponentId = Math.round(message.opponentId));
    message.opponentNickname !== undefined &&
      (obj.opponentNickname = message.opponentNickname);
    message.gameInfo !== undefined &&
      (obj.gameInfo = message.gameInfo
        ? GameInfo.toJSON(message.gameInfo)
        : undefined);
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<CreateStreamToClientMessage_ProposeIndication>,
      I
    >
  >(object: I): CreateStreamToClientMessage_ProposeIndication {
    const message = createBaseCreateStreamToClientMessage_ProposeIndication();
    message.opponentId = object.opponentId ?? 0;
    message.opponentNickname = object.opponentNickname ?? "";
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? GameInfo.fromPartial(object.gameInfo)
        : undefined;
    return message;
  },
};

function createBaseCreateStreamToClientMessage_AcceptResponse(): CreateStreamToClientMessage_AcceptResponse {
  return { ok: undefined, error: undefined };
}

export const CreateStreamToClientMessage_AcceptResponse = {
  encode(
    message: CreateStreamToClientMessage_AcceptResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.ok !== undefined) {
      CreateStreamToClientMessage_AcceptResponse_Ok.encode(
        message.ok,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.error !== undefined) {
      CreateStreamToClientMessage_AcceptResponse_Error.encode(
        message.error,
        writer.uint32(18).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_AcceptResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage_AcceptResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.ok = CreateStreamToClientMessage_AcceptResponse_Ok.decode(
            reader,
            reader.uint32()
          );
          break;
        case 2:
          message.error =
            CreateStreamToClientMessage_AcceptResponse_Error.decode(
              reader,
              reader.uint32()
            );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage_AcceptResponse {
    return {
      ok: isSet(object.ok)
        ? CreateStreamToClientMessage_AcceptResponse_Ok.fromJSON(object.ok)
        : undefined,
      error: isSet(object.error)
        ? CreateStreamToClientMessage_AcceptResponse_Error.fromJSON(
            object.error
          )
        : undefined,
    };
  },

  toJSON(message: CreateStreamToClientMessage_AcceptResponse): unknown {
    const obj: any = {};
    message.ok !== undefined &&
      (obj.ok = message.ok
        ? CreateStreamToClientMessage_AcceptResponse_Ok.toJSON(message.ok)
        : undefined);
    message.error !== undefined &&
      (obj.error = message.error
        ? CreateStreamToClientMessage_AcceptResponse_Error.toJSON(message.error)
        : undefined);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<CreateStreamToClientMessage_AcceptResponse>, I>
  >(object: I): CreateStreamToClientMessage_AcceptResponse {
    const message = createBaseCreateStreamToClientMessage_AcceptResponse();
    message.ok =
      object.ok !== undefined && object.ok !== null
        ? CreateStreamToClientMessage_AcceptResponse_Ok.fromPartial(object.ok)
        : undefined;
    message.error =
      object.error !== undefined && object.error !== null
        ? CreateStreamToClientMessage_AcceptResponse_Error.fromPartial(
            object.error
          )
        : undefined;
    return message;
  },
};

function createBaseCreateStreamToClientMessage_AcceptResponse_Error(): CreateStreamToClientMessage_AcceptResponse_Error {
  return { reason: 0 };
}

export const CreateStreamToClientMessage_AcceptResponse_Error = {
  encode(
    message: CreateStreamToClientMessage_AcceptResponse_Error,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.reason !== 0) {
      writer.uint32(8).int32(message.reason);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_AcceptResponse_Error {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message =
      createBaseCreateStreamToClientMessage_AcceptResponse_Error();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.reason = reader.int32() as any;
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage_AcceptResponse_Error {
    return {
      reason: isSet(object.reason)
        ? createStreamToClientMessage_AcceptResponse_Error_ReasonFromJSON(
            object.reason
          )
        : 0,
    };
  },

  toJSON(message: CreateStreamToClientMessage_AcceptResponse_Error): unknown {
    const obj: any = {};
    message.reason !== undefined &&
      (obj.reason =
        createStreamToClientMessage_AcceptResponse_Error_ReasonToJSON(
          message.reason
        ));
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<CreateStreamToClientMessage_AcceptResponse_Error>,
      I
    >
  >(object: I): CreateStreamToClientMessage_AcceptResponse_Error {
    const message =
      createBaseCreateStreamToClientMessage_AcceptResponse_Error();
    message.reason = object.reason ?? 0;
    return message;
  },
};

function createBaseCreateStreamToClientMessage_AcceptResponse_Ok(): CreateStreamToClientMessage_AcceptResponse_Ok {
  return { sessionId: "", opponentSaveData: new Uint8Array() };
}

export const CreateStreamToClientMessage_AcceptResponse_Ok = {
  encode(
    message: CreateStreamToClientMessage_AcceptResponse_Ok,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.sessionId !== "") {
      writer.uint32(10).string(message.sessionId);
    }
    if (message.opponentSaveData.length !== 0) {
      writer.uint32(18).bytes(message.opponentSaveData);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_AcceptResponse_Ok {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage_AcceptResponse_Ok();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.sessionId = reader.string();
          break;
        case 2:
          message.opponentSaveData = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage_AcceptResponse_Ok {
    return {
      sessionId: isSet(object.sessionId) ? String(object.sessionId) : "",
      opponentSaveData: isSet(object.opponentSaveData)
        ? bytesFromBase64(object.opponentSaveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: CreateStreamToClientMessage_AcceptResponse_Ok): unknown {
    const obj: any = {};
    message.sessionId !== undefined && (obj.sessionId = message.sessionId);
    message.opponentSaveData !== undefined &&
      (obj.opponentSaveData = base64FromBytes(
        message.opponentSaveData !== undefined
          ? message.opponentSaveData
          : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<CreateStreamToClientMessage_AcceptResponse_Ok>,
      I
    >
  >(object: I): CreateStreamToClientMessage_AcceptResponse_Ok {
    const message = createBaseCreateStreamToClientMessage_AcceptResponse_Ok();
    message.sessionId = object.sessionId ?? "";
    message.opponentSaveData = object.opponentSaveData ?? new Uint8Array();
    return message;
  },
};

function createBaseCreateStreamToClientMessage_RejectResponse(): CreateStreamToClientMessage_RejectResponse {
  return {};
}

export const CreateStreamToClientMessage_RejectResponse = {
  encode(
    _: CreateStreamToClientMessage_RejectResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_RejectResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage_RejectResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(_: any): CreateStreamToClientMessage_RejectResponse {
    return {};
  },

  toJSON(_: CreateStreamToClientMessage_RejectResponse): unknown {
    const obj: any = {};
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<CreateStreamToClientMessage_RejectResponse>, I>
  >(_: I): CreateStreamToClientMessage_RejectResponse {
    const message = createBaseCreateStreamToClientMessage_RejectResponse();
    return message;
  },
};

function createBaseCreateStreamToClientMessage_DisconnectIndication(): CreateStreamToClientMessage_DisconnectIndication {
  return { reason: 0 };
}

export const CreateStreamToClientMessage_DisconnectIndication = {
  encode(
    message: CreateStreamToClientMessage_DisconnectIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.reason !== 0) {
      writer.uint32(8).int32(message.reason);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_DisconnectIndication {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message =
      createBaseCreateStreamToClientMessage_DisconnectIndication();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.reason = reader.int32() as any;
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage_DisconnectIndication {
    return {
      reason: isSet(object.reason)
        ? createStreamToClientMessage_DisconnectIndication_ReasonFromJSON(
            object.reason
          )
        : 0,
    };
  },

  toJSON(message: CreateStreamToClientMessage_DisconnectIndication): unknown {
    const obj: any = {};
    message.reason !== undefined &&
      (obj.reason =
        createStreamToClientMessage_DisconnectIndication_ReasonToJSON(
          message.reason
        ));
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<CreateStreamToClientMessage_DisconnectIndication>,
      I
    >
  >(object: I): CreateStreamToClientMessage_DisconnectIndication {
    const message =
      createBaseCreateStreamToClientMessage_DisconnectIndication();
    message.reason = object.reason ?? 0;
    return message;
  },
};

function createBaseJoinStreamToServerMessage(): JoinStreamToServerMessage {
  return { joinReq: undefined, proposeReq: undefined };
}

export const JoinStreamToServerMessage = {
  encode(
    message: JoinStreamToServerMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.joinReq !== undefined) {
      JoinStreamToServerMessage_JoinRequest.encode(
        message.joinReq,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.proposeReq !== undefined) {
      JoinStreamToServerMessage_ProposeRequest.encode(
        message.proposeReq,
        writer.uint32(18).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToServerMessage {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToServerMessage();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.joinReq = JoinStreamToServerMessage_JoinRequest.decode(
            reader,
            reader.uint32()
          );
          break;
        case 2:
          message.proposeReq = JoinStreamToServerMessage_ProposeRequest.decode(
            reader,
            reader.uint32()
          );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToServerMessage {
    return {
      joinReq: isSet(object.joinReq)
        ? JoinStreamToServerMessage_JoinRequest.fromJSON(object.joinReq)
        : undefined,
      proposeReq: isSet(object.proposeReq)
        ? JoinStreamToServerMessage_ProposeRequest.fromJSON(object.proposeReq)
        : undefined,
    };
  },

  toJSON(message: JoinStreamToServerMessage): unknown {
    const obj: any = {};
    message.joinReq !== undefined &&
      (obj.joinReq = message.joinReq
        ? JoinStreamToServerMessage_JoinRequest.toJSON(message.joinReq)
        : undefined);
    message.proposeReq !== undefined &&
      (obj.proposeReq = message.proposeReq
        ? JoinStreamToServerMessage_ProposeRequest.toJSON(message.proposeReq)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<JoinStreamToServerMessage>, I>>(
    object: I
  ): JoinStreamToServerMessage {
    const message = createBaseJoinStreamToServerMessage();
    message.joinReq =
      object.joinReq !== undefined && object.joinReq !== null
        ? JoinStreamToServerMessage_JoinRequest.fromPartial(object.joinReq)
        : undefined;
    message.proposeReq =
      object.proposeReq !== undefined && object.proposeReq !== null
        ? JoinStreamToServerMessage_ProposeRequest.fromPartial(
            object.proposeReq
          )
        : undefined;
    return message;
  },
};

function createBaseJoinStreamToServerMessage_JoinRequest(): JoinStreamToServerMessage_JoinRequest {
  return { lobbyId: "" };
}

export const JoinStreamToServerMessage_JoinRequest = {
  encode(
    message: JoinStreamToServerMessage_JoinRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.lobbyId !== "") {
      writer.uint32(10).string(message.lobbyId);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToServerMessage_JoinRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToServerMessage_JoinRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.lobbyId = reader.string();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToServerMessage_JoinRequest {
    return {
      lobbyId: isSet(object.lobbyId) ? String(object.lobbyId) : "",
    };
  },

  toJSON(message: JoinStreamToServerMessage_JoinRequest): unknown {
    const obj: any = {};
    message.lobbyId !== undefined && (obj.lobbyId = message.lobbyId);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<JoinStreamToServerMessage_JoinRequest>, I>
  >(object: I): JoinStreamToServerMessage_JoinRequest {
    const message = createBaseJoinStreamToServerMessage_JoinRequest();
    message.lobbyId = object.lobbyId ?? "";
    return message;
  },
};

function createBaseJoinStreamToServerMessage_ProposeRequest(): JoinStreamToServerMessage_ProposeRequest {
  return { nickname: "", gameInfo: undefined, saveData: new Uint8Array() };
}

export const JoinStreamToServerMessage_ProposeRequest = {
  encode(
    message: JoinStreamToServerMessage_ProposeRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.nickname !== "") {
      writer.uint32(10).string(message.nickname);
    }
    if (message.gameInfo !== undefined) {
      GameInfo.encode(message.gameInfo, writer.uint32(18).fork()).ldelim();
    }
    if (message.saveData.length !== 0) {
      writer.uint32(26).bytes(message.saveData);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToServerMessage_ProposeRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToServerMessage_ProposeRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.nickname = reader.string();
          break;
        case 2:
          message.gameInfo = GameInfo.decode(reader, reader.uint32());
          break;
        case 3:
          message.saveData = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToServerMessage_ProposeRequest {
    return {
      nickname: isSet(object.nickname) ? String(object.nickname) : "",
      gameInfo: isSet(object.gameInfo)
        ? GameInfo.fromJSON(object.gameInfo)
        : undefined,
      saveData: isSet(object.saveData)
        ? bytesFromBase64(object.saveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: JoinStreamToServerMessage_ProposeRequest): unknown {
    const obj: any = {};
    message.nickname !== undefined && (obj.nickname = message.nickname);
    message.gameInfo !== undefined &&
      (obj.gameInfo = message.gameInfo
        ? GameInfo.toJSON(message.gameInfo)
        : undefined);
    message.saveData !== undefined &&
      (obj.saveData = base64FromBytes(
        message.saveData !== undefined ? message.saveData : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<JoinStreamToServerMessage_ProposeRequest>, I>
  >(object: I): JoinStreamToServerMessage_ProposeRequest {
    const message = createBaseJoinStreamToServerMessage_ProposeRequest();
    message.nickname = object.nickname ?? "";
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? GameInfo.fromPartial(object.gameInfo)
        : undefined;
    message.saveData = object.saveData ?? new Uint8Array();
    return message;
  },
};

function createBaseJoinStreamToClientMessage(): JoinStreamToClientMessage {
  return {
    disconnectInd: undefined,
    joinResp: undefined,
    proposeResp: undefined,
  };
}

export const JoinStreamToClientMessage = {
  encode(
    message: JoinStreamToClientMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.disconnectInd !== undefined) {
      JoinStreamToClientMessage_DisconnectIndication.encode(
        message.disconnectInd,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.joinResp !== undefined) {
      JoinStreamToClientMessage_JoinResponse.encode(
        message.joinResp,
        writer.uint32(18).fork()
      ).ldelim();
    }
    if (message.proposeResp !== undefined) {
      JoinStreamToClientMessage_ProposeResponse.encode(
        message.proposeResp,
        writer.uint32(26).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToClientMessage {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToClientMessage();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.disconnectInd =
            JoinStreamToClientMessage_DisconnectIndication.decode(
              reader,
              reader.uint32()
            );
          break;
        case 2:
          message.joinResp = JoinStreamToClientMessage_JoinResponse.decode(
            reader,
            reader.uint32()
          );
          break;
        case 3:
          message.proposeResp =
            JoinStreamToClientMessage_ProposeResponse.decode(
              reader,
              reader.uint32()
            );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToClientMessage {
    return {
      disconnectInd: isSet(object.disconnectInd)
        ? JoinStreamToClientMessage_DisconnectIndication.fromJSON(
            object.disconnectInd
          )
        : undefined,
      joinResp: isSet(object.joinResp)
        ? JoinStreamToClientMessage_JoinResponse.fromJSON(object.joinResp)
        : undefined,
      proposeResp: isSet(object.proposeResp)
        ? JoinStreamToClientMessage_ProposeResponse.fromJSON(object.proposeResp)
        : undefined,
    };
  },

  toJSON(message: JoinStreamToClientMessage): unknown {
    const obj: any = {};
    message.disconnectInd !== undefined &&
      (obj.disconnectInd = message.disconnectInd
        ? JoinStreamToClientMessage_DisconnectIndication.toJSON(
            message.disconnectInd
          )
        : undefined);
    message.joinResp !== undefined &&
      (obj.joinResp = message.joinResp
        ? JoinStreamToClientMessage_JoinResponse.toJSON(message.joinResp)
        : undefined);
    message.proposeResp !== undefined &&
      (obj.proposeResp = message.proposeResp
        ? JoinStreamToClientMessage_ProposeResponse.toJSON(message.proposeResp)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<JoinStreamToClientMessage>, I>>(
    object: I
  ): JoinStreamToClientMessage {
    const message = createBaseJoinStreamToClientMessage();
    message.disconnectInd =
      object.disconnectInd !== undefined && object.disconnectInd !== null
        ? JoinStreamToClientMessage_DisconnectIndication.fromPartial(
            object.disconnectInd
          )
        : undefined;
    message.joinResp =
      object.joinResp !== undefined && object.joinResp !== null
        ? JoinStreamToClientMessage_JoinResponse.fromPartial(object.joinResp)
        : undefined;
    message.proposeResp =
      object.proposeResp !== undefined && object.proposeResp !== null
        ? JoinStreamToClientMessage_ProposeResponse.fromPartial(
            object.proposeResp
          )
        : undefined;
    return message;
  },
};

function createBaseJoinStreamToClientMessage_JoinResponse(): JoinStreamToClientMessage_JoinResponse {
  return { info: undefined };
}

export const JoinStreamToClientMessage_JoinResponse = {
  encode(
    message: JoinStreamToClientMessage_JoinResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.info !== undefined) {
      JoinStreamToClientMessage_JoinResponse_Info.encode(
        message.info,
        writer.uint32(10).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToClientMessage_JoinResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToClientMessage_JoinResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.info = JoinStreamToClientMessage_JoinResponse_Info.decode(
            reader,
            reader.uint32()
          );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToClientMessage_JoinResponse {
    return {
      info: isSet(object.info)
        ? JoinStreamToClientMessage_JoinResponse_Info.fromJSON(object.info)
        : undefined,
    };
  },

  toJSON(message: JoinStreamToClientMessage_JoinResponse): unknown {
    const obj: any = {};
    message.info !== undefined &&
      (obj.info = message.info
        ? JoinStreamToClientMessage_JoinResponse_Info.toJSON(message.info)
        : undefined);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<JoinStreamToClientMessage_JoinResponse>, I>
  >(object: I): JoinStreamToClientMessage_JoinResponse {
    const message = createBaseJoinStreamToClientMessage_JoinResponse();
    message.info =
      object.info !== undefined && object.info !== null
        ? JoinStreamToClientMessage_JoinResponse_Info.fromPartial(object.info)
        : undefined;
    return message;
  },
};

function createBaseJoinStreamToClientMessage_JoinResponse_Info(): JoinStreamToClientMessage_JoinResponse_Info {
  return {
    opponentNickname: "",
    gameInfo: undefined,
    availableGames: [],
    settings: undefined,
  };
}

export const JoinStreamToClientMessage_JoinResponse_Info = {
  encode(
    message: JoinStreamToClientMessage_JoinResponse_Info,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.opponentNickname !== "") {
      writer.uint32(10).string(message.opponentNickname);
    }
    if (message.gameInfo !== undefined) {
      GameInfo.encode(message.gameInfo, writer.uint32(18).fork()).ldelim();
    }
    for (const v of message.availableGames) {
      GameInfo.encode(v!, writer.uint32(26).fork()).ldelim();
    }
    if (message.settings !== undefined) {
      Settings.encode(message.settings, writer.uint32(34).fork()).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToClientMessage_JoinResponse_Info {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToClientMessage_JoinResponse_Info();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.opponentNickname = reader.string();
          break;
        case 2:
          message.gameInfo = GameInfo.decode(reader, reader.uint32());
          break;
        case 3:
          message.availableGames.push(GameInfo.decode(reader, reader.uint32()));
          break;
        case 4:
          message.settings = Settings.decode(reader, reader.uint32());
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToClientMessage_JoinResponse_Info {
    return {
      opponentNickname: isSet(object.opponentNickname)
        ? String(object.opponentNickname)
        : "",
      gameInfo: isSet(object.gameInfo)
        ? GameInfo.fromJSON(object.gameInfo)
        : undefined,
      availableGames: Array.isArray(object?.availableGames)
        ? object.availableGames.map((e: any) => GameInfo.fromJSON(e))
        : [],
      settings: isSet(object.settings)
        ? Settings.fromJSON(object.settings)
        : undefined,
    };
  },

  toJSON(message: JoinStreamToClientMessage_JoinResponse_Info): unknown {
    const obj: any = {};
    message.opponentNickname !== undefined &&
      (obj.opponentNickname = message.opponentNickname);
    message.gameInfo !== undefined &&
      (obj.gameInfo = message.gameInfo
        ? GameInfo.toJSON(message.gameInfo)
        : undefined);
    if (message.availableGames) {
      obj.availableGames = message.availableGames.map((e) =>
        e ? GameInfo.toJSON(e) : undefined
      );
    } else {
      obj.availableGames = [];
    }
    message.settings !== undefined &&
      (obj.settings = message.settings
        ? Settings.toJSON(message.settings)
        : undefined);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<JoinStreamToClientMessage_JoinResponse_Info>, I>
  >(object: I): JoinStreamToClientMessage_JoinResponse_Info {
    const message = createBaseJoinStreamToClientMessage_JoinResponse_Info();
    message.opponentNickname = object.opponentNickname ?? "";
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? GameInfo.fromPartial(object.gameInfo)
        : undefined;
    message.availableGames =
      object.availableGames?.map((e) => GameInfo.fromPartial(e)) || [];
    message.settings =
      object.settings !== undefined && object.settings !== null
        ? Settings.fromPartial(object.settings)
        : undefined;
    return message;
  },
};

function createBaseJoinStreamToClientMessage_ProposeResponse(): JoinStreamToClientMessage_ProposeResponse {
  return { ok: undefined, error: undefined };
}

export const JoinStreamToClientMessage_ProposeResponse = {
  encode(
    message: JoinStreamToClientMessage_ProposeResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.ok !== undefined) {
      JoinStreamToClientMessage_ProposeResponse_Ok.encode(
        message.ok,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.error !== undefined) {
      JoinStreamToClientMessage_ProposeResponse_Error.encode(
        message.error,
        writer.uint32(18).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToClientMessage_ProposeResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToClientMessage_ProposeResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.ok = JoinStreamToClientMessage_ProposeResponse_Ok.decode(
            reader,
            reader.uint32()
          );
          break;
        case 2:
          message.error =
            JoinStreamToClientMessage_ProposeResponse_Error.decode(
              reader,
              reader.uint32()
            );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToClientMessage_ProposeResponse {
    return {
      ok: isSet(object.ok)
        ? JoinStreamToClientMessage_ProposeResponse_Ok.fromJSON(object.ok)
        : undefined,
      error: isSet(object.error)
        ? JoinStreamToClientMessage_ProposeResponse_Error.fromJSON(object.error)
        : undefined,
    };
  },

  toJSON(message: JoinStreamToClientMessage_ProposeResponse): unknown {
    const obj: any = {};
    message.ok !== undefined &&
      (obj.ok = message.ok
        ? JoinStreamToClientMessage_ProposeResponse_Ok.toJSON(message.ok)
        : undefined);
    message.error !== undefined &&
      (obj.error = message.error
        ? JoinStreamToClientMessage_ProposeResponse_Error.toJSON(message.error)
        : undefined);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<JoinStreamToClientMessage_ProposeResponse>, I>
  >(object: I): JoinStreamToClientMessage_ProposeResponse {
    const message = createBaseJoinStreamToClientMessage_ProposeResponse();
    message.ok =
      object.ok !== undefined && object.ok !== null
        ? JoinStreamToClientMessage_ProposeResponse_Ok.fromPartial(object.ok)
        : undefined;
    message.error =
      object.error !== undefined && object.error !== null
        ? JoinStreamToClientMessage_ProposeResponse_Error.fromPartial(
            object.error
          )
        : undefined;
    return message;
  },
};

function createBaseJoinStreamToClientMessage_ProposeResponse_Error(): JoinStreamToClientMessage_ProposeResponse_Error {
  return { reason: 0 };
}

export const JoinStreamToClientMessage_ProposeResponse_Error = {
  encode(
    message: JoinStreamToClientMessage_ProposeResponse_Error,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.reason !== 0) {
      writer.uint32(8).int32(message.reason);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToClientMessage_ProposeResponse_Error {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToClientMessage_ProposeResponse_Error();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.reason = reader.int32() as any;
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToClientMessage_ProposeResponse_Error {
    return {
      reason: isSet(object.reason)
        ? joinStreamToClientMessage_ProposeResponse_Error_ReasonFromJSON(
            object.reason
          )
        : 0,
    };
  },

  toJSON(message: JoinStreamToClientMessage_ProposeResponse_Error): unknown {
    const obj: any = {};
    message.reason !== undefined &&
      (obj.reason =
        joinStreamToClientMessage_ProposeResponse_Error_ReasonToJSON(
          message.reason
        ));
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<JoinStreamToClientMessage_ProposeResponse_Error>,
      I
    >
  >(object: I): JoinStreamToClientMessage_ProposeResponse_Error {
    const message = createBaseJoinStreamToClientMessage_ProposeResponse_Error();
    message.reason = object.reason ?? 0;
    return message;
  },
};

function createBaseJoinStreamToClientMessage_ProposeResponse_Ok(): JoinStreamToClientMessage_ProposeResponse_Ok {
  return { sessionId: "", opponentSaveData: new Uint8Array() };
}

export const JoinStreamToClientMessage_ProposeResponse_Ok = {
  encode(
    message: JoinStreamToClientMessage_ProposeResponse_Ok,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.sessionId !== "") {
      writer.uint32(10).string(message.sessionId);
    }
    if (message.opponentSaveData.length !== 0) {
      writer.uint32(18).bytes(message.opponentSaveData);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToClientMessage_ProposeResponse_Ok {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToClientMessage_ProposeResponse_Ok();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.sessionId = reader.string();
          break;
        case 2:
          message.opponentSaveData = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToClientMessage_ProposeResponse_Ok {
    return {
      sessionId: isSet(object.sessionId) ? String(object.sessionId) : "",
      opponentSaveData: isSet(object.opponentSaveData)
        ? bytesFromBase64(object.opponentSaveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: JoinStreamToClientMessage_ProposeResponse_Ok): unknown {
    const obj: any = {};
    message.sessionId !== undefined && (obj.sessionId = message.sessionId);
    message.opponentSaveData !== undefined &&
      (obj.opponentSaveData = base64FromBytes(
        message.opponentSaveData !== undefined
          ? message.opponentSaveData
          : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<JoinStreamToClientMessage_ProposeResponse_Ok>,
      I
    >
  >(object: I): JoinStreamToClientMessage_ProposeResponse_Ok {
    const message = createBaseJoinStreamToClientMessage_ProposeResponse_Ok();
    message.sessionId = object.sessionId ?? "";
    message.opponentSaveData = object.opponentSaveData ?? new Uint8Array();
    return message;
  },
};

function createBaseJoinStreamToClientMessage_DisconnectIndication(): JoinStreamToClientMessage_DisconnectIndication {
  return { reason: 0 };
}

export const JoinStreamToClientMessage_DisconnectIndication = {
  encode(
    message: JoinStreamToClientMessage_DisconnectIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.reason !== 0) {
      writer.uint32(8).int32(message.reason);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): JoinStreamToClientMessage_DisconnectIndication {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinStreamToClientMessage_DisconnectIndication();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.reason = reader.int32() as any;
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinStreamToClientMessage_DisconnectIndication {
    return {
      reason: isSet(object.reason)
        ? joinStreamToClientMessage_DisconnectIndication_ReasonFromJSON(
            object.reason
          )
        : 0,
    };
  },

  toJSON(message: JoinStreamToClientMessage_DisconnectIndication): unknown {
    const obj: any = {};
    message.reason !== undefined &&
      (obj.reason = joinStreamToClientMessage_DisconnectIndication_ReasonToJSON(
        message.reason
      ));
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<JoinStreamToClientMessage_DisconnectIndication>,
      I
    >
  >(object: I): JoinStreamToClientMessage_DisconnectIndication {
    const message = createBaseJoinStreamToClientMessage_DisconnectIndication();
    message.reason = object.reason ?? 0;
    return message;
  },
};

declare var self: any | undefined;
declare var window: any | undefined;
declare var global: any | undefined;
var globalThis: any = (() => {
  if (typeof globalThis !== "undefined") return globalThis;
  if (typeof self !== "undefined") return self;
  if (typeof window !== "undefined") return window;
  if (typeof global !== "undefined") return global;
  throw "Unable to locate global object";
})();

const atob: (b64: string) => string =
  globalThis.atob ||
  ((b64) => globalThis.Buffer.from(b64, "base64").toString("binary"));
function bytesFromBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const arr = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; ++i) {
    arr[i] = bin.charCodeAt(i);
  }
  return arr;
}

const btoa: (bin: string) => string =
  globalThis.btoa ||
  ((bin) => globalThis.Buffer.from(bin, "binary").toString("base64"));
function base64FromBytes(arr: Uint8Array): string {
  const bin: string[] = [];
  arr.forEach((byte) => {
    bin.push(String.fromCharCode(byte));
  });
  return btoa(bin.join(""));
}

type Builtin =
  | Date
  | Function
  | Uint8Array
  | string
  | number
  | boolean
  | undefined;

export type DeepPartial<T> = T extends Builtin
  ? T
  : T extends Array<infer U>
  ? Array<DeepPartial<U>>
  : T extends ReadonlyArray<infer U>
  ? ReadonlyArray<DeepPartial<U>>
  : T extends {}
  ? { [K in keyof T]?: DeepPartial<T[K]> }
  : Partial<T>;

type KeysOfUnion<T> = T extends T ? keyof T : never;
export type Exact<P, I extends P> = P extends Builtin
  ? P
  : P & { [K in keyof P]: Exact<P[K], I[K]> } & Record<
        Exclude<keyof I, KeysOfUnion<P>>,
        never
      >;

if (_m0.util.Long !== Long) {
  _m0.util.Long = Long as any;
  _m0.configure();
}

function isSet(value: any): boolean {
  return value !== null && value !== undefined;
}

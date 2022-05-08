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
}

export interface CreateStreamToServerMessage_CreateRequest {
  nickname: string;
  gameInfo: GameInfo | undefined;
  availableGames: GameInfo[];
  settings: Settings | undefined;
}

export interface CreateStreamToServerMessage_AcceptRequest {
  saveData: Uint8Array;
}

export interface CreateStreamToClientMessage {
  timeoutInd: CreateStreamToClientMessage_TimeoutIndication | undefined;
  createResp: CreateStreamToClientMessage_CreateResponse | undefined;
  proposeInd: CreateStreamToClientMessage_ProposeIndication | undefined;
  acceptResp: CreateStreamToClientMessage_AcceptResponse | undefined;
}

export interface CreateStreamToClientMessage_CreateResponse {
  lobbyId: string;
}

export interface CreateStreamToClientMessage_ProposeIndication {
  opponentNickname: string;
  gameInfo: GameInfo | undefined;
}

export interface CreateStreamToClientMessage_AcceptResponse {
  sessionId: string;
  opponentSaveData: Uint8Array;
}

export interface CreateStreamToClientMessage_TimeoutIndication {}

export interface JoinRequest {
  lobbyId: string;
  nickname: string;
  gameInfo: GameInfo | undefined;
  saveData: Uint8Array;
}

export interface JoinResponse {
  sessionId: string;
  opponentSaveData: Uint8Array;
}

export interface GetInfoRequest {
  lobbyId: string;
}

export interface GetInfoResponse {
  creatorNickname: string;
  gameInfo: GameInfo | undefined;
  availableGames: GameInfo[];
  settings: Settings | undefined;
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
  return { createReq: undefined, acceptReq: undefined };
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
  return { saveData: new Uint8Array() };
}

export const CreateStreamToServerMessage_AcceptRequest = {
  encode(
    message: CreateStreamToServerMessage_AcceptRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.saveData.length !== 0) {
      writer.uint32(10).bytes(message.saveData);
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
      saveData: isSet(object.saveData)
        ? bytesFromBase64(object.saveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: CreateStreamToServerMessage_AcceptRequest): unknown {
    const obj: any = {};
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
    message.saveData = object.saveData ?? new Uint8Array();
    return message;
  },
};

function createBaseCreateStreamToClientMessage(): CreateStreamToClientMessage {
  return {
    timeoutInd: undefined,
    createResp: undefined,
    proposeInd: undefined,
    acceptResp: undefined,
  };
}

export const CreateStreamToClientMessage = {
  encode(
    message: CreateStreamToClientMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.timeoutInd !== undefined) {
      CreateStreamToClientMessage_TimeoutIndication.encode(
        message.timeoutInd,
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
          message.timeoutInd =
            CreateStreamToClientMessage_TimeoutIndication.decode(
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
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): CreateStreamToClientMessage {
    return {
      timeoutInd: isSet(object.timeoutInd)
        ? CreateStreamToClientMessage_TimeoutIndication.fromJSON(
            object.timeoutInd
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
    };
  },

  toJSON(message: CreateStreamToClientMessage): unknown {
    const obj: any = {};
    message.timeoutInd !== undefined &&
      (obj.timeoutInd = message.timeoutInd
        ? CreateStreamToClientMessage_TimeoutIndication.toJSON(
            message.timeoutInd
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
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<CreateStreamToClientMessage>, I>>(
    object: I
  ): CreateStreamToClientMessage {
    const message = createBaseCreateStreamToClientMessage();
    message.timeoutInd =
      object.timeoutInd !== undefined && object.timeoutInd !== null
        ? CreateStreamToClientMessage_TimeoutIndication.fromPartial(
            object.timeoutInd
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
  return { opponentNickname: "", gameInfo: undefined };
}

export const CreateStreamToClientMessage_ProposeIndication = {
  encode(
    message: CreateStreamToClientMessage_ProposeIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.opponentNickname !== "") {
      writer.uint32(10).string(message.opponentNickname);
    }
    if (message.gameInfo !== undefined) {
      GameInfo.encode(message.gameInfo, writer.uint32(18).fork()).ldelim();
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
          message.opponentNickname = reader.string();
          break;
        case 2:
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
    message.opponentNickname = object.opponentNickname ?? "";
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? GameInfo.fromPartial(object.gameInfo)
        : undefined;
    return message;
  },
};

function createBaseCreateStreamToClientMessage_AcceptResponse(): CreateStreamToClientMessage_AcceptResponse {
  return { sessionId: "", opponentSaveData: new Uint8Array() };
}

export const CreateStreamToClientMessage_AcceptResponse = {
  encode(
    message: CreateStreamToClientMessage_AcceptResponse,
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
  ): CreateStreamToClientMessage_AcceptResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage_AcceptResponse();
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

  fromJSON(object: any): CreateStreamToClientMessage_AcceptResponse {
    return {
      sessionId: isSet(object.sessionId) ? String(object.sessionId) : "",
      opponentSaveData: isSet(object.opponentSaveData)
        ? bytesFromBase64(object.opponentSaveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: CreateStreamToClientMessage_AcceptResponse): unknown {
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
    I extends Exact<DeepPartial<CreateStreamToClientMessage_AcceptResponse>, I>
  >(object: I): CreateStreamToClientMessage_AcceptResponse {
    const message = createBaseCreateStreamToClientMessage_AcceptResponse();
    message.sessionId = object.sessionId ?? "";
    message.opponentSaveData = object.opponentSaveData ?? new Uint8Array();
    return message;
  },
};

function createBaseCreateStreamToClientMessage_TimeoutIndication(): CreateStreamToClientMessage_TimeoutIndication {
  return {};
}

export const CreateStreamToClientMessage_TimeoutIndication = {
  encode(
    _: CreateStreamToClientMessage_TimeoutIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): CreateStreamToClientMessage_TimeoutIndication {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCreateStreamToClientMessage_TimeoutIndication();
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

  fromJSON(_: any): CreateStreamToClientMessage_TimeoutIndication {
    return {};
  },

  toJSON(_: CreateStreamToClientMessage_TimeoutIndication): unknown {
    const obj: any = {};
    return obj;
  },

  fromPartial<
    I extends Exact<
      DeepPartial<CreateStreamToClientMessage_TimeoutIndication>,
      I
    >
  >(_: I): CreateStreamToClientMessage_TimeoutIndication {
    const message = createBaseCreateStreamToClientMessage_TimeoutIndication();
    return message;
  },
};

function createBaseJoinRequest(): JoinRequest {
  return {
    lobbyId: "",
    nickname: "",
    gameInfo: undefined,
    saveData: new Uint8Array(),
  };
}

export const JoinRequest = {
  encode(
    message: JoinRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.lobbyId !== "") {
      writer.uint32(10).string(message.lobbyId);
    }
    if (message.nickname !== "") {
      writer.uint32(18).string(message.nickname);
    }
    if (message.gameInfo !== undefined) {
      GameInfo.encode(message.gameInfo, writer.uint32(26).fork()).ldelim();
    }
    if (message.saveData.length !== 0) {
      writer.uint32(34).bytes(message.saveData);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): JoinRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.lobbyId = reader.string();
          break;
        case 2:
          message.nickname = reader.string();
          break;
        case 3:
          message.gameInfo = GameInfo.decode(reader, reader.uint32());
          break;
        case 4:
          message.saveData = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): JoinRequest {
    return {
      lobbyId: isSet(object.lobbyId) ? String(object.lobbyId) : "",
      nickname: isSet(object.nickname) ? String(object.nickname) : "",
      gameInfo: isSet(object.gameInfo)
        ? GameInfo.fromJSON(object.gameInfo)
        : undefined,
      saveData: isSet(object.saveData)
        ? bytesFromBase64(object.saveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: JoinRequest): unknown {
    const obj: any = {};
    message.lobbyId !== undefined && (obj.lobbyId = message.lobbyId);
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

  fromPartial<I extends Exact<DeepPartial<JoinRequest>, I>>(
    object: I
  ): JoinRequest {
    const message = createBaseJoinRequest();
    message.lobbyId = object.lobbyId ?? "";
    message.nickname = object.nickname ?? "";
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? GameInfo.fromPartial(object.gameInfo)
        : undefined;
    message.saveData = object.saveData ?? new Uint8Array();
    return message;
  },
};

function createBaseJoinResponse(): JoinResponse {
  return { sessionId: "", opponentSaveData: new Uint8Array() };
}

export const JoinResponse = {
  encode(
    message: JoinResponse,
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

  decode(input: _m0.Reader | Uint8Array, length?: number): JoinResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseJoinResponse();
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

  fromJSON(object: any): JoinResponse {
    return {
      sessionId: isSet(object.sessionId) ? String(object.sessionId) : "",
      opponentSaveData: isSet(object.opponentSaveData)
        ? bytesFromBase64(object.opponentSaveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: JoinResponse): unknown {
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

  fromPartial<I extends Exact<DeepPartial<JoinResponse>, I>>(
    object: I
  ): JoinResponse {
    const message = createBaseJoinResponse();
    message.sessionId = object.sessionId ?? "";
    message.opponentSaveData = object.opponentSaveData ?? new Uint8Array();
    return message;
  },
};

function createBaseGetInfoRequest(): GetInfoRequest {
  return { lobbyId: "" };
}

export const GetInfoRequest = {
  encode(
    message: GetInfoRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.lobbyId !== "") {
      writer.uint32(10).string(message.lobbyId);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GetInfoRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetInfoRequest();
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

  fromJSON(object: any): GetInfoRequest {
    return {
      lobbyId: isSet(object.lobbyId) ? String(object.lobbyId) : "",
    };
  },

  toJSON(message: GetInfoRequest): unknown {
    const obj: any = {};
    message.lobbyId !== undefined && (obj.lobbyId = message.lobbyId);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GetInfoRequest>, I>>(
    object: I
  ): GetInfoRequest {
    const message = createBaseGetInfoRequest();
    message.lobbyId = object.lobbyId ?? "";
    return message;
  },
};

function createBaseGetInfoResponse(): GetInfoResponse {
  return {
    creatorNickname: "",
    gameInfo: undefined,
    availableGames: [],
    settings: undefined,
  };
}

export const GetInfoResponse = {
  encode(
    message: GetInfoResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.creatorNickname !== "") {
      writer.uint32(10).string(message.creatorNickname);
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

  decode(input: _m0.Reader | Uint8Array, length?: number): GetInfoResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetInfoResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.creatorNickname = reader.string();
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

  fromJSON(object: any): GetInfoResponse {
    return {
      creatorNickname: isSet(object.creatorNickname)
        ? String(object.creatorNickname)
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

  toJSON(message: GetInfoResponse): unknown {
    const obj: any = {};
    message.creatorNickname !== undefined &&
      (obj.creatorNickname = message.creatorNickname);
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

  fromPartial<I extends Exact<DeepPartial<GetInfoResponse>, I>>(
    object: I
  ): GetInfoResponse {
    const message = createBaseGetInfoResponse();
    message.creatorNickname = object.creatorNickname ?? "";
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

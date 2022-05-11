/* eslint-disable */
import Long from "long";
import * as _m0 from "protobufjs/minimal";

export const protobufPackage = "tango.ipc";

export interface FromCoreMessage {
  stateInd: FromCoreMessage_StateIndication | undefined;
  smuggleInd: FromCoreMessage_SmuggleIndication | undefined;
  connectionQualityInd: FromCoreMessage_ConnectionQualityIndication | undefined;
}

export interface FromCoreMessage_StateIndication {
  state: FromCoreMessage_StateIndication_State;
}

export enum FromCoreMessage_StateIndication_State {
  UNKNOWN = 0,
  RUNNING = 1,
  WAITING = 2,
  CONNECTING = 3,
  STARTING = 4,
  UNRECOGNIZED = -1,
}

export function fromCoreMessage_StateIndication_StateFromJSON(
  object: any
): FromCoreMessage_StateIndication_State {
  switch (object) {
    case 0:
    case "UNKNOWN":
      return FromCoreMessage_StateIndication_State.UNKNOWN;
    case 1:
    case "RUNNING":
      return FromCoreMessage_StateIndication_State.RUNNING;
    case 2:
    case "WAITING":
      return FromCoreMessage_StateIndication_State.WAITING;
    case 3:
    case "CONNECTING":
      return FromCoreMessage_StateIndication_State.CONNECTING;
    case 4:
    case "STARTING":
      return FromCoreMessage_StateIndication_State.STARTING;
    case -1:
    case "UNRECOGNIZED":
    default:
      return FromCoreMessage_StateIndication_State.UNRECOGNIZED;
  }
}

export function fromCoreMessage_StateIndication_StateToJSON(
  object: FromCoreMessage_StateIndication_State
): string {
  switch (object) {
    case FromCoreMessage_StateIndication_State.UNKNOWN:
      return "UNKNOWN";
    case FromCoreMessage_StateIndication_State.RUNNING:
      return "RUNNING";
    case FromCoreMessage_StateIndication_State.WAITING:
      return "WAITING";
    case FromCoreMessage_StateIndication_State.CONNECTING:
      return "CONNECTING";
    case FromCoreMessage_StateIndication_State.STARTING:
      return "STARTING";
    default:
      return "UNKNOWN";
  }
}

export interface FromCoreMessage_SmuggleIndication {
  data: Uint8Array;
}

export interface FromCoreMessage_ConnectionQualityIndication {
  rtt: number;
}

export interface ToCoreMessage {
  startReq: ToCoreMessage_StartRequest | undefined;
  smuggleReq: ToCoreMessage_SmuggleRequest | undefined;
}

export interface ToCoreMessage_StartRequest {
  windowTitle: string;
  romPath: string;
  savePath: string;
  settings: ToCoreMessage_StartRequest_MatchSettings | undefined;
}

export interface ToCoreMessage_StartRequest_MatchSettings {
  shadowSavePath: string;
  shadowRomPath: string;
  inputDelay: number;
  shadowInputDelay: number;
  matchType: number;
  replaysPath: string;
  replayMetadata: Uint8Array;
  rngSeed: Uint8Array;
  opponentNickname: string;
}

export interface ToCoreMessage_SmuggleRequest {
  data: Uint8Array;
}

function createBaseFromCoreMessage(): FromCoreMessage {
  return {
    stateInd: undefined,
    smuggleInd: undefined,
    connectionQualityInd: undefined,
  };
}

export const FromCoreMessage = {
  encode(
    message: FromCoreMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.stateInd !== undefined) {
      FromCoreMessage_StateIndication.encode(
        message.stateInd,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.smuggleInd !== undefined) {
      FromCoreMessage_SmuggleIndication.encode(
        message.smuggleInd,
        writer.uint32(18).fork()
      ).ldelim();
    }
    if (message.connectionQualityInd !== undefined) {
      FromCoreMessage_ConnectionQualityIndication.encode(
        message.connectionQualityInd,
        writer.uint32(26).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): FromCoreMessage {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.stateInd = FromCoreMessage_StateIndication.decode(
            reader,
            reader.uint32()
          );
          break;
        case 2:
          message.smuggleInd = FromCoreMessage_SmuggleIndication.decode(
            reader,
            reader.uint32()
          );
          break;
        case 3:
          message.connectionQualityInd =
            FromCoreMessage_ConnectionQualityIndication.decode(
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

  fromJSON(object: any): FromCoreMessage {
    return {
      stateInd: isSet(object.stateInd)
        ? FromCoreMessage_StateIndication.fromJSON(object.stateInd)
        : undefined,
      smuggleInd: isSet(object.smuggleInd)
        ? FromCoreMessage_SmuggleIndication.fromJSON(object.smuggleInd)
        : undefined,
      connectionQualityInd: isSet(object.connectionQualityInd)
        ? FromCoreMessage_ConnectionQualityIndication.fromJSON(
            object.connectionQualityInd
          )
        : undefined,
    };
  },

  toJSON(message: FromCoreMessage): unknown {
    const obj: any = {};
    message.stateInd !== undefined &&
      (obj.stateInd = message.stateInd
        ? FromCoreMessage_StateIndication.toJSON(message.stateInd)
        : undefined);
    message.smuggleInd !== undefined &&
      (obj.smuggleInd = message.smuggleInd
        ? FromCoreMessage_SmuggleIndication.toJSON(message.smuggleInd)
        : undefined);
    message.connectionQualityInd !== undefined &&
      (obj.connectionQualityInd = message.connectionQualityInd
        ? FromCoreMessage_ConnectionQualityIndication.toJSON(
            message.connectionQualityInd
          )
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<FromCoreMessage>, I>>(
    object: I
  ): FromCoreMessage {
    const message = createBaseFromCoreMessage();
    message.stateInd =
      object.stateInd !== undefined && object.stateInd !== null
        ? FromCoreMessage_StateIndication.fromPartial(object.stateInd)
        : undefined;
    message.smuggleInd =
      object.smuggleInd !== undefined && object.smuggleInd !== null
        ? FromCoreMessage_SmuggleIndication.fromPartial(object.smuggleInd)
        : undefined;
    message.connectionQualityInd =
      object.connectionQualityInd !== undefined &&
      object.connectionQualityInd !== null
        ? FromCoreMessage_ConnectionQualityIndication.fromPartial(
            object.connectionQualityInd
          )
        : undefined;
    return message;
  },
};

function createBaseFromCoreMessage_StateIndication(): FromCoreMessage_StateIndication {
  return { state: 0 };
}

export const FromCoreMessage_StateIndication = {
  encode(
    message: FromCoreMessage_StateIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.state !== 0) {
      writer.uint32(8).int32(message.state);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): FromCoreMessage_StateIndication {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage_StateIndication();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.state = reader.int32() as any;
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): FromCoreMessage_StateIndication {
    return {
      state: isSet(object.state)
        ? fromCoreMessage_StateIndication_StateFromJSON(object.state)
        : 0,
    };
  },

  toJSON(message: FromCoreMessage_StateIndication): unknown {
    const obj: any = {};
    message.state !== undefined &&
      (obj.state = fromCoreMessage_StateIndication_StateToJSON(message.state));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<FromCoreMessage_StateIndication>, I>>(
    object: I
  ): FromCoreMessage_StateIndication {
    const message = createBaseFromCoreMessage_StateIndication();
    message.state = object.state ?? 0;
    return message;
  },
};

function createBaseFromCoreMessage_SmuggleIndication(): FromCoreMessage_SmuggleIndication {
  return { data: new Uint8Array() };
}

export const FromCoreMessage_SmuggleIndication = {
  encode(
    message: FromCoreMessage_SmuggleIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.data.length !== 0) {
      writer.uint32(10).bytes(message.data);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): FromCoreMessage_SmuggleIndication {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage_SmuggleIndication();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.data = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): FromCoreMessage_SmuggleIndication {
    return {
      data: isSet(object.data)
        ? bytesFromBase64(object.data)
        : new Uint8Array(),
    };
  },

  toJSON(message: FromCoreMessage_SmuggleIndication): unknown {
    const obj: any = {};
    message.data !== undefined &&
      (obj.data = base64FromBytes(
        message.data !== undefined ? message.data : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<FromCoreMessage_SmuggleIndication>, I>
  >(object: I): FromCoreMessage_SmuggleIndication {
    const message = createBaseFromCoreMessage_SmuggleIndication();
    message.data = object.data ?? new Uint8Array();
    return message;
  },
};

function createBaseFromCoreMessage_ConnectionQualityIndication(): FromCoreMessage_ConnectionQualityIndication {
  return { rtt: 0 };
}

export const FromCoreMessage_ConnectionQualityIndication = {
  encode(
    message: FromCoreMessage_ConnectionQualityIndication,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.rtt !== 0) {
      writer.uint32(8).uint64(message.rtt);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): FromCoreMessage_ConnectionQualityIndication {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage_ConnectionQualityIndication();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.rtt = longToNumber(reader.uint64() as Long);
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): FromCoreMessage_ConnectionQualityIndication {
    return {
      rtt: isSet(object.rtt) ? Number(object.rtt) : 0,
    };
  },

  toJSON(message: FromCoreMessage_ConnectionQualityIndication): unknown {
    const obj: any = {};
    message.rtt !== undefined && (obj.rtt = Math.round(message.rtt));
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<FromCoreMessage_ConnectionQualityIndication>, I>
  >(object: I): FromCoreMessage_ConnectionQualityIndication {
    const message = createBaseFromCoreMessage_ConnectionQualityIndication();
    message.rtt = object.rtt ?? 0;
    return message;
  },
};

function createBaseToCoreMessage(): ToCoreMessage {
  return { startReq: undefined, smuggleReq: undefined };
}

export const ToCoreMessage = {
  encode(
    message: ToCoreMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.startReq !== undefined) {
      ToCoreMessage_StartRequest.encode(
        message.startReq,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.smuggleReq !== undefined) {
      ToCoreMessage_SmuggleRequest.encode(
        message.smuggleReq,
        writer.uint32(18).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): ToCoreMessage {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseToCoreMessage();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.startReq = ToCoreMessage_StartRequest.decode(
            reader,
            reader.uint32()
          );
          break;
        case 2:
          message.smuggleReq = ToCoreMessage_SmuggleRequest.decode(
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

  fromJSON(object: any): ToCoreMessage {
    return {
      startReq: isSet(object.startReq)
        ? ToCoreMessage_StartRequest.fromJSON(object.startReq)
        : undefined,
      smuggleReq: isSet(object.smuggleReq)
        ? ToCoreMessage_SmuggleRequest.fromJSON(object.smuggleReq)
        : undefined,
    };
  },

  toJSON(message: ToCoreMessage): unknown {
    const obj: any = {};
    message.startReq !== undefined &&
      (obj.startReq = message.startReq
        ? ToCoreMessage_StartRequest.toJSON(message.startReq)
        : undefined);
    message.smuggleReq !== undefined &&
      (obj.smuggleReq = message.smuggleReq
        ? ToCoreMessage_SmuggleRequest.toJSON(message.smuggleReq)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<ToCoreMessage>, I>>(
    object: I
  ): ToCoreMessage {
    const message = createBaseToCoreMessage();
    message.startReq =
      object.startReq !== undefined && object.startReq !== null
        ? ToCoreMessage_StartRequest.fromPartial(object.startReq)
        : undefined;
    message.smuggleReq =
      object.smuggleReq !== undefined && object.smuggleReq !== null
        ? ToCoreMessage_SmuggleRequest.fromPartial(object.smuggleReq)
        : undefined;
    return message;
  },
};

function createBaseToCoreMessage_StartRequest(): ToCoreMessage_StartRequest {
  return { windowTitle: "", romPath: "", savePath: "", settings: undefined };
}

export const ToCoreMessage_StartRequest = {
  encode(
    message: ToCoreMessage_StartRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.windowTitle !== "") {
      writer.uint32(10).string(message.windowTitle);
    }
    if (message.romPath !== "") {
      writer.uint32(18).string(message.romPath);
    }
    if (message.savePath !== "") {
      writer.uint32(26).string(message.savePath);
    }
    if (message.settings !== undefined) {
      ToCoreMessage_StartRequest_MatchSettings.encode(
        message.settings,
        writer.uint32(34).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): ToCoreMessage_StartRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseToCoreMessage_StartRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.windowTitle = reader.string();
          break;
        case 2:
          message.romPath = reader.string();
          break;
        case 3:
          message.savePath = reader.string();
          break;
        case 4:
          message.settings = ToCoreMessage_StartRequest_MatchSettings.decode(
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

  fromJSON(object: any): ToCoreMessage_StartRequest {
    return {
      windowTitle: isSet(object.windowTitle) ? String(object.windowTitle) : "",
      romPath: isSet(object.romPath) ? String(object.romPath) : "",
      savePath: isSet(object.savePath) ? String(object.savePath) : "",
      settings: isSet(object.settings)
        ? ToCoreMessage_StartRequest_MatchSettings.fromJSON(object.settings)
        : undefined,
    };
  },

  toJSON(message: ToCoreMessage_StartRequest): unknown {
    const obj: any = {};
    message.windowTitle !== undefined &&
      (obj.windowTitle = message.windowTitle);
    message.romPath !== undefined && (obj.romPath = message.romPath);
    message.savePath !== undefined && (obj.savePath = message.savePath);
    message.settings !== undefined &&
      (obj.settings = message.settings
        ? ToCoreMessage_StartRequest_MatchSettings.toJSON(message.settings)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<ToCoreMessage_StartRequest>, I>>(
    object: I
  ): ToCoreMessage_StartRequest {
    const message = createBaseToCoreMessage_StartRequest();
    message.windowTitle = object.windowTitle ?? "";
    message.romPath = object.romPath ?? "";
    message.savePath = object.savePath ?? "";
    message.settings =
      object.settings !== undefined && object.settings !== null
        ? ToCoreMessage_StartRequest_MatchSettings.fromPartial(object.settings)
        : undefined;
    return message;
  },
};

function createBaseToCoreMessage_StartRequest_MatchSettings(): ToCoreMessage_StartRequest_MatchSettings {
  return {
    shadowSavePath: "",
    shadowRomPath: "",
    inputDelay: 0,
    shadowInputDelay: 0,
    matchType: 0,
    replaysPath: "",
    replayMetadata: new Uint8Array(),
    rngSeed: new Uint8Array(),
    opponentNickname: "",
  };
}

export const ToCoreMessage_StartRequest_MatchSettings = {
  encode(
    message: ToCoreMessage_StartRequest_MatchSettings,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.shadowSavePath !== "") {
      writer.uint32(10).string(message.shadowSavePath);
    }
    if (message.shadowRomPath !== "") {
      writer.uint32(18).string(message.shadowRomPath);
    }
    if (message.inputDelay !== 0) {
      writer.uint32(24).uint32(message.inputDelay);
    }
    if (message.shadowInputDelay !== 0) {
      writer.uint32(32).uint32(message.shadowInputDelay);
    }
    if (message.matchType !== 0) {
      writer.uint32(40).uint32(message.matchType);
    }
    if (message.replaysPath !== "") {
      writer.uint32(50).string(message.replaysPath);
    }
    if (message.replayMetadata.length !== 0) {
      writer.uint32(58).bytes(message.replayMetadata);
    }
    if (message.rngSeed.length !== 0) {
      writer.uint32(66).bytes(message.rngSeed);
    }
    if (message.opponentNickname !== "") {
      writer.uint32(74).string(message.opponentNickname);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): ToCoreMessage_StartRequest_MatchSettings {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseToCoreMessage_StartRequest_MatchSettings();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.shadowSavePath = reader.string();
          break;
        case 2:
          message.shadowRomPath = reader.string();
          break;
        case 3:
          message.inputDelay = reader.uint32();
          break;
        case 4:
          message.shadowInputDelay = reader.uint32();
          break;
        case 5:
          message.matchType = reader.uint32();
          break;
        case 6:
          message.replaysPath = reader.string();
          break;
        case 7:
          message.replayMetadata = reader.bytes();
          break;
        case 8:
          message.rngSeed = reader.bytes();
          break;
        case 9:
          message.opponentNickname = reader.string();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): ToCoreMessage_StartRequest_MatchSettings {
    return {
      shadowSavePath: isSet(object.shadowSavePath)
        ? String(object.shadowSavePath)
        : "",
      shadowRomPath: isSet(object.shadowRomPath)
        ? String(object.shadowRomPath)
        : "",
      inputDelay: isSet(object.inputDelay) ? Number(object.inputDelay) : 0,
      shadowInputDelay: isSet(object.shadowInputDelay)
        ? Number(object.shadowInputDelay)
        : 0,
      matchType: isSet(object.matchType) ? Number(object.matchType) : 0,
      replaysPath: isSet(object.replaysPath) ? String(object.replaysPath) : "",
      replayMetadata: isSet(object.replayMetadata)
        ? bytesFromBase64(object.replayMetadata)
        : new Uint8Array(),
      rngSeed: isSet(object.rngSeed)
        ? bytesFromBase64(object.rngSeed)
        : new Uint8Array(),
      opponentNickname: isSet(object.opponentNickname)
        ? String(object.opponentNickname)
        : "",
    };
  },

  toJSON(message: ToCoreMessage_StartRequest_MatchSettings): unknown {
    const obj: any = {};
    message.shadowSavePath !== undefined &&
      (obj.shadowSavePath = message.shadowSavePath);
    message.shadowRomPath !== undefined &&
      (obj.shadowRomPath = message.shadowRomPath);
    message.inputDelay !== undefined &&
      (obj.inputDelay = Math.round(message.inputDelay));
    message.shadowInputDelay !== undefined &&
      (obj.shadowInputDelay = Math.round(message.shadowInputDelay));
    message.matchType !== undefined &&
      (obj.matchType = Math.round(message.matchType));
    message.replaysPath !== undefined &&
      (obj.replaysPath = message.replaysPath);
    message.replayMetadata !== undefined &&
      (obj.replayMetadata = base64FromBytes(
        message.replayMetadata !== undefined
          ? message.replayMetadata
          : new Uint8Array()
      ));
    message.rngSeed !== undefined &&
      (obj.rngSeed = base64FromBytes(
        message.rngSeed !== undefined ? message.rngSeed : new Uint8Array()
      ));
    message.opponentNickname !== undefined &&
      (obj.opponentNickname = message.opponentNickname);
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<ToCoreMessage_StartRequest_MatchSettings>, I>
  >(object: I): ToCoreMessage_StartRequest_MatchSettings {
    const message = createBaseToCoreMessage_StartRequest_MatchSettings();
    message.shadowSavePath = object.shadowSavePath ?? "";
    message.shadowRomPath = object.shadowRomPath ?? "";
    message.inputDelay = object.inputDelay ?? 0;
    message.shadowInputDelay = object.shadowInputDelay ?? 0;
    message.matchType = object.matchType ?? 0;
    message.replaysPath = object.replaysPath ?? "";
    message.replayMetadata = object.replayMetadata ?? new Uint8Array();
    message.rngSeed = object.rngSeed ?? new Uint8Array();
    message.opponentNickname = object.opponentNickname ?? "";
    return message;
  },
};

function createBaseToCoreMessage_SmuggleRequest(): ToCoreMessage_SmuggleRequest {
  return { data: new Uint8Array() };
}

export const ToCoreMessage_SmuggleRequest = {
  encode(
    message: ToCoreMessage_SmuggleRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.data.length !== 0) {
      writer.uint32(10).bytes(message.data);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): ToCoreMessage_SmuggleRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseToCoreMessage_SmuggleRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.data = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): ToCoreMessage_SmuggleRequest {
    return {
      data: isSet(object.data)
        ? bytesFromBase64(object.data)
        : new Uint8Array(),
    };
  },

  toJSON(message: ToCoreMessage_SmuggleRequest): unknown {
    const obj: any = {};
    message.data !== undefined &&
      (obj.data = base64FromBytes(
        message.data !== undefined ? message.data : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<ToCoreMessage_SmuggleRequest>, I>>(
    object: I
  ): ToCoreMessage_SmuggleRequest {
    const message = createBaseToCoreMessage_SmuggleRequest();
    message.data = object.data ?? new Uint8Array();
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

function longToNumber(long: Long): number {
  if (long.gt(Number.MAX_SAFE_INTEGER)) {
    throw new globalThis.Error("Value is larger than Number.MAX_SAFE_INTEGER");
  }
  return long.toNumber();
}

if (_m0.util.Long !== Long) {
  _m0.util.Long = Long as any;
  _m0.configure();
}

function isSet(value: any): boolean {
  return value !== null && value !== undefined;
}

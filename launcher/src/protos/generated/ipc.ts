/* eslint-disable */
import Long from "long";
import * as _m0 from "protobufjs/minimal";

export const protobufPackage = "tango.ipc";

export enum ExitCode {
  EXIT_CODE_UNKNOWN = 0,
  EXIT_CODE_LOST_CONNECTION = 10,
  EXIT_CODE_PROTOCOL_VERSION_TOO_OLD = 11,
  EXIT_CODE_PROTOCOL_VERSION_TOO_NEW = 12,
  EXIT_CODE_RUST_PANIC = 101,
  UNRECOGNIZED = -1,
}

export function exitCodeFromJSON(object: any): ExitCode {
  switch (object) {
    case 0:
    case "EXIT_CODE_UNKNOWN":
      return ExitCode.EXIT_CODE_UNKNOWN;
    case 10:
    case "EXIT_CODE_LOST_CONNECTION":
      return ExitCode.EXIT_CODE_LOST_CONNECTION;
    case 11:
    case "EXIT_CODE_PROTOCOL_VERSION_TOO_OLD":
      return ExitCode.EXIT_CODE_PROTOCOL_VERSION_TOO_OLD;
    case 12:
    case "EXIT_CODE_PROTOCOL_VERSION_TOO_NEW":
      return ExitCode.EXIT_CODE_PROTOCOL_VERSION_TOO_NEW;
    case 101:
    case "EXIT_CODE_RUST_PANIC":
      return ExitCode.EXIT_CODE_RUST_PANIC;
    case -1:
    case "UNRECOGNIZED":
    default:
      return ExitCode.UNRECOGNIZED;
  }
}

export function exitCodeToJSON(object: ExitCode): string {
  switch (object) {
    case ExitCode.EXIT_CODE_UNKNOWN:
      return "EXIT_CODE_UNKNOWN";
    case ExitCode.EXIT_CODE_LOST_CONNECTION:
      return "EXIT_CODE_LOST_CONNECTION";
    case ExitCode.EXIT_CODE_PROTOCOL_VERSION_TOO_OLD:
      return "EXIT_CODE_PROTOCOL_VERSION_TOO_OLD";
    case ExitCode.EXIT_CODE_PROTOCOL_VERSION_TOO_NEW:
      return "EXIT_CODE_PROTOCOL_VERSION_TOO_NEW";
    case ExitCode.EXIT_CODE_RUST_PANIC:
      return "EXIT_CODE_RUST_PANIC";
    default:
      return "UNKNOWN";
  }
}

export interface FromCoreMessage {
  stateEv: FromCoreMessage_StateEvent | undefined;
  smuggleEv: FromCoreMessage_SmuggleEvent | undefined;
  connectionQualityEv: FromCoreMessage_ConnectionQualityEvent | undefined;
  roundEndedEv: FromCoreMessage_RoundEndedEvent | undefined;
}

export interface FromCoreMessage_StateEvent {
  state: FromCoreMessage_StateEvent_State;
}

export enum FromCoreMessage_StateEvent_State {
  UNKNOWN = 0,
  RUNNING = 1,
  WAITING = 2,
  CONNECTING = 3,
  STARTING = 4,
  UNRECOGNIZED = -1,
}

export function fromCoreMessage_StateEvent_StateFromJSON(
  object: any
): FromCoreMessage_StateEvent_State {
  switch (object) {
    case 0:
    case "UNKNOWN":
      return FromCoreMessage_StateEvent_State.UNKNOWN;
    case 1:
    case "RUNNING":
      return FromCoreMessage_StateEvent_State.RUNNING;
    case 2:
    case "WAITING":
      return FromCoreMessage_StateEvent_State.WAITING;
    case 3:
    case "CONNECTING":
      return FromCoreMessage_StateEvent_State.CONNECTING;
    case 4:
    case "STARTING":
      return FromCoreMessage_StateEvent_State.STARTING;
    case -1:
    case "UNRECOGNIZED":
    default:
      return FromCoreMessage_StateEvent_State.UNRECOGNIZED;
  }
}

export function fromCoreMessage_StateEvent_StateToJSON(
  object: FromCoreMessage_StateEvent_State
): string {
  switch (object) {
    case FromCoreMessage_StateEvent_State.UNKNOWN:
      return "UNKNOWN";
    case FromCoreMessage_StateEvent_State.RUNNING:
      return "RUNNING";
    case FromCoreMessage_StateEvent_State.WAITING:
      return "WAITING";
    case FromCoreMessage_StateEvent_State.CONNECTING:
      return "CONNECTING";
    case FromCoreMessage_StateEvent_State.STARTING:
      return "STARTING";
    default:
      return "UNKNOWN";
  }
}

export interface FromCoreMessage_SmuggleEvent {
  data: Uint8Array;
}

export interface FromCoreMessage_ConnectionQualityEvent {
  rtt: number;
}

export interface FromCoreMessage_RoundEndedEvent {
  replayFilename: string;
}

export interface ToCoreMessage {
  startReq: ToCoreMessage_StartRequest | undefined;
  smuggleReq: ToCoreMessage_SmuggleRequest | undefined;
}

export interface ToCoreMessage_StartRequest {
  windowTitle: string;
  romPath: string;
  savePath: string;
  windowScale: number;
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
  opponentNickname?: string | undefined;
  maxQueueLength: number;
}

export interface ToCoreMessage_SmuggleRequest {
  data: Uint8Array;
}

function createBaseFromCoreMessage(): FromCoreMessage {
  return {
    stateEv: undefined,
    smuggleEv: undefined,
    connectionQualityEv: undefined,
    roundEndedEv: undefined,
  };
}

export const FromCoreMessage = {
  encode(
    message: FromCoreMessage,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.stateEv !== undefined) {
      FromCoreMessage_StateEvent.encode(
        message.stateEv,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.smuggleEv !== undefined) {
      FromCoreMessage_SmuggleEvent.encode(
        message.smuggleEv,
        writer.uint32(18).fork()
      ).ldelim();
    }
    if (message.connectionQualityEv !== undefined) {
      FromCoreMessage_ConnectionQualityEvent.encode(
        message.connectionQualityEv,
        writer.uint32(26).fork()
      ).ldelim();
    }
    if (message.roundEndedEv !== undefined) {
      FromCoreMessage_RoundEndedEvent.encode(
        message.roundEndedEv,
        writer.uint32(34).fork()
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
          message.stateEv = FromCoreMessage_StateEvent.decode(
            reader,
            reader.uint32()
          );
          break;
        case 2:
          message.smuggleEv = FromCoreMessage_SmuggleEvent.decode(
            reader,
            reader.uint32()
          );
          break;
        case 3:
          message.connectionQualityEv =
            FromCoreMessage_ConnectionQualityEvent.decode(
              reader,
              reader.uint32()
            );
          break;
        case 4:
          message.roundEndedEv = FromCoreMessage_RoundEndedEvent.decode(
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
      stateEv: isSet(object.stateEv)
        ? FromCoreMessage_StateEvent.fromJSON(object.stateEv)
        : undefined,
      smuggleEv: isSet(object.smuggleEv)
        ? FromCoreMessage_SmuggleEvent.fromJSON(object.smuggleEv)
        : undefined,
      connectionQualityEv: isSet(object.connectionQualityEv)
        ? FromCoreMessage_ConnectionQualityEvent.fromJSON(
            object.connectionQualityEv
          )
        : undefined,
      roundEndedEv: isSet(object.roundEndedEv)
        ? FromCoreMessage_RoundEndedEvent.fromJSON(object.roundEndedEv)
        : undefined,
    };
  },

  toJSON(message: FromCoreMessage): unknown {
    const obj: any = {};
    message.stateEv !== undefined &&
      (obj.stateEv = message.stateEv
        ? FromCoreMessage_StateEvent.toJSON(message.stateEv)
        : undefined);
    message.smuggleEv !== undefined &&
      (obj.smuggleEv = message.smuggleEv
        ? FromCoreMessage_SmuggleEvent.toJSON(message.smuggleEv)
        : undefined);
    message.connectionQualityEv !== undefined &&
      (obj.connectionQualityEv = message.connectionQualityEv
        ? FromCoreMessage_ConnectionQualityEvent.toJSON(
            message.connectionQualityEv
          )
        : undefined);
    message.roundEndedEv !== undefined &&
      (obj.roundEndedEv = message.roundEndedEv
        ? FromCoreMessage_RoundEndedEvent.toJSON(message.roundEndedEv)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<FromCoreMessage>, I>>(
    object: I
  ): FromCoreMessage {
    const message = createBaseFromCoreMessage();
    message.stateEv =
      object.stateEv !== undefined && object.stateEv !== null
        ? FromCoreMessage_StateEvent.fromPartial(object.stateEv)
        : undefined;
    message.smuggleEv =
      object.smuggleEv !== undefined && object.smuggleEv !== null
        ? FromCoreMessage_SmuggleEvent.fromPartial(object.smuggleEv)
        : undefined;
    message.connectionQualityEv =
      object.connectionQualityEv !== undefined &&
      object.connectionQualityEv !== null
        ? FromCoreMessage_ConnectionQualityEvent.fromPartial(
            object.connectionQualityEv
          )
        : undefined;
    message.roundEndedEv =
      object.roundEndedEv !== undefined && object.roundEndedEv !== null
        ? FromCoreMessage_RoundEndedEvent.fromPartial(object.roundEndedEv)
        : undefined;
    return message;
  },
};

function createBaseFromCoreMessage_StateEvent(): FromCoreMessage_StateEvent {
  return { state: 0 };
}

export const FromCoreMessage_StateEvent = {
  encode(
    message: FromCoreMessage_StateEvent,
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
  ): FromCoreMessage_StateEvent {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage_StateEvent();
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

  fromJSON(object: any): FromCoreMessage_StateEvent {
    return {
      state: isSet(object.state)
        ? fromCoreMessage_StateEvent_StateFromJSON(object.state)
        : 0,
    };
  },

  toJSON(message: FromCoreMessage_StateEvent): unknown {
    const obj: any = {};
    message.state !== undefined &&
      (obj.state = fromCoreMessage_StateEvent_StateToJSON(message.state));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<FromCoreMessage_StateEvent>, I>>(
    object: I
  ): FromCoreMessage_StateEvent {
    const message = createBaseFromCoreMessage_StateEvent();
    message.state = object.state ?? 0;
    return message;
  },
};

function createBaseFromCoreMessage_SmuggleEvent(): FromCoreMessage_SmuggleEvent {
  return { data: new Uint8Array() };
}

export const FromCoreMessage_SmuggleEvent = {
  encode(
    message: FromCoreMessage_SmuggleEvent,
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
  ): FromCoreMessage_SmuggleEvent {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage_SmuggleEvent();
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

  fromJSON(object: any): FromCoreMessage_SmuggleEvent {
    return {
      data: isSet(object.data)
        ? bytesFromBase64(object.data)
        : new Uint8Array(),
    };
  },

  toJSON(message: FromCoreMessage_SmuggleEvent): unknown {
    const obj: any = {};
    message.data !== undefined &&
      (obj.data = base64FromBytes(
        message.data !== undefined ? message.data : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<FromCoreMessage_SmuggleEvent>, I>>(
    object: I
  ): FromCoreMessage_SmuggleEvent {
    const message = createBaseFromCoreMessage_SmuggleEvent();
    message.data = object.data ?? new Uint8Array();
    return message;
  },
};

function createBaseFromCoreMessage_ConnectionQualityEvent(): FromCoreMessage_ConnectionQualityEvent {
  return { rtt: 0 };
}

export const FromCoreMessage_ConnectionQualityEvent = {
  encode(
    message: FromCoreMessage_ConnectionQualityEvent,
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
  ): FromCoreMessage_ConnectionQualityEvent {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage_ConnectionQualityEvent();
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

  fromJSON(object: any): FromCoreMessage_ConnectionQualityEvent {
    return {
      rtt: isSet(object.rtt) ? Number(object.rtt) : 0,
    };
  },

  toJSON(message: FromCoreMessage_ConnectionQualityEvent): unknown {
    const obj: any = {};
    message.rtt !== undefined && (obj.rtt = Math.round(message.rtt));
    return obj;
  },

  fromPartial<
    I extends Exact<DeepPartial<FromCoreMessage_ConnectionQualityEvent>, I>
  >(object: I): FromCoreMessage_ConnectionQualityEvent {
    const message = createBaseFromCoreMessage_ConnectionQualityEvent();
    message.rtt = object.rtt ?? 0;
    return message;
  },
};

function createBaseFromCoreMessage_RoundEndedEvent(): FromCoreMessage_RoundEndedEvent {
  return { replayFilename: "" };
}

export const FromCoreMessage_RoundEndedEvent = {
  encode(
    message: FromCoreMessage_RoundEndedEvent,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.replayFilename !== "") {
      writer.uint32(10).string(message.replayFilename);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): FromCoreMessage_RoundEndedEvent {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseFromCoreMessage_RoundEndedEvent();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.replayFilename = reader.string();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): FromCoreMessage_RoundEndedEvent {
    return {
      replayFilename: isSet(object.replayFilename)
        ? String(object.replayFilename)
        : "",
    };
  },

  toJSON(message: FromCoreMessage_RoundEndedEvent): unknown {
    const obj: any = {};
    message.replayFilename !== undefined &&
      (obj.replayFilename = message.replayFilename);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<FromCoreMessage_RoundEndedEvent>, I>>(
    object: I
  ): FromCoreMessage_RoundEndedEvent {
    const message = createBaseFromCoreMessage_RoundEndedEvent();
    message.replayFilename = object.replayFilename ?? "";
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
  return {
    windowTitle: "",
    romPath: "",
    savePath: "",
    windowScale: 0,
    settings: undefined,
  };
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
    if (message.windowScale !== 0) {
      writer.uint32(32).uint32(message.windowScale);
    }
    if (message.settings !== undefined) {
      ToCoreMessage_StartRequest_MatchSettings.encode(
        message.settings,
        writer.uint32(42).fork()
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
          message.windowScale = reader.uint32();
          break;
        case 5:
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
      windowScale: isSet(object.windowScale) ? Number(object.windowScale) : 0,
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
    message.windowScale !== undefined &&
      (obj.windowScale = Math.round(message.windowScale));
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
    message.windowScale = object.windowScale ?? 0;
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
    opponentNickname: undefined,
    maxQueueLength: 0,
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
    if (message.opponentNickname !== undefined) {
      writer.uint32(74).string(message.opponentNickname);
    }
    if (message.maxQueueLength !== 0) {
      writer.uint32(80).uint32(message.maxQueueLength);
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
        case 10:
          message.maxQueueLength = reader.uint32();
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
        : undefined,
      maxQueueLength: isSet(object.maxQueueLength)
        ? Number(object.maxQueueLength)
        : 0,
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
    message.maxQueueLength !== undefined &&
      (obj.maxQueueLength = Math.round(message.maxQueueLength));
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
    message.opponentNickname = object.opponentNickname ?? undefined;
    message.maxQueueLength = object.maxQueueLength ?? 0;
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

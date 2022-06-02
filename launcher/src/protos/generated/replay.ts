/* eslint-disable */
import Long from "long";
import * as _m0 from "protobufjs/minimal";

export const protobufPackage = "";

export interface ReplayMetadata {
  ts: number;
  linkCode: string;
  localSide: ReplayMetadata_Side | undefined;
  remoteSide: ReplayMetadata_Side | undefined;
}

export interface ReplayMetadata_GameInfo {
  rom: string;
  patch: ReplayMetadata_GameInfo_Patch | undefined;
}

export interface ReplayMetadata_GameInfo_Patch {
  name: string;
  version: string;
}

export interface ReplayMetadata_Side {
  nickname: string;
  gameInfo: ReplayMetadata_GameInfo | undefined;
  revealSetup: boolean;
}

function createBaseReplayMetadata(): ReplayMetadata {
  return { ts: 0, linkCode: "", localSide: undefined, remoteSide: undefined };
}

export const ReplayMetadata = {
  encode(
    message: ReplayMetadata,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.ts !== 0) {
      writer.uint32(8).uint64(message.ts);
    }
    if (message.linkCode !== "") {
      writer.uint32(18).string(message.linkCode);
    }
    if (message.localSide !== undefined) {
      ReplayMetadata_Side.encode(
        message.localSide,
        writer.uint32(26).fork()
      ).ldelim();
    }
    if (message.remoteSide !== undefined) {
      ReplayMetadata_Side.encode(
        message.remoteSide,
        writer.uint32(34).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): ReplayMetadata {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseReplayMetadata();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.ts = longToNumber(reader.uint64() as Long);
          break;
        case 2:
          message.linkCode = reader.string();
          break;
        case 3:
          message.localSide = ReplayMetadata_Side.decode(
            reader,
            reader.uint32()
          );
          break;
        case 4:
          message.remoteSide = ReplayMetadata_Side.decode(
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

  fromJSON(object: any): ReplayMetadata {
    return {
      ts: isSet(object.ts) ? Number(object.ts) : 0,
      linkCode: isSet(object.linkCode) ? String(object.linkCode) : "",
      localSide: isSet(object.localSide)
        ? ReplayMetadata_Side.fromJSON(object.localSide)
        : undefined,
      remoteSide: isSet(object.remoteSide)
        ? ReplayMetadata_Side.fromJSON(object.remoteSide)
        : undefined,
    };
  },

  toJSON(message: ReplayMetadata): unknown {
    const obj: any = {};
    message.ts !== undefined && (obj.ts = Math.round(message.ts));
    message.linkCode !== undefined && (obj.linkCode = message.linkCode);
    message.localSide !== undefined &&
      (obj.localSide = message.localSide
        ? ReplayMetadata_Side.toJSON(message.localSide)
        : undefined);
    message.remoteSide !== undefined &&
      (obj.remoteSide = message.remoteSide
        ? ReplayMetadata_Side.toJSON(message.remoteSide)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<ReplayMetadata>, I>>(
    object: I
  ): ReplayMetadata {
    const message = createBaseReplayMetadata();
    message.ts = object.ts ?? 0;
    message.linkCode = object.linkCode ?? "";
    message.localSide =
      object.localSide !== undefined && object.localSide !== null
        ? ReplayMetadata_Side.fromPartial(object.localSide)
        : undefined;
    message.remoteSide =
      object.remoteSide !== undefined && object.remoteSide !== null
        ? ReplayMetadata_Side.fromPartial(object.remoteSide)
        : undefined;
    return message;
  },
};

function createBaseReplayMetadata_GameInfo(): ReplayMetadata_GameInfo {
  return { rom: "", patch: undefined };
}

export const ReplayMetadata_GameInfo = {
  encode(
    message: ReplayMetadata_GameInfo,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.rom !== "") {
      writer.uint32(10).string(message.rom);
    }
    if (message.patch !== undefined) {
      ReplayMetadata_GameInfo_Patch.encode(
        message.patch,
        writer.uint32(18).fork()
      ).ldelim();
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): ReplayMetadata_GameInfo {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseReplayMetadata_GameInfo();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.rom = reader.string();
          break;
        case 2:
          message.patch = ReplayMetadata_GameInfo_Patch.decode(
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

  fromJSON(object: any): ReplayMetadata_GameInfo {
    return {
      rom: isSet(object.rom) ? String(object.rom) : "",
      patch: isSet(object.patch)
        ? ReplayMetadata_GameInfo_Patch.fromJSON(object.patch)
        : undefined,
    };
  },

  toJSON(message: ReplayMetadata_GameInfo): unknown {
    const obj: any = {};
    message.rom !== undefined && (obj.rom = message.rom);
    message.patch !== undefined &&
      (obj.patch = message.patch
        ? ReplayMetadata_GameInfo_Patch.toJSON(message.patch)
        : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<ReplayMetadata_GameInfo>, I>>(
    object: I
  ): ReplayMetadata_GameInfo {
    const message = createBaseReplayMetadata_GameInfo();
    message.rom = object.rom ?? "";
    message.patch =
      object.patch !== undefined && object.patch !== null
        ? ReplayMetadata_GameInfo_Patch.fromPartial(object.patch)
        : undefined;
    return message;
  },
};

function createBaseReplayMetadata_GameInfo_Patch(): ReplayMetadata_GameInfo_Patch {
  return { name: "", version: "" };
}

export const ReplayMetadata_GameInfo_Patch = {
  encode(
    message: ReplayMetadata_GameInfo_Patch,
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

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): ReplayMetadata_GameInfo_Patch {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseReplayMetadata_GameInfo_Patch();
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

  fromJSON(object: any): ReplayMetadata_GameInfo_Patch {
    return {
      name: isSet(object.name) ? String(object.name) : "",
      version: isSet(object.version) ? String(object.version) : "",
    };
  },

  toJSON(message: ReplayMetadata_GameInfo_Patch): unknown {
    const obj: any = {};
    message.name !== undefined && (obj.name = message.name);
    message.version !== undefined && (obj.version = message.version);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<ReplayMetadata_GameInfo_Patch>, I>>(
    object: I
  ): ReplayMetadata_GameInfo_Patch {
    const message = createBaseReplayMetadata_GameInfo_Patch();
    message.name = object.name ?? "";
    message.version = object.version ?? "";
    return message;
  },
};

function createBaseReplayMetadata_Side(): ReplayMetadata_Side {
  return { nickname: "", gameInfo: undefined, revealSetup: false };
}

export const ReplayMetadata_Side = {
  encode(
    message: ReplayMetadata_Side,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.nickname !== "") {
      writer.uint32(10).string(message.nickname);
    }
    if (message.gameInfo !== undefined) {
      ReplayMetadata_GameInfo.encode(
        message.gameInfo,
        writer.uint32(18).fork()
      ).ldelim();
    }
    if (message.revealSetup === true) {
      writer.uint32(24).bool(message.revealSetup);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): ReplayMetadata_Side {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseReplayMetadata_Side();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.nickname = reader.string();
          break;
        case 2:
          message.gameInfo = ReplayMetadata_GameInfo.decode(
            reader,
            reader.uint32()
          );
          break;
        case 3:
          message.revealSetup = reader.bool();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): ReplayMetadata_Side {
    return {
      nickname: isSet(object.nickname) ? String(object.nickname) : "",
      gameInfo: isSet(object.gameInfo)
        ? ReplayMetadata_GameInfo.fromJSON(object.gameInfo)
        : undefined,
      revealSetup: isSet(object.revealSetup)
        ? Boolean(object.revealSetup)
        : false,
    };
  },

  toJSON(message: ReplayMetadata_Side): unknown {
    const obj: any = {};
    message.nickname !== undefined && (obj.nickname = message.nickname);
    message.gameInfo !== undefined &&
      (obj.gameInfo = message.gameInfo
        ? ReplayMetadata_GameInfo.toJSON(message.gameInfo)
        : undefined);
    message.revealSetup !== undefined &&
      (obj.revealSetup = message.revealSetup);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<ReplayMetadata_Side>, I>>(
    object: I
  ): ReplayMetadata_Side {
    const message = createBaseReplayMetadata_Side();
    message.nickname = object.nickname ?? "";
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? ReplayMetadata_GameInfo.fromPartial(object.gameInfo)
        : undefined;
    message.revealSetup = object.revealSetup ?? false;
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

/* eslint-disable */
import Long from "long";
import * as _m0 from "protobufjs/minimal";

export const protobufPackage = "";

export interface GameInfo {
  rom: string;
  patch: GameInfo_Patch | undefined;
}

export interface GameInfo_Patch {
  name: string;
  version: string;
}

export interface SetSettings {
  nickname: string;
  matchType: number;
  gameInfo: GameInfo | undefined;
  availableGames: GameInfo[];
  inputDelay: number;
  revealSetup: boolean;
}

export interface Commit {
  commitment: Uint8Array;
}

export interface Uncommit {}

export interface Chunk {
  chunk: Uint8Array;
}

export interface Message {
  setSettings: SetSettings | undefined;
  commit: Commit | undefined;
  uncommit: Uncommit | undefined;
  chunk: Chunk | undefined;
}

export interface NegotiatedState {
  nonce: Uint8Array;
  saveData: Uint8Array;
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

function createBaseSetSettings(): SetSettings {
  return {
    nickname: "",
    matchType: 0,
    gameInfo: undefined,
    availableGames: [],
    inputDelay: 0,
    revealSetup: false,
  };
}

export const SetSettings = {
  encode(
    message: SetSettings,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.nickname !== "") {
      writer.uint32(10).string(message.nickname);
    }
    if (message.matchType !== 0) {
      writer.uint32(16).uint32(message.matchType);
    }
    if (message.gameInfo !== undefined) {
      GameInfo.encode(message.gameInfo, writer.uint32(26).fork()).ldelim();
    }
    for (const v of message.availableGames) {
      GameInfo.encode(v!, writer.uint32(34).fork()).ldelim();
    }
    if (message.inputDelay !== 0) {
      writer.uint32(40).uint32(message.inputDelay);
    }
    if (message.revealSetup === true) {
      writer.uint32(48).bool(message.revealSetup);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): SetSettings {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseSetSettings();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.nickname = reader.string();
          break;
        case 2:
          message.matchType = reader.uint32();
          break;
        case 3:
          message.gameInfo = GameInfo.decode(reader, reader.uint32());
          break;
        case 4:
          message.availableGames.push(GameInfo.decode(reader, reader.uint32()));
          break;
        case 5:
          message.inputDelay = reader.uint32();
          break;
        case 6:
          message.revealSetup = reader.bool();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): SetSettings {
    return {
      nickname: isSet(object.nickname) ? String(object.nickname) : "",
      matchType: isSet(object.matchType) ? Number(object.matchType) : 0,
      gameInfo: isSet(object.gameInfo)
        ? GameInfo.fromJSON(object.gameInfo)
        : undefined,
      availableGames: Array.isArray(object?.availableGames)
        ? object.availableGames.map((e: any) => GameInfo.fromJSON(e))
        : [],
      inputDelay: isSet(object.inputDelay) ? Number(object.inputDelay) : 0,
      revealSetup: isSet(object.revealSetup)
        ? Boolean(object.revealSetup)
        : false,
    };
  },

  toJSON(message: SetSettings): unknown {
    const obj: any = {};
    message.nickname !== undefined && (obj.nickname = message.nickname);
    message.matchType !== undefined &&
      (obj.matchType = Math.round(message.matchType));
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
    message.inputDelay !== undefined &&
      (obj.inputDelay = Math.round(message.inputDelay));
    message.revealSetup !== undefined &&
      (obj.revealSetup = message.revealSetup);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<SetSettings>, I>>(
    object: I
  ): SetSettings {
    const message = createBaseSetSettings();
    message.nickname = object.nickname ?? "";
    message.matchType = object.matchType ?? 0;
    message.gameInfo =
      object.gameInfo !== undefined && object.gameInfo !== null
        ? GameInfo.fromPartial(object.gameInfo)
        : undefined;
    message.availableGames =
      object.availableGames?.map((e) => GameInfo.fromPartial(e)) || [];
    message.inputDelay = object.inputDelay ?? 0;
    message.revealSetup = object.revealSetup ?? false;
    return message;
  },
};

function createBaseCommit(): Commit {
  return { commitment: new Uint8Array() };
}

export const Commit = {
  encode(
    message: Commit,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.commitment.length !== 0) {
      writer.uint32(10).bytes(message.commitment);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): Commit {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseCommit();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.commitment = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): Commit {
    return {
      commitment: isSet(object.commitment)
        ? bytesFromBase64(object.commitment)
        : new Uint8Array(),
    };
  },

  toJSON(message: Commit): unknown {
    const obj: any = {};
    message.commitment !== undefined &&
      (obj.commitment = base64FromBytes(
        message.commitment !== undefined ? message.commitment : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<Commit>, I>>(object: I): Commit {
    const message = createBaseCommit();
    message.commitment = object.commitment ?? new Uint8Array();
    return message;
  },
};

function createBaseUncommit(): Uncommit {
  return {};
}

export const Uncommit = {
  encode(_: Uncommit, writer: _m0.Writer = _m0.Writer.create()): _m0.Writer {
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): Uncommit {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseUncommit();
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

  fromJSON(_: any): Uncommit {
    return {};
  },

  toJSON(_: Uncommit): unknown {
    const obj: any = {};
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<Uncommit>, I>>(_: I): Uncommit {
    const message = createBaseUncommit();
    return message;
  },
};

function createBaseChunk(): Chunk {
  return { chunk: new Uint8Array() };
}

export const Chunk = {
  encode(message: Chunk, writer: _m0.Writer = _m0.Writer.create()): _m0.Writer {
    if (message.chunk.length !== 0) {
      writer.uint32(10).bytes(message.chunk);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): Chunk {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseChunk();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.chunk = reader.bytes();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): Chunk {
    return {
      chunk: isSet(object.chunk)
        ? bytesFromBase64(object.chunk)
        : new Uint8Array(),
    };
  },

  toJSON(message: Chunk): unknown {
    const obj: any = {};
    message.chunk !== undefined &&
      (obj.chunk = base64FromBytes(
        message.chunk !== undefined ? message.chunk : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<Chunk>, I>>(object: I): Chunk {
    const message = createBaseChunk();
    message.chunk = object.chunk ?? new Uint8Array();
    return message;
  },
};

function createBaseMessage(): Message {
  return {
    setSettings: undefined,
    commit: undefined,
    uncommit: undefined,
    chunk: undefined,
  };
}

export const Message = {
  encode(
    message: Message,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.setSettings !== undefined) {
      SetSettings.encode(
        message.setSettings,
        writer.uint32(10).fork()
      ).ldelim();
    }
    if (message.commit !== undefined) {
      Commit.encode(message.commit, writer.uint32(18).fork()).ldelim();
    }
    if (message.uncommit !== undefined) {
      Uncommit.encode(message.uncommit, writer.uint32(26).fork()).ldelim();
    }
    if (message.chunk !== undefined) {
      Chunk.encode(message.chunk, writer.uint32(34).fork()).ldelim();
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): Message {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseMessage();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.setSettings = SetSettings.decode(reader, reader.uint32());
          break;
        case 2:
          message.commit = Commit.decode(reader, reader.uint32());
          break;
        case 3:
          message.uncommit = Uncommit.decode(reader, reader.uint32());
          break;
        case 4:
          message.chunk = Chunk.decode(reader, reader.uint32());
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): Message {
    return {
      setSettings: isSet(object.setSettings)
        ? SetSettings.fromJSON(object.setSettings)
        : undefined,
      commit: isSet(object.commit) ? Commit.fromJSON(object.commit) : undefined,
      uncommit: isSet(object.uncommit)
        ? Uncommit.fromJSON(object.uncommit)
        : undefined,
      chunk: isSet(object.chunk) ? Chunk.fromJSON(object.chunk) : undefined,
    };
  },

  toJSON(message: Message): unknown {
    const obj: any = {};
    message.setSettings !== undefined &&
      (obj.setSettings = message.setSettings
        ? SetSettings.toJSON(message.setSettings)
        : undefined);
    message.commit !== undefined &&
      (obj.commit = message.commit ? Commit.toJSON(message.commit) : undefined);
    message.uncommit !== undefined &&
      (obj.uncommit = message.uncommit
        ? Uncommit.toJSON(message.uncommit)
        : undefined);
    message.chunk !== undefined &&
      (obj.chunk = message.chunk ? Chunk.toJSON(message.chunk) : undefined);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<Message>, I>>(object: I): Message {
    const message = createBaseMessage();
    message.setSettings =
      object.setSettings !== undefined && object.setSettings !== null
        ? SetSettings.fromPartial(object.setSettings)
        : undefined;
    message.commit =
      object.commit !== undefined && object.commit !== null
        ? Commit.fromPartial(object.commit)
        : undefined;
    message.uncommit =
      object.uncommit !== undefined && object.uncommit !== null
        ? Uncommit.fromPartial(object.uncommit)
        : undefined;
    message.chunk =
      object.chunk !== undefined && object.chunk !== null
        ? Chunk.fromPartial(object.chunk)
        : undefined;
    return message;
  },
};

function createBaseNegotiatedState(): NegotiatedState {
  return { nonce: new Uint8Array(), saveData: new Uint8Array() };
}

export const NegotiatedState = {
  encode(
    message: NegotiatedState,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.nonce.length !== 0) {
      writer.uint32(10).bytes(message.nonce);
    }
    if (message.saveData.length !== 0) {
      writer.uint32(18).bytes(message.saveData);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): NegotiatedState {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseNegotiatedState();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.nonce = reader.bytes();
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

  fromJSON(object: any): NegotiatedState {
    return {
      nonce: isSet(object.nonce)
        ? bytesFromBase64(object.nonce)
        : new Uint8Array(),
      saveData: isSet(object.saveData)
        ? bytesFromBase64(object.saveData)
        : new Uint8Array(),
    };
  },

  toJSON(message: NegotiatedState): unknown {
    const obj: any = {};
    message.nonce !== undefined &&
      (obj.nonce = base64FromBytes(
        message.nonce !== undefined ? message.nonce : new Uint8Array()
      ));
    message.saveData !== undefined &&
      (obj.saveData = base64FromBytes(
        message.saveData !== undefined ? message.saveData : new Uint8Array()
      ));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<NegotiatedState>, I>>(
    object: I
  ): NegotiatedState {
    const message = createBaseNegotiatedState();
    message.nonce = object.nonce ?? new Uint8Array();
    message.saveData = object.saveData ?? new Uint8Array();
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

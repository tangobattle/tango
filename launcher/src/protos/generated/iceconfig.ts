/* eslint-disable */
import Long from "long";
import * as _m0 from "protobufjs/minimal";

export const protobufPackage = "tango.iceconfig";

export interface GetRequest {}

export interface GetResponse {
  iceServers: GetResponse_ICEServer[];
}

export interface GetResponse_ICEServer {
  credential?: string | undefined;
  username?: string | undefined;
  urls: string[];
}

export interface GetLegacyRequest {}

export interface GetLegacyResponse {
  iceServers: string[];
}

function createBaseGetRequest(): GetRequest {
  return {};
}

export const GetRequest = {
  encode(_: GetRequest, writer: _m0.Writer = _m0.Writer.create()): _m0.Writer {
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GetRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetRequest();
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

  fromJSON(_: any): GetRequest {
    return {};
  },

  toJSON(_: GetRequest): unknown {
    const obj: any = {};
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GetRequest>, I>>(_: I): GetRequest {
    const message = createBaseGetRequest();
    return message;
  },
};

function createBaseGetResponse(): GetResponse {
  return { iceServers: [] };
}

export const GetResponse = {
  encode(
    message: GetResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    for (const v of message.iceServers) {
      GetResponse_ICEServer.encode(v!, writer.uint32(10).fork()).ldelim();
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GetResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.iceServers.push(
            GetResponse_ICEServer.decode(reader, reader.uint32())
          );
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): GetResponse {
    return {
      iceServers: Array.isArray(object?.iceServers)
        ? object.iceServers.map((e: any) => GetResponse_ICEServer.fromJSON(e))
        : [],
    };
  },

  toJSON(message: GetResponse): unknown {
    const obj: any = {};
    if (message.iceServers) {
      obj.iceServers = message.iceServers.map((e) =>
        e ? GetResponse_ICEServer.toJSON(e) : undefined
      );
    } else {
      obj.iceServers = [];
    }
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GetResponse>, I>>(
    object: I
  ): GetResponse {
    const message = createBaseGetResponse();
    message.iceServers =
      object.iceServers?.map((e) => GetResponse_ICEServer.fromPartial(e)) || [];
    return message;
  },
};

function createBaseGetResponse_ICEServer(): GetResponse_ICEServer {
  return { credential: undefined, username: undefined, urls: [] };
}

export const GetResponse_ICEServer = {
  encode(
    message: GetResponse_ICEServer,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    if (message.credential !== undefined) {
      writer.uint32(10).string(message.credential);
    }
    if (message.username !== undefined) {
      writer.uint32(18).string(message.username);
    }
    for (const v of message.urls) {
      writer.uint32(26).string(v!);
    }
    return writer;
  },

  decode(
    input: _m0.Reader | Uint8Array,
    length?: number
  ): GetResponse_ICEServer {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetResponse_ICEServer();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.credential = reader.string();
          break;
        case 2:
          message.username = reader.string();
          break;
        case 3:
          message.urls.push(reader.string());
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): GetResponse_ICEServer {
    return {
      credential: isSet(object.credential)
        ? String(object.credential)
        : undefined,
      username: isSet(object.username) ? String(object.username) : undefined,
      urls: Array.isArray(object?.urls)
        ? object.urls.map((e: any) => String(e))
        : [],
    };
  },

  toJSON(message: GetResponse_ICEServer): unknown {
    const obj: any = {};
    message.credential !== undefined && (obj.credential = message.credential);
    message.username !== undefined && (obj.username = message.username);
    if (message.urls) {
      obj.urls = message.urls.map((e) => e);
    } else {
      obj.urls = [];
    }
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GetResponse_ICEServer>, I>>(
    object: I
  ): GetResponse_ICEServer {
    const message = createBaseGetResponse_ICEServer();
    message.credential = object.credential ?? undefined;
    message.username = object.username ?? undefined;
    message.urls = object.urls?.map((e) => e) || [];
    return message;
  },
};

function createBaseGetLegacyRequest(): GetLegacyRequest {
  return {};
}

export const GetLegacyRequest = {
  encode(
    _: GetLegacyRequest,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GetLegacyRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetLegacyRequest();
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

  fromJSON(_: any): GetLegacyRequest {
    return {};
  },

  toJSON(_: GetLegacyRequest): unknown {
    const obj: any = {};
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GetLegacyRequest>, I>>(
    _: I
  ): GetLegacyRequest {
    const message = createBaseGetLegacyRequest();
    return message;
  },
};

function createBaseGetLegacyResponse(): GetLegacyResponse {
  return { iceServers: [] };
}

export const GetLegacyResponse = {
  encode(
    message: GetLegacyResponse,
    writer: _m0.Writer = _m0.Writer.create()
  ): _m0.Writer {
    for (const v of message.iceServers) {
      writer.uint32(10).string(v!);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GetLegacyResponse {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetLegacyResponse();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.iceServers.push(reader.string());
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): GetLegacyResponse {
    return {
      iceServers: Array.isArray(object?.iceServers)
        ? object.iceServers.map((e: any) => String(e))
        : [],
    };
  },

  toJSON(message: GetLegacyResponse): unknown {
    const obj: any = {};
    if (message.iceServers) {
      obj.iceServers = message.iceServers.map((e) => e);
    } else {
      obj.iceServers = [];
    }
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GetLegacyResponse>, I>>(
    object: I
  ): GetLegacyResponse {
    const message = createBaseGetLegacyResponse();
    message.iceServers = object.iceServers?.map((e) => e) || [];
    return message;
  },
};

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

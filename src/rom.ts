import * as crc32 from "crc-32";

export interface ROMInfo {
  title: string;
  crc32: number;
}

const decoder = new TextDecoder("ascii");

export function getROMInfo(buffer: ArrayBuffer) {
  const title = decoder.decode(new Uint8Array(buffer, 0x000000a0, 12));
  return { title, crc32: crc32.buf(new Uint8Array(buffer)) >>> 0 };
}

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

export interface KnownROM {
  crc32: number;
  netplayCompatiblity: string;
}

export const KNOWN_ROMS: { [title: string]: KnownROM } = {
  MEGAMAN6_FXX: { crc32: 0xdee6f2a9, netplayCompatiblity: "bn6f" },
  MEGAMAN6_GXX: { crc32: 0x79452182, netplayCompatiblity: "bn6g" },
  ROCKEXE6_RXX: { crc32: 0x2dfb603e, netplayCompatiblity: "exe6f" },
  ROCKEXE6_GXX: { crc32: 0x6285918a, netplayCompatiblity: "exe6g" },
};

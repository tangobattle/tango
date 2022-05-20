import * as crc32 from "crc-32";
import { readFile } from "fs/promises";

import _roms from "./roms.json";

export const KNOWN_ROMS = _roms as { [name: string]: KnownROM };

export interface ROMInfo {
  name: string;
  crc32: number;
}

const decoder = new TextDecoder("ascii");

export function getROMInfo(buffer: ArrayBuffer) {
  const name = decoder.decode(new Uint8Array(buffer, 0x000000a0, 16));
  return { name, crc32: crc32.buf(new Uint8Array(buffer)) >>> 0 };
}

export interface KnownROM {
  title: { [language: string]: string };
  crc32: number;
  netplayCompatibility: string;
}

export async function getROMName(path: string): Promise<string | null> {
  const romInfo = getROMInfo((await readFile(path)).buffer);
  const knownROM = KNOWN_ROMS[romInfo.name];
  if (knownROM == null) {
    return null;
  }
  if (romInfo.crc32 != knownROM.crc32) {
    throw `mismatched crc32: expected ${knownROM.crc32
      .toString(16)
      .padStart(8, "0")}, got ${romInfo.crc32.toString(16).padStart(8, "0")}`;
  }
  return romInfo.name;
}

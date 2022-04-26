import * as crc32 from "crc-32";
import { readdir, readFile } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";

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

export async function scan(dir: string) {
  const games = {} as { [name: string]: string };

  let romNames: string[];
  try {
    romNames = await readdir(dir);
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      await mkdirp(dir);
      romNames = [];
    } else {
      throw e;
    }
  }

  for (const result of await Promise.allSettled(
    romNames.map(async (romName) => {
      try {
        const romInfo = getROMInfo(
          (await readFile(path.join(dir, romName))).buffer
        );
        const knownROM = KNOWN_ROMS[romInfo.name];
        if (knownROM == null) {
          throw `unknown rom name: ${romInfo.name}`;
        }
        if (romInfo.crc32 != knownROM.crc32) {
          throw `mismatched crc32: expected ${knownROM.crc32
            .toString(16)
            .padStart(8, "0")}, got ${romInfo.crc32
            .toString(16)
            .padStart(8, "0")}`;
        }

        games[romInfo.name] = romName;
      } catch (e) {
        throw `failed to scan rom ${romName}: ${e}`;
      }
    })
  )) {
    if (result.status == "rejected") {
      console.warn("rom skipped:", result.reason);
    }
  }
  return games;
}

import * as crc32 from "crc-32";
import path from "path";
import { readdir, readFile } from "fs/promises";
import _roms from "./roms.json";

const roms = _roms as { [title: string]: KnownROM };

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

export async function scan(dir: string) {
  const games = {} as { [filename: string]: string };
  const promises = [];
  for (const f of await readdir(dir)) {
    promises.push(
      (async (f) => {
        try {
          const romInfo = getROMInfo(
            (await readFile(path.join(dir, f))).buffer
          );
          const knownROM = roms[romInfo.title];
          if (knownROM == null) {
            throw `unknown rom title: ${romInfo.title}`;
          }
          if (romInfo.crc32 != knownROM.crc32) {
            throw `mismatched crc32: expected ${knownROM.crc32
              .toString(16)
              .padStart(8, "0")}, got ${romInfo.crc32
              .toString(16)
              .padStart(8, "0")}`;
          }

          games[f] = romInfo.title;
        } catch (e) {
          throw `failed to scan ${f}: ${e}`;
        }
      })(f)
    );
  }
  for (const result of await Promise.allSettled(promises)) {
    if (result.status == "rejected") {
      console.warn("rom skipped:", result.reason);
    }
  }
  return games;
}

import * as crc32 from "crc-32";
import { readFile } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";

import { walk } from "./fsutil";

export const KNOWN_ROM_FAMILIES = require("./roms.json5").default as {
  [family: string]: {
    lang: string;
    title: { [language: string]: string };
    versions: { [name: string]: KnownROM };
  };
};

export const FAMILY_BY_ROM_NAME = (() => {
  const FAMILY_BY_ROM_NAME: { [romName: string]: string } = {};
  for (const family of Object.keys(KNOWN_ROM_FAMILIES)) {
    for (const version of Object.keys(KNOWN_ROM_FAMILIES[family].versions)) {
      FAMILY_BY_ROM_NAME[version] = family;
    }
  }
  return FAMILY_BY_ROM_NAME;
})();

export interface ROMInfo {
  name: string;
  revision: number;
}

const decoder = new TextDecoder("ascii");

export function getROMInfo(buffer: ArrayBuffer): ROMInfo {
  const dv = new DataView(buffer);
  const name = decoder.decode(new Uint8Array(buffer, 0x000000a0, 16));
  return {
    name,
    revision: dv.getUint8(0x000000bc),
  };
}

export interface KnownROM {
  title: { [language: string]: string } | null;
  order: number;
  revisions: {
    [key: number]: {
      crc32: number;
      saveedit?: any;
    };
  };
}

export async function scan(dir: string) {
  const games = {} as {
    [name: string]: {
      filename: string;
      revision: number;
    };
  };

  const filenames: string[] = [];
  try {
    for await (const fn of walk(dir)) {
      filenames.push(fn);
    }
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      await mkdirp(dir);
    } else {
      throw e;
    }
  }

  for (const result of await Promise.allSettled(
    filenames.map(async (filename) => {
      try {
        const romBuffer = (await readFile(path.join(dir, filename))).buffer;
        const romInfo = getROMInfo(romBuffer);
        const crc32Checksum = crc32.buf(new Uint8Array(romBuffer)) >>> 0;
        if (
          !Object.prototype.hasOwnProperty.call(
            FAMILY_BY_ROM_NAME,
            romInfo.name
          )
        ) {
          throw `unknown rom name: ${romInfo.name}`;
        }

        const familyName = FAMILY_BY_ROM_NAME[romInfo.name];
        const family = KNOWN_ROM_FAMILIES[familyName];
        const rom = family.versions[romInfo.name];

        if (crc32Checksum != rom.revisions[romInfo.revision].crc32) {
          throw `mismatched crc32: expected ${rom.revisions[
            romInfo.revision
          ].crc32
            .toString(16)
            .padStart(8, "0")}, got ${crc32Checksum
            .toString(16)
            .padStart(8, "0")}`;
        }

        games[romInfo.name] = {
          filename,
          revision: romInfo.revision,
        };
      } catch (e) {
        throw `failed to scan rom ${filename}: ${e}`;
      }
    })
  )) {
    if (result.status == "rejected") {
      console.warn("rom skipped:", result.reason);
    }
  }
  return games;
}

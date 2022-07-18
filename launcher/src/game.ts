import { readFile } from "fs/promises";

import applyBPS from "./bps";

export async function makeROM(romPath: string, patchPath: string | null) {
  let rom = new Uint8Array(await readFile(romPath));
  if (patchPath != null) {
    rom = applyBPS(rom, new Uint8Array(await readFile(patchPath)));
  }
  return rom.buffer;
}

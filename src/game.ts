import { readFile, writeFile } from "fs/promises";

import applyBPS from "./bps";

export async function makeROM(
  romPath: string,
  patchPath: string | null,
  outPath: string
) {
  let rom = new Uint8Array(await readFile(romPath));
  if (patchPath != null) {
    rom = applyBPS(rom, new Uint8Array(await readFile(patchPath)));
  }
  await writeFile(outPath, rom);
}

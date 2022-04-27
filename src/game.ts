import { readFile, writeFile } from "fs/promises";
import tmp from "tmp-promise";

import applyBPS from "./bps";

export async function makeROM(romPath: string, patchPath: string | null) {
  let rom = new Uint8Array(await readFile(romPath));
  if (patchPath != null) {
    rom = applyBPS(rom, new Uint8Array(await readFile(patchPath)));
  }

  const romFile = await tmp.file();
  await writeFile(romFile.path, rom);
  return romFile;
}

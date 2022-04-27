import { readFile, writeFile } from "fs/promises";
import tmp from "tmp-promise";

import applyBPS from "./bps";

export async function makeROM(romPath: string, patchPath: string | null) {
  let rom = new Uint8Array(await readFile(romPath));
  if (patchPath != null) {
    rom = applyBPS(rom, new Uint8Array(await readFile(patchPath)));
  }

  const romFile = await tmp.file();
  // eslint-disable-next-line no-console
  console.info("writing temporary ROM to %s", romFile.path);
  try {
    await writeFile(romFile.path, rom);
    return romFile;
  } catch (e) {
    await romFile.cleanup();
    throw e;
  }
}

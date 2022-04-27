import { readFile, writeFile } from "fs/promises";
import path from "path";
import tmp from "tmp-promise";

import patchers from "./patch";

export async function makeROM(romPath: string, patchPath: string | null) {
  let rom = new Uint8Array(await readFile(romPath));
  if (patchPath != null) {
    rom = patchers[path.extname(patchPath).slice(1) as "ips" | "bps"](
      rom,
      new Uint8Array(await readFile(patchPath))
    );
  }

  const romFile = await tmp.file();
  await writeFile(romFile.path, rom);
  return romFile;
}

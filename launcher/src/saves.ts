import { readdir, readFile } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";

import * as bn6 from "./saveedit/bn6";

export interface SaveInfo {
  loader: string;
  romName: string;
}

export async function scan(dir: string) {
  const saves = {} as { [fn: string]: SaveInfo };

  let saveNames: string[];
  try {
    saveNames = await readdir(dir);
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      await mkdirp(dir);
      saveNames = [];
    } else {
      throw e;
    }
  }

  for (const result of await Promise.allSettled(
    saveNames.map(async (saveName) => {
      try {
        const editor = bn6.Editor.fromUnmaskedSRAM(
          bn6.Editor.sramDumpToRaw(
            (await readFile(path.join(dir, saveName))).buffer
          )
        );
        saves[saveName] = {
          loader: "bn6",
          romName: editor.getROMName(),
        };
      } catch (e) {
        throw `failed to scan save ${saveName}: ${e}`;
      }
    })
  )) {
    if (result.status == "rejected") {
      console.warn("save skipped:", result.reason);
    }
  }
  return saves;
}

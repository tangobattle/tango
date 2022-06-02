import { readdir, readFile } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";

import { sniff } from "./saveedit";

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
      const sniffed = sniff((await readFile(path.join(dir, saveName))).buffer);
      if (sniffed == null) {
        console.warn("could not sniff save", saveName);
        return;
      }
      saves[saveName] = {
        loader: sniffed.loader,
        romName: sniffed.editor.getROMName(),
      };
    })
  )) {
    if (result.status == "rejected") {
      console.warn("save skipped:", result.reason);
    }
  }
  return saves;
}

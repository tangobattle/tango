import { readdir, readFile } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";

import { Editor, sniff } from "./saveedit";

export interface SaveInfo {
  gameFamily: string;
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
      let editor: Editor;
      try {
        editor = sniff((await readFile(path.join(dir, saveName))).buffer);
      } catch (e) {
        console.warn("failed to sniff", saveName, e);
        return;
      }
      saves[saveName] = {
        gameFamily: editor.getGameFamily(),
        romName: editor.getROMName(),
      };
    })
  )) {
    if (result.status == "rejected") {
      console.warn("save skipped:", result.reason);
    }
  }
  return saves;
}

import { readFile } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";

import { walk } from "./fsutil";
import { sniff } from "./saveedit";

export async function scan(dir: string) {
  const saves = {} as { [fn: string]: string[] };

  const saveNames: string[] = [];
  try {
    for await (const fn of walk(dir)) {
      saveNames.push(fn);
    }
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      await mkdirp(dir);
    } else {
      throw e;
    }
  }

  for (const result of await Promise.allSettled(
    saveNames.map(async (saveName) => {
      let sniffed: string[];
      try {
        sniffed = sniff((await readFile(path.join(dir, saveName))).buffer);
      } catch (e) {
        console.warn("failed to sniff", saveName, e);
        return;
      }
      saves[saveName] = sniffed;
    })
  )) {
    if (result.status == "rejected") {
      console.warn("save skipped:", result.reason);
    }
  }
  return saves;
}

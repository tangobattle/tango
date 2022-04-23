import { readdir, readFile } from "fs/promises";
import path from "path";
import BN6Editor from "./saveedit/bn6";

export interface SaveInfo {
  loader: string;
  romName: string;
}

export async function scan(dir: string) {
  const saves = {} as { [fn: string]: SaveInfo };
  const promises = [];
  for (const f of await readdir(dir)) {
    promises.push(
      (async (f) => {
        try {
          const editor = new BN6Editor(
            BN6Editor.sramDumpToRaw((await readFile(path.join(dir, f))).buffer)
          );
          saves[f] = {
            loader: "bn6",
            romName: editor.getGameInfo().romName,
          };
        } catch (e) {
          throw `failed to scan ${f}: ${e}`;
        }
      })(f)
    );
  }
  for (const result of await Promise.allSettled(promises)) {
    if (result.status == "rejected") {
      console.warn("rom skipped:", result.reason);
    }
  }
  return saves;
}

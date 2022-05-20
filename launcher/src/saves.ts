import { readFile } from "fs/promises";

import * as bn6 from "./saveedit/bn6";

export interface SaveInfo {
  loader: string;
  romName: string;
}

export async function getSaveInfo(path: string): Promise<SaveInfo> {
  const editor = bn6.Editor.fromUnmaskedSRAM(
    bn6.Editor.sramDumpToRaw((await readFile(path)).buffer)
  );
  return {
    loader: "bn6",
    romName: editor.getROMName(),
  };
}

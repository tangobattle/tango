import * as bn4 from "./bn4";
import * as bn6 from "./bn6";

export interface Sniffed {
  editor: Editor;
  loader: string;
}

export interface Editor {
  getROMName(): string;
}

export function sniff(buffer: ArrayBuffer): Sniffed | null {
  try {
    return {
      editor: bn6.Editor.fromUnmaskedSRAM(bn6.Editor.sramDumpToRaw(buffer)),
      loader: "bn6",
    };
  } catch (e) {
    void e;
  }
  try {
    return {
      editor: bn4.Editor.fromUnmaskedSRAM(bn4.Editor.sramDumpToRaw(buffer)),
      loader: "bn4",
    };
  } catch (e) {
    void e;
  }
  return null;
}

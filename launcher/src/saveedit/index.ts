import * as bn4 from "./bn4";
import * as bn6 from "./bn6";

export interface GameInfo {
  region: "US" | "JP" | "PL";
  version: string | null;
}

export interface Chip {
  index?: number;
  name: {
    [lang: string]: string;
  };
  description?: {
    [lang: string]: string;
  };
  element?: string;
  codes?: string;
  version?: string | null;
  damage?: number;
  mb?: number;
  class?: string;
}

export interface NavicustProgram {
  name: {
    [lang: string]: string;
  };
  colors: string[];
  isSolid: boolean;
  squares: number[];
}

export interface Editor {
  getROMName(): string;
  getGameFamily(): string;
  getGameInfo(): GameInfo;
  getFolderEditor(): FolderEditor | null;
  getNavicustEditor(): NavicustEditor | null;
  rebuild(): void;
}

export interface FolderEditor {
  getChipData(): (Chip | null)[];
  getEquippedFolder(): number;
  getChip(
    folderIdx: number,
    chipIdx: number
  ): {
    id: number;
    code: string;
  } | null;
  isRegularChipInPlace(): boolean;
  getRegularChipIndex(folderIdx: number): number | null;
  getTagChip1Index(folderIdx: number): number | null;
  getTagChip2Index(folderIdx: number): number | null;
}

export interface NavicustEditor {
  getNavicustProgramData(): (NavicustProgram | null)[];
  getNavicustBlock(i: number): {
    id: number;
    variant: number;
    col: number;
    row: number;
    rot: number;
    compressed: boolean;
  } | null;
}

export function sniff(buffer: ArrayBuffer): Editor | null {
  try {
    return bn6.Editor.fromUnmaskedSRAM(bn6.Editor.sramDumpToRaw(buffer));
  } catch (e) {
    void e;
  }
  try {
    return bn4.Editor.fromUnmaskedSRAM(bn4.Editor.sramDumpToRaw(buffer));
  } catch (e) {
    void e;
  }
  return null;
}

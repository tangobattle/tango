import * as bn3 from "./bn3";
import * as bn4 from "./bn4";
import * as exe45 from "./exe45";
import * as bn5 from "./bn5";
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

export interface Modcard {
  name: {
    [lang: string]: string;
  };
  mb: number;
  parameters: {
    name: {
      [lang: string]: string;
    };
    version: string | null;
    debuff: boolean;
  }[];
  abilities: {
    name: {
      [lang: string]: string;
    };
    version: string | null;
    debuff: boolean;
  }[];
}

export interface EditorClass {
  new (buffer: ArrayBuffer, romName: string, verifyChecksum: boolean): Editor;
  sramDumpToRaw(buffer: ArrayBuffer): ArrayBuffer;
  sniff(buffer: ArrayBuffer): string[];
}

export interface Editor {
  getROMName(): string;
  getFolderEditor(): FolderEditor | null;
  getNavicustEditor(): NavicustEditor | null;
  getModcardsEditor(): ModcardsEditor | null;
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
  getCommandLine(): number;
  hasOutOfBounds(): boolean;
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

export interface ModcardsEditor {
  getModcardData(): (Modcard | null)[];
  getModcardCount(): number;
  setModcardCount(n: number): void;
  getModcard(i: number): { id: number; enabled: boolean } | null;
}

const EDITORS: { [key: string]: EditorClass } = {
  bn3: bn3.Editor,
  bn4: bn4.Editor,
  exe45: exe45.Editor,
  bn5: bn5.Editor,
  bn6: bn6.Editor,
};

export function editorClassForGameFamily(family: string): EditorClass {
  switch (family) {
    case "bn3":
    case "exe3":
      return bn3.Editor;
    case "bn4":
    case "exe4":
      return bn4.Editor;
    case "exe45":
        return exe45.Editor;
    case "bn5":
    case "exe5":
      return bn5.Editor;
    case "bn6":
    case "exe6":
      return bn6.Editor;
  }
  throw `no editor class found: ${family}`;
}

export function sniff(buffer: ArrayBuffer): string[] {
  const errors: { [key: string]: any } = {};
  for (const k of Object.keys(EDITORS)) {
    const Editor = EDITORS[k];
    try {
      return Editor.sniff(Editor.sramDumpToRaw(buffer));
    } catch (e) {
      errors[k] = e;
    }
  }
  throw errors;
}

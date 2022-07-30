import { ROMInfo } from "../rom";

import type {
  FolderEditor,
  NavicustEditor,
  ModcardsEditor,
  BN4ModcardsEditor,
} from "./";

export abstract class EditorBase {
  abstract getROMInfo(): ROMInfo;

  getFolderEditor(): FolderEditor | null {
    return null;
  }

  getNavicustEditor(): NavicustEditor | null {
    return null;
  }

  getModcardsEditor(): ModcardsEditor | null {
    return null;
  }

  getBN4ModcardsEditor(): BN4ModcardsEditor | null {
    return null;
  }

  abstract rebuild(): void;
}

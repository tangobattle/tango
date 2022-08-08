import { ROMInfo } from "../rom";

import type {
  NaviEditor,
  FolderEditor,
  NavicustEditor,
  ModcardsEditor,
  BN4ModcardsEditor,
  DarkAIEditor,
} from "./";

export abstract class EditorBase {
  abstract getROMInfo(): ROMInfo;

  getNaviEditor(): NaviEditor | null {
    return null;
  }

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

  getDarkAIEditor(): DarkAIEditor | null {
    return null;
  }

  abstract rebuild(): void;
}

import { Editor } from "./";

interface Restrictions {
  megas: number;
  gigas: number;
  regularMemory: number;
}

const DEFAULT_FOLDER_RESTRICTIONS: Restrictions = {
  megas: 5,
  gigas: 1,
  regularMemory: 0,
};

function getFolderRestrictions(editor: Editor) {
  const restrictions = DEFAULT_FOLDER_RESTRICTIONS;
  restrictions.regularMemory = editor.getRegMemory();
  return restrictions;
}

const TAG_MEMORY = 60;
const MODCARD_MEMORY = 80;

function getMaxChipCopiesAllowed(mb: number) {
  // prettier-ignore
  return mb < 20 ? 5 :
         mb < 30 ? 4 :
         mb < 40 ? 3 :
         mb < 50 ? 2 :
                   1;
}

export type Problem =
  | { kind: "too-much-hp" }
  | { kind: "too-much-regular-memory" }
  | { kind: "too-many-megas" }
  | { kind: "too-many-gigas" }
  | { kind: "not-enough-chips" }
  | { kind: "chip-copies-exceeded"; id: number }
  | { kind: "overlapping-ncp"; i: number; j: number }
  | { kind: "regular-memory-exceeded" }
  | { kind: "tag-memory-exceeded" }
  | { kind: "missing-second-tag-chip" }
  | { kind: "modcard-memory-exceeded" }
  | { kind: "duplicate-modcard"; i: number; j: number }
  | { kind: "wrong-language-chip"; i: number }
  | { kind: "wrong-version-chip"; i: number };

export function* check(editor: Editor): Generator<Problem> {
  if (editor.getBaseHP() > 1000) {
    yield { kind: "too-much-hp" };
  }

  if (editor.getRegMemory() > 50) {
    yield { kind: "too-much-regular-memory" };
  }

  const folderRestrictions = getFolderRestrictions(editor);
}

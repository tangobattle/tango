import { Editor } from "./";

interface Restrictions {
  megas: number;
  gigas: number;
  regular_memory: number;
}

const DEFAULT_FOLDER_RESTRICTIONS: Restrictions = {
  megas: 5,
  gigas: 1,
  regular_memory: 0,
};

function getRestrictions(editor: Editor) {
  const restrictions = DEFAULT_FOLDER_RESTRICTIONS;
  return restrictions;
}

const TAG_MEMORY = 60;

export type Problem =
  | { kind: "too-much-hp" }
  | { kind: "too-many-megas" }
  | { kind: "too-many-gigas" }
  | { kind: "not-enough-chips" }
  | { kind: "overlapping-ncp"; i: number; j: number }
  | { kind: "regular-memory-exceeded" }
  | { kind: "tag-memory-exceeded" }
  | { kind: "missing-second-tag-chip" }
  | { kind: "modcard-memory-exceeded" }
  | { kind: "wrong-language-chip"; i: number }
  | { kind: "wrong-version-chip"; i: number };

export function check(editor: Editor) {
  const problems: Problem[] = [];
  return problems;
}

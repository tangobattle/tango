import { opendir } from "fs/promises";
import path from "path";

export async function* walk(dir: string, root?: string): AsyncIterable<string> {
  if (root == null) {
    root = dir;
  }
  for await (const d of await opendir(dir)) {
    const entry = path.join(dir, d.name);
    if (d.isDirectory()) {
      yield* walk(entry, root);
    } else if (d.isFile()) {
      yield path.relative(root, entry);
    }
  }
}

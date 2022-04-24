import { spawn } from "child_process";
import path from "path";

import { app } from "@electron/remote";

export async function getKeyName() {
  const proc = spawn(path.join(app.getAppPath(), "core", "keymaptool"), {
    stdio: [null, "pipe", null],
  });
  let buf = "";
  for await (const data of proc.stdout) {
    buf += data;
  }
  proc.kill();
  buf = buf.trimEnd();
  return buf != "" ? buf : null;
}

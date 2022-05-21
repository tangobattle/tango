import { app } from "@electron/remote";

import { PhysicalInput } from "./config";
import { spawn } from "./process";

export async function captureInput(lang: string, message: string) {
  const proc = spawn(app, "keymaptool", ["--lang", lang, "--", message]);
  (async () => {
    for await (const data of proc.stderr) {
      for (const line of data.toString().split(/\r?\n/g)) {
        if (line == "") {
          continue;
        }
        // eslint-disable-next-line no-console
        console.info("keymaptool:", line);
      }
    }
  })();

  const buf = [];
  for await (const x of proc.stdout) {
    buf.push(x);
  }

  const s = buf.join("").trimEnd();
  if (s == "") {
    return null;
  }
  return JSON.parse(s) as PhysicalInput;
}

import { ChildProcessWithoutNullStreams } from "child_process";

import { app } from "@electron/remote";

import { spawn } from "./process";

export class Keymaptool {
  private proc: ChildProcessWithoutNullStreams;

  constructor(lang: string, { env }: { env?: NodeJS.ProcessEnv } = {}) {
    this.proc = spawn(app, "keymaptool", ["--lang", lang], { env });
  }

  async request(message: string) {
    if (this.proc.signalCode != null || this.proc.exitCode != null) {
      return null;
    }

    this.proc.stdin.write(message + "\n");
    const it = this.proc.stdout[Symbol.asyncIterator]();

    let buf = "";

    // eslint-disable-next-line no-constant-condition
    while (true) {
      const { value, done } = await it.next();
      if (done) {
        this.close();
        return null;
      }

      buf += value;
      if (buf[buf.length - 1] == "\n") {
        break;
      }
    }

    return buf.trimEnd();
  }

  close() {
    this.proc.stdin.end();
    this.proc.kill();
  }
}

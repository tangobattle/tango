import { ChildProcessWithoutNullStreams, spawn } from "child_process";

import { getBinPath } from "./paths";

export class Keymaptool {
  private proc: ChildProcessWithoutNullStreams;

  constructor({ env }: { env?: NodeJS.ProcessEnv } = {}) {
    this.proc = spawn(getBinPath("keymaptool"), { env });
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

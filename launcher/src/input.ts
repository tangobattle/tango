import { ChildProcessWithoutNullStreams } from "child_process";

import { app } from "@electron/remote";

import { spawn } from "./process";

export class Keymaptool {
  private proc: ChildProcessWithoutNullStreams;

  constructor(
    lang: string,
    target: "keyboard" | "controller",
    { env }: { env?: NodeJS.ProcessEnv } = {}
  ) {
    this.proc = spawn(app, "keymaptool", ["--lang", lang, "--target", target], {
      env,
    });
    (async () => {
      for await (const data of this!.proc.stderr) {
        for (const line of data.toString().split(/\r?\n/g)) {
          if (line == "") {
            continue;
          }
          // eslint-disable-next-line no-console
          console.info("keymaptool:", line);
        }
      }
    })();
    this.proc.addListener("exit", () => {
      // eslint-disable-next-line no-console
      console.info("keymaptool exited", {
        exitCode: this.proc.exitCode,
        signalCode: this.proc.signalCode,
      });
    });
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

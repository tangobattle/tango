import { ChildProcessWithoutNullStreams, spawn } from "child_process";
import * as types from "./types";

export class Core {
  private proc: ChildProcessWithoutNullStreams;
  private exitPromise: Promise<void>;

  constructor(args: types.Args, { signal }: { signal?: AbortSignal } = {}) {
    this.proc = spawn("core/tango-core", [JSON.stringify(args)], {
      signal,
    });
    this.exitPromise = new Promise((resolve) => {
      this.proc.addListener("exit", () => {
        this.proc.kill();
        resolve();
      });
    });
    window.addEventListener("beforeunload", () => {
      this.proc.kill();
    });
  }

  public async *readEventStream() {
    let buf = "";
    for await (const data of this.proc.stdout) {
      buf += data;
      const lines = buf.split(/\n/g);
      buf = lines[lines.length - 1];

      for (const r of lines.slice(0, -1)) {
        yield JSON.parse(r) as types.Notification;
      }
    }
  }

  public async wait() {
    await this.exitPromise;
    return { exitCode: this.proc.exitCode, signalCode: this.proc.signalCode };
  }
}

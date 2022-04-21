import { ChildProcessWithoutNullStreams, spawn } from "child_process";
import * as types from "./types";
import { app } from "@electron/remote";
import path from "path";

export class Core {
  private _proc: ChildProcessWithoutNullStreams;
  private _stderr: string[];
  private _exitPromise: Promise<void>;

  constructor(args: types.Args, { signal }: { signal?: AbortSignal } = {}) {
    const proc = spawn(
      path.join(app.getAppPath(), "core", "tango-core"),
      [JSON.stringify(args)],
      {
        signal,
      }
    );
    this._exitPromise = new Promise((resolve) => {
      proc.addListener("exit", () => {
        proc.kill();
        resolve();
      });
    });
    window.addEventListener("beforeunload", () => {
      proc.kill();
    });

    const stderr = [] as string[];

    (async () => {
      for await (const data of proc.stderr) {
        stderr.push(data);
      }
    })();

    this._proc = proc;
    this._stderr = stderr;
  }

  public async *readEventStream() {
    let buf = "";
    for await (const data of this._proc.stdout) {
      buf += data;
      const lines = buf.split(/\n/g);
      buf = lines[lines.length - 1];

      for (const r of lines.slice(0, -1)) {
        yield JSON.parse(r) as types.Notification;
      }
    }
  }

  public stderr() {
    return this._stderr.join("");
  }

  public async wait() {
    await this._exitPromise;
    return { exitCode: this._proc.exitCode, signalCode: this._proc.signalCode };
  }
}

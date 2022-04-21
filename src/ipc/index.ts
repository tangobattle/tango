import { spawn } from "child_process";
import * as types from "./types";

export function spawnCore(
  args: types.Args,
  { signal }: { signal?: AbortSignal } = {}
) {
  const proc = spawn("core/tango-core", [JSON.stringify(args)], {
    signal,
  });
  proc.addListener("exit", () => {
    proc.kill();
  });
  window.addEventListener("beforeunload", () => {
    proc.kill();
  });
  return proc;
}

import { spawn } from "child_process";

export interface Keymapping {
  up: string;
  down: string;
  left: string;
  right: string;
  a: string;
  b: string;
  l: string;
  r: string;
  select: string;
  start: string;
}

export interface CoreOptions {
  romPath: string;
  savePath: string;
  sessionID: string;
  matchType: number;
  replayPrefix: string;
  matchmakingConnectAddr: string;
  iceServers: string[];
  inputDelay: number;
  keymapping: Keymapping;
}

export function spawnCore(opts: CoreOptions) {
  const proc = spawn("core/tango-core", [
    "--rom-path",
    opts.romPath,

    "--save-path",
    opts.savePath,

    "--session-id",
    opts.sessionID,

    "--match-type",
    opts.matchType.toString(),

    "--replay-prefix",
    opts.replayPrefix,

    "--matchmaking-connect-addr",
    opts.matchmakingConnectAddr,

    ...opts.iceServers.flatMap((url) => ["--ice-servers", url]),

    "--input-delay",
    opts.inputDelay.toString(),

    "--keymapping",
    JSON.stringify(opts.keymapping),
  ]);
  proc.addListener("exit", () => {
    proc.kill();
  });
  window.addEventListener("beforeunload", (event) => {
    proc.kill();
  });
  return proc;
}

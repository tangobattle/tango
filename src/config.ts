import { readFileSync, writeFileSync } from "fs";
import { readFile, writeFile } from "fs/promises";

import * as ipc from "./ipc";

export interface Config {
  updateChannel: string;
  wgpuBackend: string | null;
  keymapping: ipc.Args["keymapping"];
  matchmakingConnectAddr: string;
  iceServers: string[];
}

export const DEFAULT: Config = {
  updateChannel: "alpha",
  wgpuBackend: null,
  keymapping: {
    up: "Up",
    down: "Down",
    left: "Left",
    right: "Right",
    a: "Z",
    b: "X",
    l: "A",
    r: "S",
    select: "Back",
    start: "Return",
  },
  matchmakingConnectAddr: "wss://mm.tango.murk.land",
  iceServers: [
    "stun://stun.l.google.com:19302",
    "stun://stun1.l.google.com:19302",
    "stun://stun2.l.google.com:19302",
    "stun://stun3.l.google.com:19302",
    "stun://stun4.l.google.com:19302",
  ],
};

export function ensureSync(path: string) {
  let data;
  try {
    data = readFileSync(path);
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      writeFileSync(path, JSON.stringify(DEFAULT, null, 4) + "\n");
      return DEFAULT;
    }
    throw e;
  }
  try {
    return { ...DEFAULT, ...JSON.parse(data.toString()) } as Config;
  } catch {
    return DEFAULT;
  }
}

export async function load(path: string) {
  const data = await readFile(path);
  return JSON.parse(data.toString()) as Config;
}

export async function save(config: Config, path: string) {
  await writeFile(path, JSON.stringify(config, null, 4) + "\n");
}

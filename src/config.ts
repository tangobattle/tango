import { readFile, writeFile } from "fs/promises";
import mkdirp from "mkdirp";

import * as ipc from "./ipc";
import { getBasePath } from "./paths";

export interface Config {
  wgpuBackend: string | null;
  keymapping: ipc.Args["keymapping"];
  matchmakingConnectAddr: string;
  iceServers: string[];
}

export const DEFAULT: Config = {
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

export async function load(path: string) {
  let data;
  const p = path;
  try {
    data = await readFile(p);
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      await mkdirp(getBasePath());
      return DEFAULT;
    }
    throw e;
  }
  const str = data.toString();
  try {
    return { ...DEFAULT, ...JSON.parse(str) } as Config;
  } catch {
    return DEFAULT;
  }
}

export async function save(config: Config, path: string) {
  await writeFile(path, JSON.stringify(config, null, 4) + "\n");
}

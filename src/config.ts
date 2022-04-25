import { readFile, writeFile } from "fs/promises";
import path from "path";

import { app } from "@electron/remote";

import * as ipc from "./ipc";

export interface Config {
  keymapping: ipc.Args["keymapping"];
  matchmakingConnectAddr: string;
  iceServers: string[];
}

export const DEFAULT: Config = {
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

export async function load(filename: string) {
  let data;
  const p = path.join(app.getAppPath(), filename);
  try {
    data = await readFile(p);
  } catch (e) {
    if ((e as any).code == "ENOENT") {
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

export async function save(config: Config, filename: string) {
  await writeFile(
    path.join(app.getAppPath(), filename),
    JSON.stringify(config, null, 4) + "\n"
  );
}

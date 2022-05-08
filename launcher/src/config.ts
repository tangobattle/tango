import { readFileSync, writeFileSync } from "fs";
import { readFile, writeFile } from "fs/promises";

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

export interface Config {
  nickname: string;
  theme: "dark" | "light";
  language: string | null;
  updateChannel: string;
  wgpuBackend: string | null;
  rustLogFilter: string;
  keymapping: Keymapping;
  signalingConnectAddr: string;
  iceServers: string[];
}

export const DEFAULT: Config = {
  nickname: "Player T",
  theme: "light",
  language: null,
  updateChannel: "latest",
  wgpuBackend: null,
  rustLogFilter: "",
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
  signalingConnectAddr: "ws://mm.tangobattle.com/",
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
    writeFileSync(path, JSON.stringify(DEFAULT, null, 4) + "\n");
    return DEFAULT;
  }
}

export async function load(path: string) {
  const data = await readFile(path);
  return { ...DEFAULT, ...JSON.parse(data.toString()) } as Config;
}

export async function save(config: Config, path: string) {
  await writeFile(path, JSON.stringify(config, null, 4) + "\n");
}

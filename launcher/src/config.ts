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

export interface ControllerMapping {
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
  enableLeftStick: boolean;
}

export interface Config {
  nickname: string | null;
  theme: "dark" | "light";
  language: string | null;
  updateChannel: string;
  rustLogFilter: string;
  controls: {
    keyboard: Keymapping;
    controller: ControllerMapping;
  };
  matchmakingServerAddr: string;
  iceServers: string[];
}

export const DEFAULT: Config = {
  nickname: null,
  theme: "light",
  language: null,
  updateChannel: "latest",
  rustLogFilter: "",
  controls: {
    keyboard: {
      up: "Up",
      down: "Down",
      left: "Left",
      right: "Right",
      a: "Z",
      b: "X",
      l: "A",
      r: "S",
      select: "Backspace",
      start: "Return",
    },
    controller: {
      up: "dpup",
      down: "dpdown",
      left: "dpleft",
      right: "dpright",
      a: "a",
      b: "b",
      l: "leftshoulder",
      r: "rightshoulder",
      select: "back",
      start: "start",
      enableLeftStick: true,
    },
  },
  matchmakingServerAddr: "https://lets.tangobattle.com",
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

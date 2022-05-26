import { readFileSync, writeFileSync } from "fs";
import { readFile, writeFile } from "fs/promises";

export type PhysicalInput =
  | { Key: string }
  | { Button: string }
  | { Axis: [string, number] };

export interface Config {
  nickname: string | null;
  theme: "dark" | "light";
  language: string | null;
  updateChannel: string;
  rustLogFilter: string;
  inputMapping: {
    up: PhysicalInput[];
    down: PhysicalInput[];
    left: PhysicalInput[];
    right: PhysicalInput[];
    a: PhysicalInput[];
    b: PhysicalInput[];
    l: PhysicalInput[];
    r: PhysicalInput[];
    select: PhysicalInput[];
    start: PhysicalInput[];
  };
  signalingEndpoint: string;
  iceconfigEndpoint: string;
  iceServers: string[];
}

export const DEFAULT: Config = {
  nickname: null,
  theme: "light",
  language: null,
  updateChannel: "latest",
  rustLogFilter: "",
  inputMapping: {
    up: [{ Key: "Up" }, { Button: "dpup" }, { Axis: ["lefty", -1] }],
    down: [{ Key: "Down" }, { Button: "dpdown" }, { Axis: ["lefty", 1] }],
    left: [{ Key: "Left" }, { Button: "dpleft" }, { Axis: ["leftx", -1] }],
    right: [{ Key: "Right" }, { Button: "dpright" }, { Axis: ["leftx", 1] }],
    a: [{ Key: "Z" }, { Button: "a" }],
    b: [{ Key: "X" }, { Button: "b" }],
    l: [{ Key: "A" }, { Button: "leftshoulder" }],
    r: [{ Key: "S" }, { Button: "rightshoulder" }],
    select: [{ Key: "Backspace" }, { Button: "back" }],
    start: [{ Key: "Return" }, { Button: "start" }],
  },
  signalingEndpoint: "wss://lets.tangobattle.com/signaling",
  iceconfigEndpoint: "https://lets.tangobattle.com/iceconfig",
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

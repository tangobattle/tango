import { readFileSync, writeFileSync } from "fs";
import { readFile, writeFile } from "fs/promises";
import path from "path";

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
  windowScale: number;
  maxQueueLength: number;
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
    speedUp: PhysicalInput[];
  };
  endpoints: {
    signaling: string;
    iceconfig: string;
    replaycollector: string;
  };
  iceServers: string[];
  patchRepo: string;
  defaultMatchSettings: {
    inputDelay: number;
    matchType: number;
  };
  paths: {
    saves: string;
    roms: string;
    replays: string;
    patches: string;
  };
}

function fillWithDefaults(app: Electron.App, config: Partial<Config>): Config {
  const default_ = defaultConfig(app);
  return {
    ...default_,
    ...config,
    inputMapping: { ...default_.inputMapping, ...config.inputMapping },
    endpoints: { ...default_.endpoints, ...config.endpoints },
    paths: { ...default_.paths, ...config.paths },
  };
}

function defaultConfig(app: Electron.App): Config {
  const basePath = path.join(app.getPath("documents"), "Tango");
  return {
    nickname: null,
    theme: "light",
    language: null,
    updateChannel: "latest",
    rustLogFilter: "",
    windowScale: 3,
    maxQueueLength: 1200,
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
      speedUp: [],
    },
    endpoints: {
      signaling: "wss://signaling.tangobattle.com",
      iceconfig: "https://iceconfig.tangobattle.com",
      replaycollector: "https://replaycollector.tangobattle.com",
    },
    iceServers: [
      "stun://stun.l.google.com:19302",
      "stun://stun1.l.google.com:19302",
      "stun://stun2.l.google.com:19302",
      "stun://stun3.l.google.com:19302",
      "stun://stun4.l.google.com:19302",
    ],
    patchRepo: "https://github.com/tangobattle/patches",
    defaultMatchSettings: {
      inputDelay: 2,
      matchType: 1,
    },
    paths: {
      roms: path.join(basePath, "roms"),
      replays: path.join(basePath, "replays"),
      patches: path.join(basePath, "patches"),
      saves: path.join(basePath, "saves"),
    },
  };
}

export function ensureSync(app: Electron.App, path: string) {
  let data;
  try {
    data = readFileSync(path);
  } catch (e) {
    if ((e as any).code == "ENOENT") {
      const config = fillWithDefaults(app, {});
      writeFileSync(path, JSON.stringify(config, null, 4) + "\n");
      return config;
    }
    throw e;
  }
  try {
    return fillWithDefaults(app, JSON.parse(data.toString()));
  } catch {
    const config = fillWithDefaults(app, {});
    writeFileSync(path, JSON.stringify(config, null, 4) + "\n");
    return config;
  }
}

export async function load(app: Electron.App, path: string) {
  const data = await readFile(path);
  return fillWithDefaults(app, JSON.parse(data.toString()));
}

export async function save(config: Config, path: string) {
  await writeFile(path, JSON.stringify(config, null, 4) + "\n");
}

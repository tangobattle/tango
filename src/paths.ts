import path from "path";

import { app } from "@electron/remote";

export function getConfigPath() {
  return "config.json";
}

export function getROMsPath() {
  return "roms";
}

export function getPatchesPath() {
  return "patches";
}

export function getReplaysPath() {
  return "replays";
}

export function getSavesPath() {
  return "saves";
}

export function getCorePath() {
  return path.join(app.getAppPath(), "core");
}

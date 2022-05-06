import path from "path";

export function getBasePath(app: Electron.App) {
  return path.join(app.getPath("documents"), "Tango");
}

export function getConfigPath(app: Electron.App) {
  return path.join(app.getPath("userData"), "config.json");
}

export function getROMsPath(app: Electron.App) {
  return path.join(getBasePath(app), "roms");
}

export function getPatchesPath(app: Electron.App) {
  return path.join(getBasePath(app), "patches");
}

export function getReplaysPath(app: Electron.App) {
  return path.join(getBasePath(app), "replays");
}

export function getSavesPath(app: Electron.App) {
  return path.join(getBasePath(app), "saves");
}

export function getBinPath(app: Electron.App, exe: string) {
  return path.join(
    app.isPackaged ? path.join(process.resourcesPath, "bin") : "dev-bin",
    exe + (process.platform === "win32" ? ".exe" : "")
  );
}

import path from "path";

export function getBasePath(app: Electron.App) {
  return path.join(app.getPath("documents"), "Tango");
}

export function getConfigPath(app: Electron.App) {
  return path.join(app.getPath("userData"), "config.json");
}

export function getBinPath(app: Electron.App, exe: string) {
  return path.join(
    app.isPackaged ? path.join(process.resourcesPath, "bin") : "dev-bin",
    exe + (process.platform === "win32" ? ".exe" : "")
  );
}

export function getDynamicLibraryPath(app: Electron.App) {
  return app.isPackaged ? path.join(process.resourcesPath, "lib") : "lib";
}

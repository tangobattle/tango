import { BrowserWindow } from "@electron/remote";

export function getMainWindow() {
  return BrowserWindow.getAllWindows()[0];
}

export function requestAttention(app: Electron.App) {
  if (process.platform === "darwin") {
    app.dock.bounce("critical");
  } else {
    getMainWindow().flashFrame(true);
  }
}

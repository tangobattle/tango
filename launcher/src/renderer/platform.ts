import { BrowserWindow } from "@electron/remote";

export function requestAttention(app: Electron.App) {
  if (process.platform === "darwin") {
    app.dock.bounce("critical");
  } else {
    const win = BrowserWindow.getAllWindows()[0];
    win.flashFrame(true);
  }
}

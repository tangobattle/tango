import { getCurrentWindow } from "@electron/remote";

export function requestAttention(app: Electron.App) {
  if (process.platform === "darwin") {
    app.dock.bounce("critical");
  } else {
    const win = getCurrentWindow();
    if (!win.isFocused()) {
      win.flashFrame(true);
    }
  }
}

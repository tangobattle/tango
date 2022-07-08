import { app, BrowserWindow, Menu, shell } from "electron";
import * as log from "electron-log";
import { autoUpdater } from "electron-updater";
import mkdirp from "mkdirp";
import * as path from "path";
import * as url from "url";

import * as remoteMain from "@electron/remote/main";

import * as config from "../config";
import { getBasePath, getConfigPath } from "../paths";

app.commandLine.appendSwitch("in-process-gpu");

mkdirp.sync(getBasePath(app));
const cfg = config.ensureSync(app, getConfigPath(app));

remoteMain.initialize();

Object.assign(console, log.functions);
autoUpdater.logger = console;

if (autoUpdater.channel != "disabled") {
  autoUpdater.channel = cfg.updateChannel;
  autoUpdater.allowDowngrade = false;
  autoUpdater.allowPrerelease = cfg.updateChannel != "latest";
}

autoUpdater.addListener("update-available", () => {
  if (mainWindow != null) {
    mainWindow.webContents.send("update-status", "available");
  }
});

autoUpdater.addListener("update-downloaded", () => {
  if (mainWindow != null) {
    mainWindow.webContents.send("update-status", "downloaded");
  }
});

autoUpdater.addListener("update-not-available", () => {
  if (mainWindow != null) {
    mainWindow.webContents.send("update-status", "not-available");
  }
});

let mainWindow: Electron.BrowserWindow | null;

function createWindow() {
  const template: (Electron.MenuItemConstructorOptions | Electron.MenuItem)[] =
    [];
  if (process.platform === "darwin") {
    const name = app.getName();
    template.unshift(
      {
        label: name,
        submenu: [
          { role: "about" },
          { type: "separator" },
          { role: "services" },
          { type: "separator" },
          { role: "hide" },
          { role: "hideOthers" },
          { role: "unhide" },
          { type: "separator" },
          { role: "reload" },
          { role: "toggleDevTools" },
          { type: "separator" },
          { role: "quit" },
        ],
      },
      {
        label: "Edit",
        submenu: [
          { role: "undo" },
          { role: "redo" },
          { type: "separator" },
          { role: "cut" },
          { role: "copy" },
          { role: "paste" },
          { role: "delete" },
          { type: "separator" },
          { role: "selectAll" },
        ],
      }
    );
  }
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));

  const win = new BrowserWindow({
    width: 800,
    height: 600,
    minWidth: 800,
    minHeight: 600,
    backgroundColor: "#000000",
    title: "Tango",
    fullscreenable: false,
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false,
      devTools: process.env.NODE_ENV !== "production",
    },
  });
  mainWindow = win;

  win.loadURL(
    process.env.NODE_ENV == "development"
      ? "http://localhost:4000/"
      : url.format({
          pathname: path.join(__dirname, "renderer", "index.html"),
          protocol: "file:",
          slashes: true,
        })
  );

  win.on("closed", () => {
    mainWindow = null;
  });

  win.on("focus", () => {
    win.flashFrame(false);
  });

  win.webContents.addListener("new-window", function (e, url) {
    e.preventDefault();
    shell.openExternal(url);
  });

  win.webContents.session.webRequest.onBeforeSendHeaders(
    (details, callback) => {
      callback({ requestHeaders: { Origin: "*", ...details.requestHeaders } });
    }
  );

  win.webContents.session.webRequest.onHeadersReceived((details, callback) => {
    callback({
      responseHeaders: {
        "Access-Control-Allow-Origin": ["*"],
        ...details.responseHeaders,
      },
    });
  });

  remoteMain.enable(win.webContents);

  if (cfg.updateChannel != "disabled") {
    autoUpdater.checkForUpdates();
  } else {
    console.warn("automatic updates are disabled!");
  }
}

// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.on("ready", createWindow);

// Quit when all windows are closed.
app.on("window-all-closed", () => {
  // On OS X it is common for applications and their menu bar
  // to stay active until the user quits explicitly with Cmd + Q
  if (process.platform !== "darwin") {
    app.quit();
  }
});

app.on("activate", () => {
  // On OS X it"s common to re-create a window in the app when the
  // dock icon is clicked and there are no other windows open.
  if (mainWindow === null) {
    createWindow();
  }
});

app.on("web-contents-created", (_event, contents) => {
  contents.on("will-navigate", (event, navigationUrl) => {
    const parsedUrl = new URL(navigationUrl);
    if (parsedUrl.protocol != "mailto:") {
      event.preventDefault();
    }
  });
});

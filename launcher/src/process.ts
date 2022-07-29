import {
    ChildProcessWithoutNullStreams, spawn as origSpawn, SpawnOptionsWithoutStdio
} from "child_process";
import { accessSync, constants } from "fs";

import { getBinPath, getDynamicLibraryPath } from "./paths";

function hasCatchsegv(app: Electron.App) {
  try {
    accessSync(getBinPath(app, "catchsegv"), constants.X_OK);
    return true;
  } catch (_e) {
    return false;
  }
}

export function spawn(
  app: Electron.App,
  command: string,
  args?: ReadonlyArray<string>,
  { env, ...rest }: SpawnOptionsWithoutStdio = {}
): ChildProcessWithoutNullStreams {
  command = getBinPath(app, command);
  const realArgs = (args ?? []).slice();
  if (hasCatchsegv(app)) {
    // eslint-disable-next-line no-console
    console.info("catchsegv available, wrapping process");
    realArgs.unshift("--");
    realArgs.unshift(command);
    command = getBinPath(app, "catchsegv");
  } else {
    // eslint-disable-next-line no-console
    console.info("catchsegv NOT available, will NOT wrap process");
  }
  return origSpawn(command, realArgs, {
    env: { DYLD_FALLBACK_LIBRARY_PATH: getDynamicLibraryPath(app), ...env },
    ...rest,
  });
}

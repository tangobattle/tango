import { Mutex } from "async-mutex";
import { watch } from "chokidar";
import { constants } from "fs";
import { access } from "fs/promises";
import mkdirp from "mkdirp";
import path from "path";
import React, { useContext } from "react";

import { app } from "@electron/remote";

import { getPatchInfo, PatchInfos } from "../../patch";
import { getPatchesPath } from "../../paths";

export interface PatchesValue {
  patches: PatchInfos;
}

const Context = React.createContext(null! as PatchesValue);

export const PatchesProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [currentPatches, setCurrentPatches] = React.useState<PatchInfos>({});
  const dir = getPatchesPath(app);

  React.useEffect(() => {
    const stateMu = new Mutex();

    const remove = async (fn: string) => {
      setCurrentPatches((currentPatches) => {
        currentPatches = { ...currentPatches };
        delete currentPatches[fn];
        return currentPatches;
      });
    };

    const upsert = async (fn: string) => {
      try {
        const patchInfo = await getPatchInfo(path.join(dir, fn));
        if (patchInfo == null) {
          return;
        }
        setCurrentPatches((currentPatches) => ({
          ...currentPatches,
          [fn]: patchInfo,
        }));
      } catch (e) {
        console.error(`failed to scan ${fn}`, e);
        await remove(fn);
      }
    };

    const watcher = watch(dir, { depth: 1 });
    watcher.on("add", (p) => {
      const fn = path.relative(dir, p).split(path.sep)[0];
      (async () => {
        const release = await stateMu.acquire();
        try {
          await upsert(fn);
        } finally {
          release();
        }
      })();
    });
    watcher.on("change", (p) => {
      const fn = path.relative(dir, p).split(path.sep)[0];
      (async () => {
        const release = await stateMu.acquire();
        try {
          await upsert(fn);
        } finally {
          release();
        }
      })();
    });
    watcher.on("unlink", (p) => {
      const fn = path.relative(dir, p).split(path.sep)[0];
      (async () => {
        const release = await stateMu.acquire();
        try {
          try {
            await access(path.join(dir, p), constants.R_OK);
          } catch (e) {
            return;
          }
          await upsert(fn);
        } finally {
          release();
        }
      })();
    });
    watcher.on("unlinkDir", (p) => {
      const fn = path.relative(dir, p).split(path.sep)[0];
      (async () => {
        const release = await stateMu.acquire();
        try {
          await remove(fn);
        } finally {
          release();
        }
      })();
    });
    (async () => {
      await mkdirp(dir);
    })();
    return () => {
      watcher.close();
    };
  }, [dir]);

  return (
    <Context.Provider
      value={{
        patches: currentPatches,
      }}
    >
      {children}
    </Context.Provider>
  );
};

export const PatchesConsumer = Context.Consumer;

export function usePatches() {
  return useContext(Context);
}

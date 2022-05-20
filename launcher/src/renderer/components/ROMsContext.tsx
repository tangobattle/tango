import { Mutex } from "async-mutex";
import { watch } from "chokidar";
import mkdirp from "mkdirp";
import path from "path";
import React, { useContext } from "react";

import { app } from "@electron/remote";

import { getROMsPath } from "../../paths";
import { getROMName } from "../../rom";

export interface ROMsValue {
  roms: { [name: string]: string[] };
  hasErrors: boolean;
}

const Context = React.createContext(null! as ROMsValue);

export const ROMsProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [currentROMs, setCurrentROMs] = React.useState<{
    [name: string]: string[];
  }>({});
  const [hasErrors, setHasErrors] = React.useState(false);

  const dir = getROMsPath(app);

  React.useEffect(() => {
    const stateMu = new Mutex();

    const remove = async (fn: string) => {
      setCurrentROMs((currentROMs) => {
        currentROMs = { ...currentROMs };
        for (const romName of Object.keys(currentROMs)) {
          currentROMs[romName] = currentROMs[romName].filter((n) => n != fn);
          if (currentROMs[romName].length == 0) {
            delete currentROMs[romName];
          }
        }
        return currentROMs;
      });
    };

    const upsert = async (fn: string) => {
      try {
        const romName = await getROMName(path.join(dir, fn));
        if (romName == null) {
          return;
        }
        setCurrentROMs((currentROMs) => ({
          ...currentROMs,
          [romName]: [
            ...(currentROMs[romName] || []).filter((n) => n != fn),
            fn,
          ],
        }));
      } catch (e) {
        console.error(`failed to scan ${fn}`, e);
        setHasErrors(true);
        await remove(fn);
      }
    };

    const watcher = watch(dir);
    watcher.on("add", (p) => {
      const fn = path.relative(dir, p);
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
      const fn = path.relative(dir, p);
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
      const fn = path.relative(dir, p);
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
        roms: currentROMs,
        hasErrors,
      }}
    >
      {children}
    </Context.Provider>
  );
};

export const ROMsConsumer = Context.Consumer;

export function useROMs() {
  return useContext(Context);
}

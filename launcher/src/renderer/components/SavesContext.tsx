import { Mutex } from "async-mutex";
import { watch } from "chokidar";
import mkdirp from "mkdirp";
import path from "path";
import React, { useContext } from "react";

import { app } from "@electron/remote";

import { getSavesPath } from "../../paths";
import { getSaveInfo, SaveInfo } from "../../saves";

export interface SavesValue {
  saves: { [filename: string]: SaveInfo };
  hasErrors: boolean;
}

const Context = React.createContext(null! as SavesValue);

export const SavesProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [currentSaves, setCurrentSaves] = React.useState<{
    [filename: string]: SaveInfo;
  }>({});
  const [hasErrors, setHasErrors] = React.useState(false);

  const dir = getSavesPath(app);

  React.useEffect(() => {
    const stateMu = new Mutex();

    const remove = async (fn: string) => {
      setCurrentSaves((currentSaves) => {
        currentSaves = { ...currentSaves };
        delete currentSaves[fn];
        return currentSaves;
      });
    };

    const upsert = async (fn: string) => {
      try {
        const saveInfo = await getSaveInfo(path.join(dir, fn));
        if (saveInfo == null) {
          return;
        }
        setCurrentSaves((currentSaves) => ({
          ...currentSaves,
          [fn]: saveInfo,
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
        saves: currentSaves,
        hasErrors,
      }}
    >
      {children}
    </Context.Provider>
  );
};

export const SavesConsumer = Context.Consumer;

export function useSaves() {
  return useContext(Context);
}

import React, { useContext } from "react";

import { PatchInfos, scan, update as updatePatches } from "../../patch";
import { useConfig } from "./ConfigContext";

export interface PatchesValue {
  rescan(): Promise<void>;
  update(): Promise<void>;
  updating: boolean;
  patches: PatchInfos;
}

const Context = React.createContext(null! as PatchesValue);

function makeScanPatches(path: string) {
  let status: "pending" | "error" | "ok" = "pending";
  let result: PatchesValue["patches"];
  let err: any;
  const promise = (async () => {
    try {
      result = await scan(path);
    } catch (e) {
      console.error(e);
      err = e;
      status = "error";
    }
    status = "ok";
  })();
  return () => {
    switch (status) {
      case "pending":
        throw promise;
      case "error":
        throw err;
      case "ok":
        return result;
    }
  };
}

let scanPatches: (() => PatchInfos) | null = null;

export const PatchesProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const { config } = useConfig();
  if (scanPatches == null) {
    scanPatches = makeScanPatches(config.paths.patches);
  }
  const [currentPatches, setCurrentPatches] = React.useState(scanPatches());
  const [updating, setUpdating] = React.useState(false);

  const rescan = React.useCallback(async () => {
    try {
      setCurrentPatches(await scan(config.paths.patches));
    } catch (e) {
      console.error(e);
    }
  }, [config.paths.patches]);

  const update = React.useCallback(async () => {
    try {
      setUpdating(true);
      await updatePatches(config.paths.patches, config.patchRepo);
      await rescan();
    } catch (e) {
      console.error("failed to update patches", e);
    } finally {
      setUpdating(false);
    }
  }, [config.paths.patches, config.patchRepo, rescan]);

  React.useEffect(() => {
    update();
    const intervalId = setInterval(() => {
      update();
    }, 60 * 60 * 1000);
    return () => {
      clearInterval(intervalId);
    };
  }, [update]);

  return (
    <Context.Provider
      value={{
        rescan,
        update,
        patches: currentPatches,
        updating,
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

import React, { useContext } from "react";

import { scan } from "../../rom";
import { useConfig } from "./ConfigContext";

export interface ROMsValue {
  rescan(): Promise<void>;
  roms: {
    [name: string]: {
      filename: string;
      revision: number;
    };
  };
}

const Context = React.createContext(null! as ROMsValue);

function makeScanROMs(path: string) {
  let status: "pending" | "error" | "ok" = "pending";
  let result: ROMsValue["roms"];
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

let scanROMs: (() => ROMsValue["roms"]) | null = null;

export const ROMsProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const { config } = useConfig();
  if (scanROMs == null) {
    scanROMs = makeScanROMs(config.paths.roms);
  }
  const [currentROMs, setCurrentROMs] = React.useState(scanROMs());
  const rescan = React.useCallback(async () => {
    try {
      setCurrentROMs(await scan(config.paths.roms));
    } catch (e) {
      console.error(e);
    }
  }, [config.paths.roms]);

  return (
    <Context.Provider
      value={{
        rescan,
        roms: currentROMs,
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

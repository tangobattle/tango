import React, { useContext } from "react";

import { app } from "@electron/remote";

import { getROMsPath } from "../../paths";
import { scan } from "../../rom";

export interface ROMsValue {
  rescan(): Promise<void>;
  roms: { [name: string]: string };
}

const Context = React.createContext(null! as ROMsValue);

function makeScanROMs() {
  let status: "pending" | "error" | "ok" = "pending";
  let result: ROMsValue["roms"];
  let err: any;
  const promise = (async () => {
    try {
      result = await scan(getROMsPath(app));
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

const scanROMs = makeScanROMs();

export const ROMsProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [currentROMs, setCurrentROMs] = React.useState(scanROMs());

  return (
    <Context.Provider
      value={{
        async rescan() {
          try {
            setCurrentROMs(await scan(getROMsPath(app)));
          } catch (e) {
            console.error(e);
          }
        },
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

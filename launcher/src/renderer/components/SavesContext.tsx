import React, { useContext } from "react";

import { scan } from "../../saves";
import { useConfig } from "./ConfigContext";

export interface SavesValue {
  rescan(): Promise<void>;
  saves: { [filename: string]: string[] };
}

const Context = React.createContext(null! as SavesValue);

function makeSaveScans(path: string) {
  let status: "pending" | "error" | "ok" = "pending";
  let result: SavesValue["saves"];
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

let scanSaves: (() => SavesValue["saves"]) | null = null;

export const SavesProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const { config } = useConfig();
  if (scanSaves == null) {
    scanSaves = makeSaveScans(config.paths.saves);
  }
  const [currentSaves, setCurrentSaves] = React.useState(scanSaves());

  return (
    <Context.Provider
      value={{
        async rescan() {
          try {
            setCurrentSaves(await scan(config.paths.saves));
          } catch (e) {
            console.error(e);
          }
        },
        saves: currentSaves,
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

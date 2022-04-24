import React, { useContext } from "react";

import { getSavesPath } from "../../paths";
import { SaveInfo, scan } from "../../saves";

export interface SavesValue {
  rescan(): Promise<void>;
  saves: { [filename: string]: SaveInfo };
}

const Context = React.createContext(null! as SavesValue);

function makeSaveScans() {
  let status: "pending" | "error" | "ok" = "pending";
  let result: SavesValue["saves"];
  let err: any;
  const promise = (async () => {
    try {
      result = await scan(getSavesPath());
    } catch (e) {
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

const scanSaves = makeSaveScans();

export const SavesProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [currentSaves, setCurrentSaves] = React.useState(scanSaves());

  return (
    <Context.Provider
      value={{
        async rescan() {
          setCurrentSaves(await scan(getSavesPath()));
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

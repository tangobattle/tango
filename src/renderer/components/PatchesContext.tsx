import React, { useContext } from "react";

import { PatchInfos, scan } from "../../patchinfo";
import { getPatchesPath } from "../../paths";

export interface PatchesValue {
  rescan(): Promise<void>;
  patches: PatchInfos;
}

const Context = React.createContext(null! as PatchesValue);

function makeScanPatches() {
  let status: "pending" | "error" | "ok" = "pending";
  let result: PatchesValue["patches"];
  let err: any;
  const promise = (async () => {
    try {
      result = await scan(getPatchesPath());
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

const scanPatches = makeScanPatches();

export const PatchesProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [currentPatches, setCurrentPatches] = React.useState(scanPatches());

  return (
    <Context.Provider
      value={{
        async rescan() {
          setCurrentPatches(await scan(getPatchesPath()));
        },
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

import React, { useContext } from "react";
import { getROMsPath } from "../../paths";
import { scan } from "../../rom";

export interface ROMsValue {
  roms: { [filename: string]: string };
}

const Context = React.createContext(null! as ROMsValue);

function makeScanROMs() {
  let status: "pending" | "error" | "ok" = "pending";
  let result: ROMsValue["roms"];
  let err: any;
  const promise = (async () => {
    try {
      result = await scan(getROMsPath());
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

const scanROMs = makeScanROMs();

export const ROMsProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  return (
    <Context.Provider
      value={{
        roms: scanROMs(),
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

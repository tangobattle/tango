import { mkdtemp, rm } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import React, { useContext } from "react";

const Context = React.createContext(null! as { tempDir: string });

function makeGetTempDir() {
  let status: "pending" | "error" | "ok" = "pending";
  let result: string;
  let err: any;
  const promise = (async () => {
    try {
      result = await mkdtemp(path.join(tmpdir(), "tango-"));
      // eslint-disable-next-line no-console
      console.info("created temporary directory:", result);
      const beforeunload = () => {
        (async () => {
          await rm(result, { recursive: true, force: true });
        })();
        window.removeEventListener("beforeunload", beforeunload);
      };
      window.addEventListener("beforeunload", beforeunload);
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

const getTempDir = makeGetTempDir();

export const TempDirProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const tempDir = getTempDir();
  return <Context.Provider value={{ tempDir }}>{children}</Context.Provider>;
};

export const TempDirContext = Context.Consumer;

export function useTempDir() {
  return useContext(Context);
}

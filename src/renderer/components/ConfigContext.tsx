import React, { useContext } from "react";
import * as config from "../../config";

export interface ConfigContextValue {
  config: config.Config;
  save(cfg: config.Config): Promise<void>;
}

const Context = React.createContext(
  null as any as ConfigContextValue /* should never be null */
);

function makeLoadConfig() {
  let status: "pending" | "error" | "ok" = "pending";
  let result: config.Config;
  let err: any;
  const promise = (async () => {
    try {
      result = await config.load();
      await config.save(result);
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

const loadConfig = makeLoadConfig();

export const ConfigProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [currentConfig, setCurrentConfig] = React.useState(loadConfig());

  return (
    <Context.Provider
      value={{
        config: currentConfig,
        save: async (cfg) => {
          config.save(cfg);
          setCurrentConfig(cfg);
        },
      }}
    >
      {children}
    </Context.Provider>
  );
};

export const ConfigConsumer = Context.Consumer;

export function useConfig() {
  return useContext(Context);
}

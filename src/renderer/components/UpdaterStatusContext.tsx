import { ipcRenderer } from "electron";
import React, { useContext } from "react";

export type Status = "not-available" | "available" | "downloaded";

const Context = React.createContext(null! as { status: Status });

export const UpdateStatusProvider = ({
  children,
}: {
  children?: React.ReactNode;
} = {}) => {
  const [status, setUpdateStatus] = React.useState("not-available" as Status);
  const updateStatusListenerCallback = React.useCallback((_e, v) => {
    setUpdateStatus(v);
  }, []);
  React.useEffect(() => {
    ipcRenderer.on("update-status", updateStatusListenerCallback);
    return () => {
      ipcRenderer.off("update-status", updateStatusListenerCallback);
    };
  }, [updateStatusListenerCallback]);

  return (
    <Context.Provider value={{ status: status }}>{children}</Context.Provider>
  );
};

export function useUpdateStatus() {
  return useContext(Context);
}

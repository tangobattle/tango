import * as ipc from ".";
import React from "react";

export function useCore() {
  const [state, setState] = React.useState<ipc.State | null>(null);
  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitStatus, setExitStatus] = React.useState<ipc.ExitStatus | null>(
    null
  );

  const abortControllerRef = React.useRef<AbortController>();

  return {
    start: (args: ipc.Args) => {
      if (abortControllerRef.current != null) {
        throw new Error("core already started");
      }

      const abortController = new AbortController();
      abortControllerRef.current = abortController;
      const core = new ipc.Core(args, {
        signal: abortController.signal,
      });
      core.on("exit", (exitStatus) => {
        setExitStatus(exitStatus);
      });
      core.on("state", (state) => {
        setState(state);
      });
      core.on("stderr", (stderr) => {
        setStderr((lines) => {
          lines.push(stderr);
          return lines;
        });
      });
    },
    stop: () => {
      if (abortControllerRef.current != null) {
        abortControllerRef.current.abort();
      }
    },
    state,
    stderr,
    exitStatus,
  };
}

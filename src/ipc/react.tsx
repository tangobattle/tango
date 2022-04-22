import * as ipc from ".";
import React from "react";

export function useCore(args: ipc.Args) {
  const [state, setState] = React.useState<ipc.State | null>(null);
  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitStatus, setExitStatus] = React.useState<ipc.ExitStatus | null>(
    null
  );

  React.useEffect(() => {
    const abortController = new AbortController();
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
    return () => {
      abortController.abort();
    };
  }, []);

  return { state, stderr, exitStatus };
}

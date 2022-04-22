import * as ipc from "../../ipc";
import { Box, CssBaseline, ThemeProvider } from "@mui/material";
import React, { useEffect } from "react";
import theme from "../theme";

const matchmakingConnectAddr = "wss://mm.tango.murk.land";

const iceServers = [
  "stun://stun.l.google.com:19302",
  "stun://stun1.l.google.com:19302",
  "stun://stun2.l.google.com:19302",
  "stun://stun3.l.google.com:19302",
  "stun://stun4.l.google.com:19302",
];

function useCore(args: ipc.Args) {
  const [state, setState] = React.useState<ipc.State | null>(null);
  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitStatus, setExitStatus] = React.useState<ipc.ExitStatus | null>(
    null
  );

  useEffect(() => {
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

export default function App(): JSX.Element {
  const core = useCore({
    rom_path: "roms/exe6f.gba",
    save_path: "roms/exe6f.sav",
    session_id: "zz",
    match_type: 0,
    replay_prefix: "replay",
    matchmaking_connect_addr: matchmakingConnectAddr,
    ice_servers: iceServers,
    input_delay: 3,
    keymapping: {
      up: "Up",
      down: "Down",
      left: "Left",
      right: "Right",
      a: "Z",
      b: "X",
      l: "A",
      r: "S",
      select: "Back",
      start: "Return",
    },
  });

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <Box>
        <main>
          <div>{core.state}</div>
          <pre>{core.stderr.join("")}</pre>
        </main>
      </Box>
    </ThemeProvider>
  );
}

import { spawnCore } from "../../ipc";
import { Box, CssBaseline, ThemeProvider } from "@mui/material";
import React from "react";
import theme from "../theme";

export default function App(): JSX.Element {
  React.useEffect(() => {
    (async () => {
      const proc = spawnCore({
        romPath: "roms/exe6f.gba",
        savePath: "roms/exe6f.sav",
        sessionID: "zz",
        matchType: 0,
        replayPrefix: "replay",
        matchmakingConnectAddr: "wss://mm.tango.murk.land",
        iceServers: [
          "stun://stun.l.google.com:19302",
          "stun://stun1.l.google.com:19302",
          "stun://stun2.l.google.com:19302",
          "stun://stun3.l.google.com:19302",
          "stun://stun4.l.google.com:19302",
        ],
        inputDelay: 3,
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

      for await (const data of proc.stdout) {
        console.log(`ipc: ${data}`);
      }

      proc.addListener("exit", (code) => {
        console.log(code);
      });
    })();
  });
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <Box>
        <main>henlo</main>
      </Box>
    </ThemeProvider>
  );
}

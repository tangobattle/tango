import { Core } from "../../ipc";
import { Box, CssBaseline, ThemeProvider } from "@mui/material";
import React from "react";
import theme from "../theme";

export default function App(): JSX.Element {
  React.useEffect(() => {
    (async () => {
      const core = new Core({
        rom_path: "roms/exe6f.gba",
        save_path: "roms/exe6f.sav",
        session_id: "zz",
        match_type: 0,
        replay_prefix: "replay",
        matchmaking_connect_addr: "wss://mm.tango.murk.land",
        ice_servers: [
          "stun://stun.l.google.com:19302",
          "stun://stun1.l.google.com:19302",
          "stun://stun2.l.google.com:19302",
          "stun://stun3.l.google.com:19302",
          "stun://stun4.l.google.com:19302",
        ],
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

      for await (const event of core.readEventStream()) {
        console.log(event);
      }
      console.log(await core.wait());
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

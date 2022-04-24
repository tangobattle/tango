import React, { Suspense } from "react";

import Box from "@mui/material/Box";
import CssBaseline from "@mui/material/CssBaseline";
import ThemeProvider from "@mui/system/ThemeProvider";

import theme from "../theme";
import { ConfigProvider } from "./ConfigContext";
import Navbar, { NavbarSelection } from "./Navbar";
import PlayPane from "./panes/PlayPane";
import ReplaysPane from "./panes/ReplaysPane";
import SettingsPane from "./panes/SettingsPane";
import { PatchesProvider } from "./PatchesContext";
import { ROMsProvider } from "./ROMsContext";
import { SavesProvider } from "./SavesContext";

function AppBody() {
  const [selected, setSelected] = React.useState<NavbarSelection>("play");

  return (
    <Box display="flex" height="100%">
      <Suspense fallback={null}>
        <Navbar
          selected={selected}
          onSelect={(v) => {
            setSelected(v);
          }}
        />
      </Suspense>
      <Suspense fallback={null}>
        <ConfigProvider>
          <ROMsProvider>
            <PatchesProvider>
              <SavesProvider>
                <PlayPane active={selected == "play"} />
                <ReplaysPane active={selected == "replays"} />
                <SettingsPane active={selected == "settings"} />
              </SavesProvider>
            </PatchesProvider>
          </ROMsProvider>
        </ConfigProvider>
      </Suspense>
    </Box>
  );
}

export default function App() {
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <AppBody />
    </ThemeProvider>
  );
}

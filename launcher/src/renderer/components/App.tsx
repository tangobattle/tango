import React, { Suspense } from "react";
import { withTranslation } from "react-i18next";

import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import CssBaseline from "@mui/material/CssBaseline";
import ThemeProvider from "@mui/system/ThemeProvider";

import createTheme from "../theme";
import { ConfigProvider, useConfig } from "./ConfigContext";
import Navbar, { NavbarSelection } from "./Navbar";
import PlayPane from "./panes/PlayPane";
import ReplaysPane from "./panes/ReplaysPane";
import SettingsPane from "./panes/SettingsPane";
import { PatchesProvider } from "./PatchesContext";
import { ROMsProvider } from "./ROMsContext";
import { SavesProvider } from "./SavesContext";
import { TempDirProvider } from "./TempDirContext";
import { UpdateStatusProvider } from "./UpdaterStatusContext";

const AppBody = withTranslation()(() => {
  const [selected, setSelected] = React.useState<NavbarSelection>("play");

  const { config } = useConfig();

  return (
    <ThemeProvider theme={createTheme(config.theme)}>
      <CssBaseline />
      <Box sx={{ display: "flex", height: "100%", width: "100%" }}>
        <Suspense fallback={null}>
          <Navbar
            selected={selected}
            onSelect={(v) => {
              setSelected(v);
            }}
          />
        </Suspense>
        <Suspense
          fallback={
            <Box
              sx={{
                display: "flex",
                width: "100%",
                height: "100%",
                justifyContent: "center",
                alignItems: "center",
              }}
            >
              <CircularProgress />
            </Box>
          }
        >
          <ROMsProvider>
            <PatchesProvider>
              <SavesProvider>
                <PlayPane active={selected == "play"} />
                <ReplaysPane active={selected == "replays"} />
                <SettingsPane active={selected == "settings"} />
              </SavesProvider>
            </PatchesProvider>
          </ROMsProvider>
        </Suspense>
      </Box>
    </ThemeProvider>
  );
});

export default function App() {
  return (
    <UpdateStatusProvider>
      <Suspense
        fallback={
          <Box
            sx={{
              display: "flex",
              width: "100%",
              height: "100%",
              justifyContent: "center",
              alignItems: "center",
            }}
          >
            <CircularProgress />
          </Box>
        }
      >
        <TempDirProvider>
          <ConfigProvider>
            <AppBody />
          </ConfigProvider>
        </TempDirProvider>
      </Suspense>
    </UpdateStatusProvider>
  );
}

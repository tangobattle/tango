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
import { ROMsProvider, useROMs } from "./ROMsContext";
import { SavesProvider, useSaves } from "./SavesContext";
import { TempDirProvider } from "./TempDirContext";
import { UpdateStatusProvider } from "./UpdaterStatusContext";

function ReadyAppBody() {
  const [selected, setSelected] = React.useState<NavbarSelection>("play");

  return (
    <>
      <Navbar
        selected={selected}
        onSelect={(v) => {
          setSelected(v);
        }}
      />
      <PlayPane active={selected == "play"} />
      <ReplaysPane active={selected == "replays"} />
      <SettingsPane active={selected == "settings"} />
    </>
  );
}

function SetupAppBody() {
  const { roms } = useROMs();
  const { saves } = useSaves();
  return <>copy roms to roms and saves to saves pls and restart tango</>;
}

function AppBody() {
  const { roms } = useROMs();
  const { saves } = useSaves();

  return Object.keys(roms).length > 0 && Object.keys(saves).length > 0 ? (
    <ReadyAppBody />
  ) : (
    <SetupAppBody />
  );
}

const ThemedAppWrapper = withTranslation()(() => {
  const { config } = useConfig();

  return (
    <ThemeProvider theme={createTheme(config.theme)}>
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
              <CssBaseline />
              <AppBody />;
            </SavesProvider>
          </PatchesProvider>
        </ROMsProvider>
      </Suspense>
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
            <CssBaseline />
            <Box sx={{ display: "flex", height: "100%", width: "100%" }}>
              <ThemedAppWrapper />
            </Box>
          </ConfigProvider>
        </TempDirProvider>
      </Suspense>
    </UpdateStatusProvider>
  );
}

import React, { Suspense } from "react";
import { withTranslation } from "react-i18next";

import { app, shell } from "@electron/remote";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import CssBaseline from "@mui/material/CssBaseline";
import Stack from "@mui/material/Stack";
import Step from "@mui/material/Step";
import StepContent from "@mui/material/StepContent";
import StepLabel from "@mui/material/StepLabel";
import Stepper from "@mui/material/Stepper";
import Typography from "@mui/material/Typography";
import ThemeProvider from "@mui/system/ThemeProvider";

import { getROMsPath, getSavesPath } from "../../paths";
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
  const { roms, rescan: rescanROMs } = useROMs();
  const { saves, rescan: rescanSaves } = useSaves();

  const activeStep =
    Object.keys(roms).length > 0 ? (Object.keys(saves).length > 0 ? 2 : 1) : 0;

  return (
    <Box
      sx={{
        width: 400,
        py: 3,
        pt: 10,
        mx: "auto",
      }}
    >
      <Stepper orientation="vertical" activeStep={activeStep}>
        <Step completed={activeStep > 0}>
          <StepLabel>
            Place your ROMs in <code>{getROMsPath(app)}</code>.
          </StepLabel>
          <StepContent>
            <Typography>Make sure they're pristine, unpatched ROMs!</Typography>
            <Stack sx={{ mt: 1 }} spacing={1} direction="row">
              <Button
                variant="outlined"
                size="small"
                onClick={() => {
                  shell.openPath(getROMsPath(app));
                }}
              >
                Open folder
              </Button>
              <Button
                variant="contained"
                size="small"
                onClick={() => {
                  rescanROMs();
                }}
              >
                I'm done!
              </Button>
            </Stack>
          </StepContent>
        </Step>
        <Step completed={activeStep > 1}>
          <StepLabel>
            Place your saves in <code>{getSavesPath(app)}</code>.
          </StepLabel>
          <StepContent>
            <Typography>Make sure they're pristine, unpatched ROMs!</Typography>
            <Stack sx={{ mt: 1 }} spacing={1} direction="row">
              <Button
                variant="outlined"
                size="small"
                onClick={() => {
                  shell.openPath(getSavesPath(app));
                }}
              >
                Open folder
              </Button>
              <Button
                variant="contained"
                size="small"
                onClick={() => {
                  rescanSaves();
                }}
              >
                I'm done!
              </Button>
            </Stack>
          </StepContent>
        </Step>
        <Step completed={activeStep > 2}>
          <StepLabel>You're ready to go!</StepLabel>
        </Step>
      </Stepper>
    </Box>
  );
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

const AppWrapper = withTranslation()(() => {
  const { config } = useConfig();

  return (
    <ThemeProvider theme={createTheme(config.theme)}>
      <CssBaseline />
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
              <AppBody />
            </SavesProvider>
          </PatchesProvider>
        </ROMsProvider>
      </Suspense>
    </ThemeProvider>
  );
});

export default function App() {
  return (
    <Box
      sx={{
        display: "flex",
        height: "100%",
        width: "100%",
      }}
    >
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
              <AppWrapper />
            </ConfigProvider>
          </TempDirProvider>
        </Suspense>
      </UpdateStatusProvider>
    </Box>
  );
}

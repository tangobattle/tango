import React, { Suspense } from "react";
import { Trans, useTranslation, withTranslation } from "react-i18next";

import { app, shell } from "@electron/remote";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import CssBaseline from "@mui/material/CssBaseline";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Step from "@mui/material/Step";
import StepContent from "@mui/material/StepContent";
import StepLabel from "@mui/material/StepLabel";
import Stepper from "@mui/material/Stepper";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import ThemeProvider from "@mui/system/ThemeProvider";

import { getROMsPath, getSavesPath } from "../../paths";
import { LANGUAGES } from "../i18n";
import createTheme from "../theme";
import { ConfigProvider, useConfig } from "./ConfigContext";
import Navbar, { NavbarSelection } from "./Navbar";
import PatchesPane from "./panes/PatchesPane";
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
      <PatchesPane active={selected == "patches"} />
      <SettingsPane active={selected == "settings"} />
    </>
  );
}

function SetupAppBody() {
  const { i18n, t } = useTranslation();

  const { config, save: saveConfig } = useConfig();
  const { roms, rescan: rescanROMs } = useROMs();
  const { saves, rescan: rescanSaves } = useSaves();

  const activeStep =
    Object.keys(roms).length > 0
      ? Object.keys(saves).length > 0
        ? config.nickname != null
          ? 3
          : 2
        : 1
      : 0;

  const [nickname, setNickname] = React.useState("");
  const [romsScanState, setROMsScanState] = React.useState<
    "init" | "pending" | "done"
  >("init");
  const [savesScanState, setSavesScanState] = React.useState<
    "init" | "pending" | "done"
  >("init");

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
      }}
    >
      <Box sx={{ width: "100%", textAlign: "right", px: 1, py: 1 }}>
        <Select
          size="small"
          variant="standard"
          value={i18n.resolvedLanguage}
          onChange={(e) => {
            i18n.changeLanguage(e.target.value);
          }}
        >
          {LANGUAGES.map(({ code, name }) => (
            <MenuItem key={code} value={code}>
              {name}
            </MenuItem>
          ))}
        </Select>
      </Box>

      <Box
        sx={{
          width: 400,
          py: 3,
          pt: 2,
          mx: "auto",
        }}
      >
        <img
          src={require("../../../static/images/logo.png")}
          width={160}
          height={160}
          alt="Tango"
          title="Tango"
          draggable={false}
          style={{ marginLeft: "auto", marginRight: "auto", display: "block" }}
        />
        <Typography sx={{ mb: 2, mt: 3 }}>
          <Trans i18nKey="setup:welcome" />
        </Typography>
        <Typography sx={{ mb: 2 }}>
          <Trans i18nKey="setup:welcome-2" />
        </Typography>
        <Stepper orientation="vertical" activeStep={activeStep}>
          <Step completed={Object.keys(roms).length > 0}>
            <StepLabel>
              <Trans
                i18nKey="setup:roms-step-title"
                values={{ path: getROMsPath(app) }}
              />
            </StepLabel>
            <StepContent>
              <Typography sx={{ mb: 2 }}>
                <Trans i18nKey="setup:roms-step-description" />
              </Typography>
              {romsScanState == "done" && activeStep == 0 ? (
                <Alert sx={{ mb: 2 }} severity="warning">
                  <Trans i18nKey="setup:step-2-error" />
                </Alert>
              ) : null}
              <Stack spacing={1} direction="row">
                <Button
                  variant="outlined"
                  size="small"
                  onClick={() => {
                    shell.openPath(getROMsPath(app));
                  }}
                >
                  <Trans i18nKey="setup:open-folder" />
                </Button>
                <Button
                  variant="contained"
                  size="small"
                  disabled={romsScanState == "pending"}
                  onClick={() => {
                    setROMsScanState("pending");
                    (async () => {
                      await rescanROMs();
                      setROMsScanState("done");
                    })();
                  }}
                >
                  <Trans i18nKey="setup:next" />
                </Button>
              </Stack>
            </StepContent>
          </Step>
          <Step completed={Object.keys(saves).length > 0}>
            <StepLabel>
              <Trans
                i18nKey="setup:saves-step-title"
                values={{ path: getSavesPath(app) }}
              />
            </StepLabel>
            <StepContent>
              <Typography sx={{ mb: 2 }}>
                <Trans i18nKey="setup:saves-step-description" />
              </Typography>
              {savesScanState == "done" && activeStep == 1 ? (
                <Alert sx={{ mb: 2 }} severity="warning">
                  <Trans i18nKey="setup:saves-step-error" />
                </Alert>
              ) : null}
              <Stack spacing={1} direction="row">
                <Button
                  variant="outlined"
                  size="small"
                  disabled={savesScanState == "pending"}
                  onClick={() => {
                    shell.openPath(getSavesPath(app));
                  }}
                >
                  <Trans i18nKey="setup:open-folder" />
                </Button>
                <Button
                  variant="contained"
                  size="small"
                  onClick={() => {
                    setSavesScanState("pending");
                    (async () => {
                      await rescanSaves();
                      setSavesScanState("done");
                    })();
                  }}
                >
                  <Trans i18nKey="setup:next" />
                </Button>
              </Stack>
            </StepContent>
          </Step>
          <Step completed={config.nickname != null}>
            <StepLabel>
              <Trans i18nKey="setup:nickname-step-title" />
            </StepLabel>
            <StepContent>
              <Typography sx={{ mb: 2 }}>
                <Trans i18nKey="setup:nickname-step-description" />
              </Typography>
              <Stack
                spacing={1}
                direction="row"
                component="form"
                sx={{ mb: 0 }}
                onSubmit={(e: React.FormEvent<HTMLFormElement>) => {
                  e.preventDefault();
                  saveConfig((config) => ({ ...config, nickname }));
                }}
              >
                <TextField
                  variant="standard"
                  size="small"
                  onChange={(e) => {
                    setNickname(e.target.value.trim());
                  }}
                  placeholder={t("setup:nickname")}
                />
                <Button
                  variant="contained"
                  size="small"
                  disabled={nickname.length == 0}
                  type="submit"
                >
                  <Trans i18nKey="setup:next" />
                </Button>
              </Stack>
            </StepContent>
          </Step>
        </Stepper>
      </Box>
    </Box>
  );
}

function AppBody() {
  const { config } = useConfig();
  return config.nickname != null ? <ReadyAppBody /> : <SetupAppBody />;
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

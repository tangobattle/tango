import React from "react";
import { Trans } from "react-i18next";
import tmp from "tmp-promise";

import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Modal from "@mui/material/Modal";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";

import { makeROM } from "../../game";
import * as ipc from "../../ipc";
import { ReplayInfo } from "../../replay";
import { useConfig } from "./ConfigContext";

export function CoreSupervisor({
  romPath,
  savePath,
  patchPath,
  matchSettings,
  windowTitle,
  incarnation,
  onExit,
}: {
  romPath: string;
  savePath: string;
  patchPath?: string;
  matchSettings?: {
    sessionID: string;
    replaysPath: string;
    replayInfo: ReplayInfo;
    inputDelay: number;
    matchType: number;
  };
  incarnation: number;
  windowTitle: string;
  onExit: () => void;
}) {
  const { config } = useConfig();

  const configRef = React.useRef(config);
  const romTmpFileRef = React.useRef<tmp.FileResult | null>(null);

  const onExitRef = React.useRef(onExit);
  React.useEffect(() => {
    onExitRef.current = onExit;
  }, [onExit]);

  const [state, setState] = React.useState<ipc.State | null>(null);
  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitLingering, setExitLingering] = React.useState(false);

  const abortControllerRef = React.useRef<AbortController>(null!);
  if (abortControllerRef.current == null) {
    abortControllerRef.current = new AbortController();
  }

  React.useEffect(() => {
    (async () => {
      romTmpFileRef.current = await makeROM(romPath, patchPath || null);

      const core = new ipc.Core(
        {
          window_title: windowTitle,
          rom_path: romTmpFileRef.current!.path,
          save_path: savePath,
          keymapping: configRef.current.keymapping,
          match_settings:
            matchSettings == null
              ? null
              : {
                  session_id: matchSettings.sessionID,
                  input_delay: matchSettings.inputDelay,
                  match_type: matchSettings.matchType,
                  matchmaking_connect_addr:
                    configRef.current.matchmakingConnectAddr,
                  ice_servers: configRef.current.iceServers,
                  replays_path: matchSettings.replaysPath,
                  replay_metadata: JSON.stringify(matchSettings.replayInfo),
                },
        },
        {
          signal: abortControllerRef.current.signal,
        }
      );
      core.on("exit", (exitStatus) => {
        if (exitStatus.exitCode == 0 || exitStatus.signalCode == "SIGTERM") {
          onExitRef.current();
        } else {
          setExitLingering(true);
        }
      });
      core.on("state", (state) => {
        setState(state);
      });
      core.on("stderr", (buf) => {
        setStderr((stderr) => {
          stderr.push(buf.toString());
          return stderr;
        });
      });
    })();

    return () => {
      if (romTmpFileRef.current != null) {
        romTmpFileRef.current.cleanup();
      }
      abortControllerRef.current.abort();
    };
  }, [romPath, savePath, patchPath, windowTitle, matchSettings, incarnation]);

  return (
    <Modal
      open={true}
      onClose={(e, reason) => {
        if (reason == "backdropClick" || reason == "escapeKeyDown") {
          return;
        }
      }}
    >
      <Box
        sx={{
          position: "absolute",
          top: "50%",
          left: "50%",
          transform: "translate(-50%, -50%)",
        }}
      >
        {!exitLingering ? (
          <Box
            sx={{
              width: 300,
              bgcolor: "background.paper",
              boxShadow: 24,
              px: 3,
              py: 2,
            }}
          >
            <Stack spacing={1}>
              <Stack
                direction="row"
                justifyContent="flex-start"
                alignItems="center"
                spacing={2}
              >
                <CircularProgress
                  sx={{ flexGrow: 0, flexShrink: 0 }}
                  disableShrink
                  size="2rem"
                />
                <Typography>
                  {state == null ? (
                    <Trans i18nKey="supervisor:status.starting" />
                  ) : state == "Running" ? (
                    <Trans i18nKey="supervisor:status.running" />
                  ) : state == "Waiting" ? (
                    <Trans i18nKey="supervisor:status.waiting" />
                  ) : state == "Connecting" ? (
                    <Trans i18nKey="supervisor:status.connecting" />
                  ) : null}
                </Typography>
              </Stack>
              <Stack direction="row" justifyContent="flex-end">
                <Button
                  variant="contained"
                  color="error"
                  onClick={(_e) => {
                    if (abortControllerRef.current != null) {
                      abortControllerRef.current.abort();
                    }
                  }}
                >
                  <Trans i18nKey="supervisor:cancel" />
                </Button>
              </Stack>
            </Stack>
          </Box>
        ) : (
          <Box
            sx={{
              width: 600,
              bgcolor: "background.paper",
              boxShadow: 24,
              px: 3,
              py: 2,
              display: "flex",
            }}
          >
            <Stack spacing={1} flexGrow={1}>
              <Box sx={{ flexGrow: 0, flexShrink: 0 }}>
                <Trans i18nKey="supervisor:crash" />
              </Box>
              <TextField
                multiline
                InputProps={{
                  sx: {
                    fontSize: "0.8rem",
                    fontFamily: "monospace",
                  },
                }}
                maxRows={20}
                sx={{
                  flexGrow: 1,
                }}
                value={stderr.join("").trimEnd()}
              />
              <Stack direction="row" justifyContent="flex-end">
                <Button
                  variant="contained"
                  color="error"
                  onClick={(_e) => {
                    onExitRef.current();
                  }}
                >
                  <Trans i18nKey="supervisor:dismiss" />
                </Button>
              </Stack>
            </Stack>
          </Box>
        )}
      </Box>
    </Modal>
  );
}

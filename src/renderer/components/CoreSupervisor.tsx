import path from "path";
import React from "react";
import { Trans } from "react-i18next";

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
import { usePatchPath, useROMPath } from "../hooks";
import { useConfig } from "./ConfigContext";
import { CopyButton } from "./CopyButton";
import { useTempDir } from "./TempDirContext";

export function CoreSupervisor({
  romName,
  patch,
  savePath,
  matchSettings,
  windowTitle,
  incarnation,
  onExit,
}: {
  romName: string;
  patch?: { name: string; version: string };
  savePath: string;
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
  const { tempDir } = useTempDir();

  const configRef = React.useRef(config);

  const romPath = useROMPath(romName);
  const patchPath = usePatchPath(patch ?? null);

  const outROMPath = path.join(
    tempDir,
    `${romName}${patch != null ? `+${patch.name}-v${patch.version}` : ""}.gba`
  );

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
      try {
        await makeROM(romPath, patchPath || null, outROMPath);
      } catch (e) {
        setStderr((stderr) => {
          stderr.push((e as any).toString());
          return stderr;
        });
        setExitLingering(true);
        throw e;
      }

      const core = new ipc.Core(
        {
          window_title: windowTitle,
          rom_path: outROMPath,
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
          env: {
            WGPU_BACKEND:
              configRef.current.wgpuBackend != null
                ? configRef.current.wgpuBackend
                : undefined,
          },
          signal: abortControllerRef.current.signal,
        }
      );
      core.on("exit", (exitStatus) => {
        setStderr((stderr) => {
          stderr.push(`\nexited with ${exitStatus}\n`);
          return stderr;
        });
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
      core.on("error", (err) => {
        setStderr((stderr) => {
          stderr.push(err.toString());
          return stderr;
        });
        setExitLingering(true);
      });
    })();
  }, [
    romPath,
    savePath,
    patchPath,
    outROMPath,
    windowTitle,
    matchSettings,
    incarnation,
  ]);

  return (
    <Modal
      open={true}
      onClose={(_e, reason) => {
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
              <Box
                sx={{
                  flexGrow: 0,
                  flexShrink: 0,
                  display: "flex",
                  position: "relative",
                }}
              >
                <CopyButton
                  value={stderr.join("").trimEnd()}
                  sx={{
                    position: "absolute",
                    right: "16px",
                    top: "8px",
                    zIndex: 999,
                  }}
                />
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
              </Box>
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

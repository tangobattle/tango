import * as ipc from "../../ipc";
import React from "react";
import tmp from "tmp-promise";
import { useConfig } from "./ConfigContext";
import { makeROM } from "../../game";
import { Trans } from "react-i18next";
import Modal from "@mui/material/Modal";
import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import { ReplayInfo } from "../../replay";

export function CoreSupervisor({
  romPath,
  patchPath,
  replayPrefix,
  replayInfo,
  windowTitle,
  sessionID,
  onExit,
}: {
  romPath: string;
  patchPath: string | null;
  replayPrefix: string;
  replayInfo: ReplayInfo;
  windowTitle: string;
  sessionID?: string;
  onExit: (exitStatus: ipc.ExitStatus) => void;
}) {
  const { config } = useConfig();

  const configRef = React.useRef(config);

  const onExitRef = React.useRef(onExit);
  React.useEffect(() => {
    onExitRef.current = onExit;
  }, [onExit]);

  const [state, setState] = React.useState<ipc.State | null>(null);
  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitStatus, setExitStatus] = React.useState<ipc.ExitStatus | null>(
    null
  );

  const abortControllerRef = React.useRef<AbortController>(null!);
  if (abortControllerRef.current == null) {
    abortControllerRef.current = new AbortController();
  }

  React.useEffect(() => {
    let romTmpFile: tmp.FileResult | null = null;

    (async () => {
      romTmpFile = await makeROM(romPath, patchPath);

      const core = new ipc.Core(
        {
          window_title: windowTitle,
          rom_path: romTmpFile!.path,
          save_path: "saves/exe6f.sav",
          keymapping: configRef.current.keymapping,
          match_settings:
            sessionID == null
              ? null
              : {
                  session_id: sessionID,
                  input_delay: 0,
                  match_type: 0,
                  matchmaking_connect_addr:
                    configRef.current.matchmakingConnectAddr,
                  ice_servers: configRef.current.iceServers,
                  replay_prefix: replayPrefix,
                  replay_metadata: JSON.stringify(replayInfo),
                },
        },
        {
          signal: abortControllerRef.current.signal,
        }
      );
      core.on("exit", (exitStatus) => {
        setExitStatus(exitStatus);
        onExitRef.current(exitStatus);
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
    })();

    return () => {
      if (romTmpFile != null) {
        romTmpFile.cleanup();
      }
      abortControllerRef.current.abort();
    };
  }, [romPath, patchPath, replayInfo, replayPrefix, windowTitle, sessionID]);

  return (
    <Modal open={true}>
      <Box
        sx={{
          position: "absolute",
          top: "50%",
          left: "50%",
          transform: "translate(-50%, -50%)",
          width: 400,
          bgcolor: "background.paper",
          boxShadow: 24,
          p: 4,
        }}
      >
        <Stack
          direction="row"
          justifyContent="flex-start"
          alignItems="center"
          spacing={2}
        >
          <CircularProgress disableShrink size="2rem" />
          <Typography>
            {state == null ? (
              <Trans i18nKey="supervisor:status.starting"></Trans>
            ) : state == "Running" ? (
              <Trans i18nKey="supervisor:status.running"></Trans>
            ) : state == "Waiting" ? (
              <Trans i18nKey="supervisor:status.waiting"></Trans>
            ) : state == "Connecting" ? (
              <Trans i18nKey="supervisor:status.connecting"></Trans>
            ) : null}
          </Typography>
        </Stack>
      </Box>
    </Modal>
  );
}

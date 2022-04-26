import { spawn } from "child_process";
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
import { getBinPath } from "../../paths";
import { useConfig } from "./ConfigContext";
import { CopyButton } from "./CopyButton";

export default function ReplayviewSupervisor({
  romPath,
  replayPath,
  patchPath,
  onExit,
}: {
  romPath: string;
  replayPath: string;
  patchPath?: string;
  onExit: () => void;
}) {
  const { config } = useConfig();

  const configRef = React.useRef(config);
  const romTmpFileRef = React.useRef<tmp.FileResult | null>(null);

  const onExitRef = React.useRef(onExit);
  React.useEffect(() => {
    onExitRef.current = onExit;
  }, [onExit]);

  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitLingering, setExitLingering] = React.useState(false);

  const abortControllerRef = React.useRef<AbortController>(null!);
  if (abortControllerRef.current == null) {
    abortControllerRef.current = new AbortController();
  }

  React.useEffect(() => {
    (async () => {
      romTmpFileRef.current = await makeROM(romPath, patchPath || null);

      const proc = spawn(
        getBinPath("replayview"),
        [romTmpFileRef.current.path, replayPath],
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

      const beforeunload = () => {
        proc.kill();
        window.removeEventListener("beforeunload", beforeunload);
      };
      window.addEventListener("beforeunload", beforeunload);

      (async () => {
        for await (const buf of proc.stderr) {
          setStderr((stderr) => {
            stderr.push(buf.toString());
            return stderr;
          });
        }
      })();

      proc.on("exit", (code, signal) => {
        if (code == 0 || signal == "SIGTERM") {
          onExitRef.current();
        } else {
          setExitLingering(true);
        }
      });
    })();

    return () => {
      if (romTmpFileRef.current != null) {
        romTmpFileRef.current.cleanup();
      }
      abortControllerRef.current.abort();
    };
  }, [romPath, patchPath, replayPath]);

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
                  <Trans i18nKey="replays:viewing" />
                </Typography>
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
                <Trans i18nKey="replays:crash" />
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

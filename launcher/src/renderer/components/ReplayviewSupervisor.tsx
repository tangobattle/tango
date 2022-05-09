import path from "path";
import React from "react";
import { Trans } from "react-i18next";

import { app } from "@electron/remote";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Modal from "@mui/material/Modal";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";

import { makeROM } from "../../game";
import { spawn } from "../../process";
import { usePatchPath, useROMPath } from "../hooks";
import { useConfig } from "./ConfigContext";
import { CopyButton } from "./CopyButton";
import { useTempDir } from "./TempDirContext";

export default function ReplayviewSupervisor({
  romName,
  patch,
  replayPath,
  onExit,
}: {
  romName: string;
  patch?: { name: string; version: string };
  replayPath: string;
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

  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitLingering, setExitLingering] = React.useState(false);

  const abortControllerRef = React.useRef<AbortController>(null!);
  if (abortControllerRef.current == null) {
    abortControllerRef.current = new AbortController();
  }

  React.useEffect(() => {
    (async () => {
      try {
        await makeROM(romPath!, patchPath, outROMPath);
      } catch (e) {
        setStderr((stderr) => {
          stderr.push((e as any).toString());
          return stderr;
        });
        setExitLingering(true);
        throw e;
      }

      const proc = spawn(app, "replayview", [outROMPath, replayPath], {
        env: {
          WGPU_BACKEND:
            configRef.current.wgpuBackend != null
              ? configRef.current.wgpuBackend
              : undefined,
          RUST_LOG: configRef.current.rustLogFilter,
          RUST_BACKTRACE: "1",
        },
        signal: abortControllerRef.current.signal,
      });

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

      proc.on("error", (err: any) => {
        setStderr((stderr) => {
          stderr.push(err.toString());
          return stderr;
        });
        setExitLingering(true);
      });

      proc.on("exit", (exitCode, signalCode) => {
        setStderr((stderr) => {
          stderr.push(
            `\nexited with ${JSON.stringify({ exitCode, signalCode })}\n`
          );
          return stderr;
        });
        if (exitCode == 0 || signalCode == "SIGTERM") {
          onExitRef.current();
        } else {
          setExitLingering(true);
        }
      });
    })();
  }, [romPath, patchPath, outROMPath, replayPath]);

  return (
    <Modal
      open={true}
      onClose={(_e, reason) => {
        if (
          !exitLingering &&
          (reason == "backdropClick" || reason == "escapeKeyDown")
        ) {
          return;
        }
        onExit();
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

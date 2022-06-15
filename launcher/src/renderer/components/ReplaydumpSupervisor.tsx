import path from "path";
import React from "react";
import { Trans } from "react-i18next";

import { app, shell } from "@electron/remote";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import LinearProgress from "@mui/material/LinearProgress";
import Modal from "@mui/material/Modal";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";

import { makeROM } from "../../game";
import { getBinPath } from "../../paths";
import { spawn } from "../../process";
import { useGetPatchPath, useGetROMPath } from "../hooks";
import { useConfig } from "./ConfigContext";
import CopyButton from "./CopyButton";
import { useTempDir } from "./TempDirContext";

export default function ReplaydumpSupervisor({
  romName,
  patch,
  replayPath,
  outPath,
  scaleFactor,
  onExit,
}: {
  romName: string;
  patch?: { name: string; version: string };
  replayPath: string;
  outPath: string;
  scaleFactor: number;
  onExit: () => void;
}) {
  const { config } = useConfig();
  const { tempDir } = useTempDir();

  const configRef = React.useRef(config);

  const getROMPath = useGetROMPath();
  const getPatchPath = useGetPatchPath();

  const romPath = getROMPath(romName);
  const patchPath = patch != null ? getPatchPath(romName, patch) : null;

  const outROMPath = path.join(
    tempDir,
    `${romName}${patch != null ? `+${patch.name}-v${patch.version}` : ""}.gba`
  );

  const onExitRef = React.useRef(onExit);
  React.useEffect(() => {
    onExitRef.current = onExit;
  }, [onExit]);

  const [stderr, setStderr] = React.useState<string[]>([]);
  const [done, setDone] = React.useState<{
    exitCode: number | null;
    signalCode: NodeJS.Signals | null;
  }>({ exitCode: null, signalCode: null });

  const maxProgressRef = React.useRef(0);
  const [progress, setProgress] = React.useState(0);

  const abortControllerRef = React.useRef<AbortController>(null!);
  if (abortControllerRef.current == null) {
    abortControllerRef.current = new AbortController();
  }

  React.useEffect(() => {
    (async () => {
      try {
        await makeROM(romPath!, patchPath || null, outROMPath);
      } catch (e) {
        setStderr((stderr) => {
          stderr.push((e as any).toString());
          return stderr;
        });
        setDone({ exitCode: -1, signalCode: null });
        throw e;
      }

      const proc = spawn(
        app,
        "replaydump",
        [
          replayPath,
          "video",
          `--ffmpeg=${getBinPath(app, "ffmpeg")}`,
          `-v=-c:v libx264 -vf scale=iw*${scaleFactor}:ih*${scaleFactor}:flags=neighbor,format=yuv420p -force_key_frames expr:gte(t,n_forced/2) -crf 18 -bf 2`,
          "--",
          outROMPath,
          outPath,
        ],
        {
          env: {
            ...process.env,
            RUST_LOG: configRef.current.rustLogFilter,
            RUST_BACKTRACE: "1",
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

      (async () => {
        let buf = "";
        for await (const data of proc.stdout) {
          buf += data;
          const lines = buf.split(/\n/g);
          buf = lines[lines.length - 1];

          const ready = lines.slice(0, -1);
          const progress = parseInt(ready[ready.length - 1]);
          if (maxProgressRef.current == 0) {
            maxProgressRef.current = progress;
          }
          setProgress(progress);
        }
      })();

      proc.on("error", (err: any) => {
        setStderr((stderr) => {
          stderr.push(err.toString());
          return stderr;
        });
        setDone((done) =>
          done == null ? { exitCode: -1, signalCode: null } : done
        );
      });

      proc.on("exit", (exitCode, signalCode) => {
        setStderr((stderr) => {
          stderr.push(
            `\nexited with ${JSON.stringify({ exitCode, signalCode })}\n`
          );
          return stderr;
        });
        if (signalCode == "SIGTERM") {
          onExitRef.current();
          return;
        }
        if (exitCode == 0 && signalCode == null) {
          shell.showItemInFolder(outPath);
        }
        setDone({ exitCode, signalCode });
      });
    })();
  }, [romPath, patchPath, outROMPath, scaleFactor, replayPath, outPath]);

  const pct =
    maxProgressRef.current > 0
      ? ((maxProgressRef.current - progress) * 100) / maxProgressRef.current
      : 0;

  return (
    <Modal
      open={true}
      onClose={(_e, reason) => {
        if (reason == "backdropClick" || reason == "escapeKeyDown") {
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
        {done.exitCode == null && done.signalCode == null ? (
          <Box
            sx={{
              width: 400,
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
                <Typography>
                  <Trans i18nKey="replays:exporting" />
                </Typography>
              </Stack>
              <Stack direction="row" sx={{ alignItems: "center" }}>
                <Box sx={{ width: "100%", mr: 1 }}>
                  <LinearProgress variant="determinate" value={pct} />
                </Box>
                <Box sx={{ minWidth: 35 }}>
                  <Typography
                    variant="body2"
                    color="text.secondary"
                  >{`${Math.round(pct)}%`}</Typography>
                </Box>
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
        ) : done.exitCode == 0 ? (
          <Box
            sx={{
              width: 400,
              bgcolor: "background.paper",
              boxShadow: 24,
              px: 3,
              py: 2,
              display: "flex",
            }}
          >
            <Stack spacing={1} flexGrow={1}>
              <Box sx={{ flexGrow: 0, flexShrink: 0 }}>
                <Trans i18nKey="replays:export-complete" />
              </Box>
              <Box
                sx={{
                  flexGrow: 0,
                  flexShrink: 0,
                  display: "flex",
                  position: "relative",
                }}
              ></Box>
              <Stack direction="row" justifyContent="flex-end">
                <Button
                  variant="contained"
                  onClick={(_e) => {
                    onExitRef.current();
                  }}
                >
                  <Trans i18nKey="supervisor:ok" />
                </Button>
              </Stack>
            </Stack>
          </Box>
        ) : done.signalCode != "SIGTERM" ? (
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
        ) : null}
      </Box>
    </Modal>
  );
}

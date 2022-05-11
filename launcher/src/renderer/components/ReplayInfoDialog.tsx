import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { app } from "@electron/remote";
import CloseIcon from "@mui/icons-material/Close";
import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import IconButton from "@mui/material/IconButton";
import Modal from "@mui/material/Modal";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { getReplaysPath } from "../../paths";
import { spawn } from "../../process";
import { ReplayInfo } from "../../replay";
import { Editor } from "../../saveedit/bn6";
import SaveViewer from "./SaveViewer";

export default function ReplayInfoDialog({
  filename,
  replayInfo,
  onClose,
}: {
  filename: string;
  replayInfo: ReplayInfo;
  onClose: () => void;
}) {
  const [editor, setEditor] = React.useState<Editor | null>(null);
  const { i18n } = useTranslation();
  const dateFormat = new Intl.DateTimeFormat(i18n.resolvedLanguage, {
    dateStyle: "medium",
    timeStyle: "medium",
  });

  React.useEffect(() => {
    (async () => {
      const proc = spawn(app, "replaydump", [
        path.join(getReplaysPath(app), filename),
        "dump-ewram",
      ]);

      (async () => {
        for await (const buf of proc.stderr) {
          // eslint-disable-next-line no-console
          console.log(buf.toString());
        }
      })();

      const bufs = [];
      for await (const buf of proc.stdout) {
        bufs.push(buf);
      }

      const buf = Buffer.concat(bufs);
      setEditor(new Editor(new Uint8Array(buf).buffer, replayInfo.rom, false));
    })();
  }, [filename, replayInfo]);

  return (
    <Modal
      open={true}
      onClose={(_e, _reason) => {
        onClose();
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
        <Stack
          sx={{
            width: 600,
            height: 600,
            bgcolor: "background.paper",
            boxShadow: 24,
          }}
          direction="column"
        >
          <Stack direction="row" sx={{ pt: 1, px: 1, alignItems: "center" }}>
            <Box>
              {replayInfo.linkCode != null ? (
                <>
                  <Typography variant="h6" component="h2" sx={{ px: 1 }}>
                    <Trans
                      i18nKey="replays:replay-title"
                      values={{
                        formattedDate: dateFormat.format(
                          new Date(replayInfo.ts)
                        ),
                        nickname: replayInfo.remote!.nickname,
                        linkCode: replayInfo.linkCode,
                      }}
                    />
                    <br />
                    <small>{dateFormat.format(new Date(replayInfo.ts))}</small>
                  </Typography>
                </>
              ) : (
                <Typography variant="h6" component="h2" sx={{ px: 1 }}>
                  {dateFormat.format(new Date(replayInfo.ts))}
                </Typography>
              )}
            </Box>
            <Tooltip title={<Trans i18nKey="common:close" />}>
              <IconButton
                sx={{ ml: "auto" }}
                onClick={() => {
                  onClose();
                }}
              >
                <CloseIcon />
              </IconButton>
            </Tooltip>
          </Stack>
          <Box flexGrow={1} sx={{ display: "flex" }}>
            {editor != null ? (
              <SaveViewer editor={editor} />
            ) : (
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
            )}
          </Box>
        </Stack>
      </Box>
    </Modal>
  );
}

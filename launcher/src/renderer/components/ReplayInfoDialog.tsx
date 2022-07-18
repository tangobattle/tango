import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { app } from "@electron/remote";
import CloseIcon from "@mui/icons-material/Close";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { spawn } from "../../process";
import { ReplayInfo } from "../../replay";
import { FAMILY_BY_ROM_NAME } from "../../rom";
import { Editor, editorClassForGameFamily } from "../../saveedit";
import { useConfig } from "./ConfigContext";
import { AllowEdits as AllowFolderEdits } from "./FolderViewer";
import SaveViewer from "./SaveViewer";
import Spinner from "./Spinner";

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
  const { config } = useConfig();
  const { i18n } = useTranslation();
  const dateFormat = new Intl.DateTimeFormat(i18n.resolvedLanguage, {
    dateStyle: "medium",
    timeStyle: "medium",
  });

  React.useEffect(() => {
    (async () => {
      const proc = spawn(
        app,
        "replaydump",
        [path.join(config.paths.replays, filename), "ewram"],
        {
          env: {
            ...process.env,
            RUST_LOG: config.rustLogFilter,
            RUST_BACKTRACE: "1",
          },
        }
      );

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
      const Editor = editorClassForGameFamily(
        FAMILY_BY_ROM_NAME[replayInfo.metadata.localSide!.gameInfo!.rom]
      );

      setEditor(
        new Editor(
          new Uint8Array(buf).buffer,
          replayInfo.metadata.localSide!.gameInfo!.rom
        )
      );
    })();
  }, [config, filename, replayInfo]);

  return (
    <Stack
      key={filename}
      sx={{
        width: "100%",
        height: "100%",
        bgcolor: "background.paper",
      }}
      direction="column"
    >
      <Stack direction="row" sx={{ pt: 1, px: 1, alignItems: "start" }}>
        <Box>
          <Typography variant="h6" component="h2" sx={{ px: 1 }}>
            <Trans
              i18nKey="replays:replay-title"
              values={{
                formattedDate: dateFormat.format(
                  new Date(replayInfo.metadata.ts)
                ),
                nickname: replayInfo.metadata.remoteSide!.nickname,
                linkCode: replayInfo.metadata.linkCode,
              }}
            />
            <br />
            <small>{dateFormat.format(new Date(replayInfo.metadata.ts))}</small>
          </Typography>
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
          <SaveViewer
            editor={editor}
            allowFolderEdits={AllowFolderEdits.None}
          />
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
            <Spinner />
          </Box>
        )}
      </Box>
    </Stack>
  );
}

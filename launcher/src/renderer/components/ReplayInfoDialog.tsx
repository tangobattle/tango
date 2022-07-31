import { readFile } from "fs/promises";
import { merge } from "lodash-es";
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

import applyBPS from "../../bps";
import { spawn } from "../../process";
import { ReplayInfo } from "../../replay";
import { FAMILY_BY_ROM_NAME, getROMInfo, KNOWN_ROM_FAMILIES } from "../../rom";
import { Editor, editorClassForGameFamily } from "../../saveedit";
import { useGetPatchPath, useGetROMPath } from "../hooks";
import { useConfig } from "./ConfigContext";
import { usePatches } from "./PatchesContext";
import SaveViewer from "./saveedit";
import { AllowEdits as AllowFolderEdits } from "./saveedit/FolderViewer";
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
  const getROMPath = useGetROMPath();
  const getPatchPath = useGetPatchPath();
  const { patches } = usePatches();

  const romPath = getROMPath(replayInfo.metadata.localSide!.gameInfo!.rom);
  const patchPath =
    replayInfo.metadata.localSide!.gameInfo!.patch != null
      ? getPatchPath(
          replayInfo.metadata.localSide!.gameInfo!.rom,
          replayInfo.metadata.localSide!.gameInfo!.patch
        )
      : null;

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

      const originalROM = await readFile(romPath!);
      let rom = originalROM;
      if (patchPath != null) {
        rom = Buffer.from(
          applyBPS(rom, new Uint8Array(await readFile(patchPath)))
        );
      }
      const romInfo = getROMInfo(originalROM.buffer);

      const patch = replayInfo.metadata.localSide!.gameInfo!.patch;

      const saveeditInfo = merge(
        {},
        KNOWN_ROM_FAMILIES[
          FAMILY_BY_ROM_NAME[replayInfo.metadata.localSide!.gameInfo!.rom]
        ].versions[replayInfo.metadata.localSide!.gameInfo!.rom].revisions[
          romInfo.revision
        ].saveedit,
        patch != null && patches[patch.name] != null
          ? patches[patch.name].versions[patch.version].saveeditOverrides
          : undefined
      );

      setEditor(
        new Editor(
          new Uint8Array(buf).buffer,
          new Uint8Array(rom).buffer,
          saveeditInfo
        )
      );
    })();
  }, [config, filename, replayInfo, romPath, patchPath, patches]);

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
      <Box flexGrow={1} sx={{ display: "flex", py: 1 }}>
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

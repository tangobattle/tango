import { opendir } from "fs/promises";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import { app, BrowserWindow, dialog, shell } from "@electron/remote";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import VideoFileOutlinedIcon from "@mui/icons-material/VideoFileOutlined";
import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import IconButton from "@mui/material/IconButton";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";

import { findPatchVersion } from "../../../patchinfo";
import { getPatchesPath, getReplaysPath, getROMsPath } from "../../../paths";
import { readReplayMetadata, ReplayInfo } from "../../../replay";
import { usePatches } from "../PatchesContext";
import ReplaydumpSupervisor from "../ReplaydumpSupervisor";
import ReplayviewSupervisor from "../ReplayviewSupervisor";
import { useROMs } from "../ROMsContext";

async function* walk(dir: string, root?: string): AsyncIterable<string> {
  if (root == null) {
    root = dir;
  }
  for await (const d of await opendir(dir)) {
    const entry = path.join(dir, d.name);
    if (d.isDirectory()) {
      yield* await walk(entry, root);
    } else if (d.isFile()) {
      yield path.relative(root, entry);
    }
  }
}

function ReplayItem({
  ListChildProps: { index, style },
  onDumpClick,
  onPlayClick,
  replay,
}: {
  ListChildProps: ListChildComponentProps;
  onDumpClick: () => void;
  onPlayClick: () => void;
  replay: {
    name: string;
    info: ReplayInfo;
    resolvedPatchVersion: string | null;
  };
}) {
  const { i18n } = useTranslation();
  const dateFormat = new Intl.DateTimeFormat(i18n.resolvedLanguage, {
    dateStyle: "medium",
    timeStyle: "medium",
  });

  const { roms } = useROMs();

  const unavailable =
    roms[replay.info.rom] == null ||
    (replay.resolvedPatchVersion == null && replay.info.patch != null);

  return (
    <ListItem
      style={style}
      key={index}
      sx={{ userSelect: "none" }}
      secondaryAction={
        <Stack direction="row">
          <Tooltip title={<Trans i18nKey="replays:show-file" />}>
            <IconButton
              onClick={() => {
                shell.showItemInFolder(
                  path.join(getReplaysPath(app), replay.name)
                );
              }}
            >
              <FolderOpenIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title={<Trans i18nKey="replays:export-video" />}>
            <IconButton
              disabled={unavailable}
              onClick={() => {
                onDumpClick();
              }}
            >
              <VideoFileOutlinedIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title={<Trans i18nKey="replays:play" />}>
            <IconButton
              disabled={unavailable}
              onClick={() => {
                onPlayClick();
              }}
            >
              <PlayArrowIcon />
            </IconButton>
          </Tooltip>
        </Stack>
      }
    >
      <ListItemText
        primary={dateFormat.format(new Date(replay.info.ts))}
        secondary={<>{replay.name}</>}
      />
    </ListItem>
  );
}

interface LoadedReplay {
  name: string;
  info: ReplayInfo;
  resolvedPatchVersion: string | null;
}

export default function ReplaysPane({ active }: { active: boolean }) {
  const { patches } = usePatches();
  const { roms } = useROMs();

  const [replays, setReplays] = React.useState<
    | { name: string; info: ReplayInfo; resolvedPatchVersion: string | null }[]
    | null
  >(null);

  const [dumpingReplay, setDumpingReplay] = React.useState<{
    replay: LoadedReplay;
    outPath: string;
    done: boolean;
  } | null>(null);

  const [viewingReplay, setViewingReplay] = React.useState<LoadedReplay | null>(
    null
  );

  React.useEffect(() => {
    if (!active) {
      setReplays(null);
      return;
    }

    (async () => {
      const replays = [];
      try {
        for await (const filename of walk(getReplaysPath(app))) {
          let replayInfo = null;
          try {
            replayInfo = await readReplayMetadata(
              path.join(getReplaysPath(app), filename)
            );
          } catch (e) {
            console.error("failed to get replay data for %s:", filename, e);
          }
          if (replayInfo == null) {
            continue;
          }
          replays.push({
            name: filename,
            info: replayInfo,
            resolvedPatchVersion:
              replayInfo.patch != null && patches[replayInfo.patch.name] != null
                ? findPatchVersion(
                    patches[replayInfo.patch.name],
                    replayInfo.patch.version
                  )
                : null,
          });
        }
      } catch (e) {
        console.error("failed to get replays:", e);
      }
      replays.sort(({ name: name1 }, { name: name2 }) => {
        return name1 < name2 ? -1 : name1 > name2 ? 1 : 0;
      });
      replays.reverse();
      setReplays(replays);
    })();
  }, [active, patches]);

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        display: active ? "block" : "none",
      }}
    >
      {replays != null ? (
        <AutoSizer>
          {({ height, width }) => (
            <FixedSizeList
              height={height}
              width={width}
              itemCount={replays.length}
              itemSize={60}
            >
              {(props) => (
                <ReplayItem
                  ListChildProps={props}
                  replay={replays[props.index]}
                  onDumpClick={() => {
                    const replay = replays[props.index];
                    const fn = dialog.showSaveDialogSync(
                      BrowserWindow.getFocusedWindow()!,
                      {
                        defaultPath: path.join(
                          getReplaysPath(app),
                          replay.name.replace(/\.[^/.]+$/, "")
                        ),
                        filters: [{ name: "MP4", extensions: ["mp4"] }],
                      }
                    );
                    setDumpingReplay(
                      fn != null
                        ? {
                            replay: replay,
                            outPath: fn,
                            done: false,
                          }
                        : null
                    );
                  }}
                  onPlayClick={() => {
                    setViewingReplay(replays[props.index]);
                  }}
                />
              )}
            </FixedSizeList>
          )}
        </AutoSizer>
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
      {viewingReplay != null ? (
        <ReplayviewSupervisor
          romPath={path.join(getROMsPath(app), roms[viewingReplay.info.rom])}
          patchPath={
            viewingReplay.resolvedPatchVersion != null
              ? path.join(
                  getPatchesPath(app),
                  viewingReplay.info.patch!.name,
                  `v${viewingReplay.resolvedPatchVersion}.${
                    patches[viewingReplay.info.patch!.name]!.versions[
                      viewingReplay.resolvedPatchVersion
                    ]!.format
                  }`
                )
              : undefined
          }
          replayPath={path.join(getReplaysPath(app), viewingReplay.name)}
          onExit={() => {
            setViewingReplay(null);
          }}
        />
      ) : null}
      {dumpingReplay != null ? (
        <ReplaydumpSupervisor
          romPath={path.join(
            getROMsPath(app),
            roms[dumpingReplay.replay.info.rom]
          )}
          patchPath={
            dumpingReplay.replay.resolvedPatchVersion != null
              ? path.join(
                  getPatchesPath(app),
                  dumpingReplay.replay.info.patch!.name,
                  `v${dumpingReplay.replay.resolvedPatchVersion}.${
                    patches[dumpingReplay.replay.info.patch!.name]!.versions[
                      dumpingReplay.replay.resolvedPatchVersion
                    ]!.format
                  }`
                )
              : undefined
          }
          replayPath={path.join(getReplaysPath(app), dumpingReplay.replay.name)}
          outPath={dumpingReplay.outPath}
          onExit={() => {
            setDumpingReplay(null);
          }}
        />
      ) : null}
    </Box>
  );
}

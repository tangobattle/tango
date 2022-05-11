import { opendir } from "fs/promises";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import { app, BrowserWindow, dialog, shell } from "@electron/remote";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import InfoOutlinedIcon from "@mui/icons-material/InfoOutlined";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import SlowMotionVideoOutlinedIcon from "@mui/icons-material/SlowMotionVideoOutlined";
import VideoFileOutlinedIcon from "@mui/icons-material/VideoFileOutlined";
import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import IconButton from "@mui/material/IconButton";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { findPatchVersion } from "../../../patch";
import { getReplaysPath } from "../../../paths";
import { readReplayMetadata, ReplayInfo } from "../../../replay";
import { usePatches } from "../PatchesContext";
import ReplaydumpSupervisor from "../ReplaydumpSupervisor";
import ReplayInfoDialog from "../ReplayInfoDialog";
import ReplayviewSupervisor from "../ReplayviewSupervisor";
import { useROMs } from "../ROMsContext";

async function* walk(dir: string, root?: string): AsyncIterable<string> {
  if (root == null) {
    root = dir;
  }
  for await (const d of await opendir(dir)) {
    const entry = path.join(dir, d.name);
    if (d.isDirectory()) {
      yield* walk(entry, root);
    } else if (d.isFile()) {
      yield path.relative(root, entry);
    }
  }
}

function ReplayItem({
  ListChildProps: { index, style },
  onInfoClick,
  onDumpClick,
  onPlayClick,
  replay,
}: {
  ListChildProps: ListChildComponentProps;
  onInfoClick: () => void;
  onDumpClick: () => void;
  onPlayClick: () => void;
  replay: LoadedReplay;
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
          <Tooltip title={<Trans i18nKey="replays:show-info" />}>
            <IconButton
              onClick={() => {
                onInfoClick();
              }}
            >
              <InfoOutlinedIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title={<Trans i18nKey="replays:show-file" />}>
            <IconButton
              onClick={() => {
                shell.showItemInFolder(
                  path.join(getReplaysPath(app), replay.filename)
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
        primary={
          replay.info.linkCode != null ? (
            <>
              <Trans
                i18nKey="replays:replay-title"
                values={{
                  formattedDate: dateFormat.format(new Date(replay.info.ts)),
                  nickname: replay.info.remote!.nickname,
                  linkCode: replay.info.linkCode,
                }}
              />{" "}
              <small>{dateFormat.format(new Date(replay.info.ts))}</small>
            </>
          ) : (
            <>{dateFormat.format(new Date(replay.info.ts))}</>
          )
        }
        secondary={<>{replay.filename}</>}
      />
    </ListItem>
  );
}

interface LoadedReplay {
  filename: string;
  info: ReplayInfo;
  resolvedPatchVersion: string | null;
}

export default function ReplaysPane({ active }: { active: boolean }) {
  const { patches } = usePatches();

  const [replays, setReplays] = React.useState<LoadedReplay[] | null>(null);

  const [dumpingReplay, setDumpingReplay] = React.useState<{
    replay: LoadedReplay;
    outPath: string;
    done: boolean;
  } | null>(null);

  const [viewingReplay, setViewingReplay] = React.useState<LoadedReplay | null>(
    null
  );

  const [infoDialogReplay, setInfoDialogReplay] =
    React.useState<LoadedReplay | null>(null);

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
            console.error("failed to get replay data", filename, e);
          }
          if (replayInfo == null) {
            continue;
          }
          replays.push({
            filename,
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
      replays.sort(({ filename: name1 }, { filename: name2 }) => {
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
        display: active ? "flex" : "none",
      }}
    >
      {replays != null ? (
        replays.length > 0 ? (
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
                    onInfoClick={() => {
                      setInfoDialogReplay(replays[props.index]);
                    }}
                    onDumpClick={() => {
                      const replay = replays[props.index];
                      const fn = dialog.showSaveDialogSync(
                        BrowserWindow.getFocusedWindow()!,
                        {
                          defaultPath: path.join(
                            getReplaysPath(app),
                            replay.filename.replace(/\.[^/.]+$/, "")
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
            flexGrow={1}
            display="flex"
            justifyContent="center"
            alignItems="center"
            sx={{ userSelect: "none", color: "text.disabled" }}
          >
            <Stack alignItems="center" spacing={1}>
              <SlowMotionVideoOutlinedIcon sx={{ fontSize: "4rem" }} />
              <Typography variant="h6">
                <Trans i18nKey="replays:no-replays" />
              </Typography>
            </Stack>
          </Box>
        )
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
          romName={path.join(viewingReplay.info.rom)}
          patch={
            viewingReplay.resolvedPatchVersion != null
              ? {
                  name: viewingReplay.info.patch!.name,
                  version: viewingReplay.resolvedPatchVersion,
                }
              : undefined
          }
          replayPath={path.join(getReplaysPath(app), viewingReplay.filename)}
          onExit={() => {
            setViewingReplay(null);
          }}
        />
      ) : null}
      {dumpingReplay != null ? (
        <ReplaydumpSupervisor
          romName={path.join(dumpingReplay.replay.info.rom)}
          patch={
            dumpingReplay.replay.resolvedPatchVersion != null
              ? {
                  name: dumpingReplay.replay.info.patch!.name,
                  version: dumpingReplay.replay.resolvedPatchVersion,
                }
              : undefined
          }
          replayPath={path.join(
            getReplaysPath(app),
            dumpingReplay.replay.filename
          )}
          outPath={dumpingReplay.outPath}
          onExit={() => {
            setDumpingReplay(null);
          }}
        />
      ) : null}
      {infoDialogReplay != null ? (
        <ReplayInfoDialog
          filename={infoDialogReplay.filename}
          replayInfo={infoDialogReplay.info}
          onClose={() => {
            setInfoDialogReplay(null);
          }}
        />
      ) : null}
    </Box>
  );
}

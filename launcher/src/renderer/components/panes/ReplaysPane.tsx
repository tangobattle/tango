import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import { dialog, getCurrentWindow, shell } from "@electron/remote";
import CloseIcon from "@mui/icons-material/Close";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import InfoOutlinedIcon from "@mui/icons-material/InfoOutlined";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import SlowMotionVideoOutlinedIcon from "@mui/icons-material/SlowMotionVideoOutlined";
import VideoFileOutlinedIcon from "@mui/icons-material/VideoFileOutlined";
import WarningIcon from "@mui/icons-material/Warning";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Modal from "@mui/material/Modal";
import Slide from "@mui/material/Slide";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { walk } from "../../../fsutil";
import { findPatchVersion } from "../../../patch";
import { readReplayMetadata, ReplayInfo } from "../../../replay";
import { useConfig } from "../ConfigContext";
import { usePatches } from "../PatchesContext";
import ReplaydumpSupervisor from "../ReplaydumpSupervisor";
import ReplayInfoDialog from "../ReplayInfoDialog";
import ReplayviewSupervisor from "../ReplayviewSupervisor";
import { useROMs } from "../ROMsContext";

function ReplayItem({
  ListChildProps: { style },
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

  const { config } = useConfig();
  const { roms } = useROMs();

  const unavailable =
    roms[replay.info.metadata.localSide!.gameInfo!.rom] == null ||
    (replay.resolvedPatchVersion == null &&
      replay.info.metadata.localSide!.gameInfo!.patch != null);

  return (
    <ListItem
      dense
      style={style}
      key={replay.filename}
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
                  path.join(config.paths.replays, replay.filename)
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
          <>
            {!replay.info.isComplete ? (
              <Tooltip title={<Trans i18nKey="replays:incomplete" />}>
                <WarningIcon
                  color="warning"
                  sx={{
                    fontSize: "1em",
                    verticalAlign: "middle",
                  }}
                />
              </Tooltip>
            ) : null}{" "}
            {replay.info.metadata.linkCode != null ? (
              <>
                <Trans
                  i18nKey="replays:replay-title"
                  values={{
                    formattedDate: dateFormat.format(
                      new Date(replay.info.metadata.ts)
                    ),
                    nickname: replay.info.metadata.remoteSide!.nickname,
                    linkCode: replay.info.metadata.linkCode,
                  }}
                />{" "}
                <small>
                  {dateFormat.format(new Date(replay.info.metadata.ts))}
                </small>
              </>
            ) : (
              <>{dateFormat.format(new Date(replay.info.metadata.ts))}</>
            )}
          </>
        }
        primaryTypographyProps={{
          sx: {
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          },
        }}
        secondary={<>{replay.filename}</>}
        secondaryTypographyProps={{
          sx: {
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          },
        }}
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
  const { config } = useConfig();

  const [replays, setReplays] = React.useState<LoadedReplay[] | null>(null);

  const [dumpingReplay, setDumpingReplay] = React.useState<{
    replay: LoadedReplay;
    outPath: string;
    scaleFactor: number;
    state: "confirm" | "in-progress";
  } | null>(null);

  const [viewingReplay, setViewingReplay] = React.useState<LoadedReplay | null>(
    null
  );

  const [infoDialogReplay, setInfoDialogReplay] =
    React.useState<LoadedReplay | null>(null);
  const [infoDialogOpen, setInfoDialogOpen] = React.useState(false);

  React.useEffect(() => {
    if (!active) {
      setReplays(null);
      return;
    }

    (async () => {
      const replays = [];
      try {
        for await (const filename of walk(config.paths.replays)) {
          let replayInfo = null;
          try {
            replayInfo = await readReplayMetadata(
              path.join(config.paths.replays, filename)
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
              replayInfo.metadata.localSide!.gameInfo!.patch != null &&
              patches[replayInfo.metadata.localSide!.gameInfo!.patch.name] !=
                null
                ? findPatchVersion(
                    patches[
                      replayInfo.metadata.localSide!.gameInfo!.patch.name
                    ],
                    replayInfo.metadata.localSide!.gameInfo!.patch.version
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
  }, [active, patches, config.paths.replays]);

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        overflow: "hidden",
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
                      setInfoDialogOpen(true);
                    }}
                    onDumpClick={() => {
                      const replay = replays[props.index];
                      const fn = dialog.showSaveDialogSync(getCurrentWindow(), {
                        defaultPath: path.join(
                          config.paths.replays,
                          replay.filename.replace(/\.[^/.]+$/, "")
                        ),
                        filters: [{ name: "MP4", extensions: ["mp4"] }],
                      });
                      setDumpingReplay(
                        fn != null
                          ? {
                              replay: replay,
                              outPath: fn,
                              scaleFactor: 5,
                              state: "confirm",
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
          romName={path.join(
            viewingReplay.info.metadata.localSide!.gameInfo!.rom
          )}
          patch={
            viewingReplay.resolvedPatchVersion != null
              ? {
                  name: viewingReplay.info.metadata.localSide!.gameInfo!.patch!
                    .name,
                  version: viewingReplay.resolvedPatchVersion,
                }
              : undefined
          }
          replayPath={path.join(config.paths.replays, viewingReplay.filename)}
          onExit={() => {
            setViewingReplay(null);
          }}
        />
      ) : null}
      {dumpingReplay != null ? (
        dumpingReplay.state == "confirm" ? (
          <Modal
            open={true}
            onClose={(_e, _reason) => {
              setDumpingReplay(null);
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
                <Stack
                  spacing={1}
                  flexGrow={1}
                  component="form"
                  onSubmit={(e: any) => {
                    e.preventDefault();
                    setDumpingReplay((dr) => ({
                      ...dr!,
                      state: "in-progress",
                    }));
                  }}
                >
                  <Stack direction="row">
                    <Typography variant="h6" component="h2" sx={{ px: 1 }}>
                      <Trans i18nKey="replays:export-settings" />
                    </Typography>
                    <Tooltip title={<Trans i18nKey="common:close" />}>
                      <IconButton
                        sx={{ ml: "auto" }}
                        onClick={() => {
                          setDumpingReplay(null);
                        }}
                      >
                        <CloseIcon />
                      </IconButton>
                    </Tooltip>
                  </Stack>
                  <Table size="small">
                    <TableBody>
                      <TableRow>
                        <TableCell
                          component="th"
                          sx={{ fontWeight: "bold", whiteSpace: "nowrap" }}
                        >
                          <Trans i18nKey="replays:output-path" />
                        </TableCell>
                        <TableCell sx={{ wordBreak: "break-all" }}>
                          <code>{dumpingReplay.outPath}</code>
                        </TableCell>
                      </TableRow>
                      <TableRow>
                        <TableCell
                          component="th"
                          sx={{ fontWeight: "bold", whiteSpace: "nowrap" }}
                        >
                          <Trans i18nKey="replays:scale-factor" />
                        </TableCell>
                        <TableCell>
                          <TextField
                            variant="standard"
                            type="number"
                            value={dumpingReplay.scaleFactor}
                            onChange={(e) => {
                              let v = parseInt(e.target.value);
                              if (isNaN(v)) {
                                v = 0;
                              }
                              setDumpingReplay((dr) => ({
                                ...dr!,
                                scaleFactor: Math.min(Math.max(v, 1), 10),
                              }));
                            }}
                            InputProps={{
                              inputProps: {
                                min: 1,
                                max: 10,
                                style: { textAlign: "right" },
                              },
                              endAdornment: (
                                <InputAdornment position="end">
                                  <Trans i18nKey="replays:scale-factor-suffix" />
                                </InputAdornment>
                              ),
                            }}
                          />
                        </TableCell>
                      </TableRow>
                    </TableBody>
                  </Table>
                  <Stack direction="row" justifyContent="flex-end">
                    <Button variant="contained" type="submit">
                      <Trans i18nKey="replays:start-export" />
                    </Button>
                  </Stack>
                </Stack>
              </Box>
            </Box>
          </Modal>
        ) : dumpingReplay.state == "in-progress" ? (
          <ReplaydumpSupervisor
            romName={path.join(
              dumpingReplay.replay.info.metadata.localSide!.gameInfo!.rom
            )}
            patch={
              dumpingReplay.replay.resolvedPatchVersion != null
                ? {
                    name: dumpingReplay.replay.info.metadata.localSide!
                      .gameInfo!.patch!.name,
                    version: dumpingReplay.replay.resolvedPatchVersion,
                  }
                : undefined
            }
            replayPath={path.join(
              config.paths.replays,
              dumpingReplay.replay.filename
            )}
            outPath={dumpingReplay.outPath}
            scaleFactor={dumpingReplay.scaleFactor}
            onExit={() => {
              setDumpingReplay(null);
            }}
          />
        ) : null
      ) : null}
      <Modal
        open={infoDialogOpen}
        onClose={(_e, _reason) => {
          setInfoDialogOpen(false);
        }}
      >
        <Slide
          in={infoDialogOpen}
          direction="up"
          unmountOnExit
          onExited={() => {
            setInfoDialogReplay(null);
          }}
        >
          <Box>
            {infoDialogReplay != null ? (
              <ReplayInfoDialog
                filename={infoDialogReplay.filename}
                replayInfo={infoDialogReplay.info}
                onClose={() => {
                  setInfoDialogOpen(false);
                }}
              />
            ) : null}
          </Box>
        </Slide>
      </Modal>
    </Box>
  );
}

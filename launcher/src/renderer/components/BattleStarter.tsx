import { readFile } from "fs/promises";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import semver from "semver";

import { app, shell } from "@electron/remote";
import { ConnectingAirportsOutlined } from "@mui/icons-material";
import CheckIcon from "@mui/icons-material/Check";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import SportsMmaIcon from "@mui/icons-material/SportsMma";
import StopIcon from "@mui/icons-material/Stop";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Checkbox from "@mui/material/Checkbox";
import CircularProgress from "@mui/material/CircularProgress";
import Collapse from "@mui/material/Collapse";
import Divider from "@mui/material/Divider";
import FormControl from "@mui/material/FormControl";
import FormControlLabel from "@mui/material/FormControlLabel";
import FormGroup from "@mui/material/FormGroup";
import IconButton from "@mui/material/IconButton";
import InputLabel from "@mui/material/InputLabel";
import ListItemText from "@mui/material/ListItemText";
import ListSubheader from "@mui/material/ListSubheader";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import * as ipc from "../../ipc";
import { getBasePath, getSavesPath } from "../../paths";
import {
    FromCoreMessage_StateIndication_State, ToCoreMessage_StartRequest_MatchSettings
} from "../../protos/ipc";
import { GameInfo, Message, SetSettings } from "../../protos/lobby";
import { KNOWN_ROMS } from "../../rom";
import { Editor } from "../../saveedit/bn6";
import { useROMPath } from "../hooks";
import { useConfig } from "./ConfigContext";
import { usePatches } from "./PatchesContext";
import { useROMs } from "./ROMsContext";
import { useSaves } from "./SavesContext";
import SaveViewer from "./SaveViewer";

const MATCH_TYPES = ["single", "triple"];

function defaultMatchSettings(
  nickname: string,
  gameInfo: GameInfo | null
): SetSettings {
  return {
    nickname,
    inputDelay: 3,
    matchType: 1,
    gameInfo: gameInfo ?? undefined,
    availableGames: [],
  };
}

function useGameTitle(gameInfo: GameInfo | null) {
  const { patches } = usePatches();
  const { i18n } = useTranslation();

  if (gameInfo == null) {
    return null;
  }

  return `${KNOWN_ROMS[gameInfo.rom].title[i18n.resolvedLanguage]}
      ${
        gameInfo.patch != null
          ? ` + ${patches[gameInfo.patch.name].title} v${
              gameInfo.patch.version
            }`
          : ""
      }`;
}

interface PendingState {
  settings: SetSettings;
  ready: boolean;
}

export default function BattleStarter({
  saveName,
  patch,
  onExit,
}: {
  saveName: string | null;
  patch: { name: string; version: string } | null;
  onExit: () => void;
}) {
  const { saves } = useSaves();

  const { config } = useConfig();
  const configRef = React.useRef(config);

  const [linkCode, setLinkCode] = React.useState("");
  const [state, setState] =
    React.useState<FromCoreMessage_StateIndication_State>(
      FromCoreMessage_StateIndication_State.UNKNOWN
    );
  const [pendingStates, setPendingStates] = React.useState<{
    core: ipc.Core;
    abortController: AbortController;
    own: PendingState | null;
    opponent: PendingState | null;
  } | null>(null);

  const gameInfo = React.useMemo(
    () =>
      saveName != null
        ? {
            rom: saves[saveName].romName,
            patch: patch ?? undefined,
          }
        : null ?? undefined,
    [saveName, saves, patch]
  );

  React.useEffect(() => {
    setPendingStates((pendingStates) =>
      pendingStates != null && pendingStates.own != null
        ? {
            ...pendingStates,
            own: {
              ...pendingStates.own,
              settings: {
                ...pendingStates.own.settings,
                gameInfo,
              },
            },
          }
        : pendingStates
    );
  }, [gameInfo]);

  const myPendingSettings = pendingStates?.own?.settings;
  const ready = pendingStates?.own?.ready ?? false;

  React.useEffect(() => {
    if (myPendingSettings == null) {
      return;
    }
    pendingStates!.core.send({
      smuggleReq: {
        data: Message.encode({
          setSettings: myPendingSettings,
          commit: undefined,
          uncommit: undefined,
          chunk: undefined,
        }).finish(),
      },
      startReq: undefined,
    });
  }, [myPendingSettings, pendingStates]);

  React.useEffect(() => {
    // TODO: Send state changes.
  }, [myPendingSettings, ready]);

  const ownGameTitle = useGameTitle(gameInfo ?? null);
  const opponentGameTitle = useGameTitle(
    pendingStates?.opponent?.settings.gameInfo ?? null
  );

  const ownROMPath = useROMPath(gameInfo?.rom ?? null);

  return (
    <Stack>
      <Collapse
        in={
          pendingStates != null &&
          pendingStates.own != null &&
          pendingStates.opponent != null
        }
      >
        <Divider />
        <Box
          sx={{
            px: 1,
            pb: 1,
          }}
        >
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell></TableCell>
                <TableCell sx={{ width: "40%", fontWeight: "bold" }}>
                  <Trans i18nKey="play:own-side" />
                </TableCell>
                <TableCell sx={{ width: "40%", fontWeight: "bold" }}>
                  {pendingStates?.opponent?.settings.nickname ?? ""}
                </TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              <TableRow>
                <TableCell component="th" sx={{ fontWeight: "bold" }}>
                  <Trans i18nKey="play:game" />
                </TableCell>
                <TableCell>{ownGameTitle}</TableCell>
                <TableCell>{opponentGameTitle}</TableCell>
              </TableRow>
              <TableRow>
                <TableCell component="th" sx={{ fontWeight: "bold" }}>
                  <Trans i18nKey="play:match-type" />
                </TableCell>
                <TableCell>
                  <Select
                    variant="standard"
                    size="small"
                    value={pendingStates?.own?.settings.matchType ?? 0}
                    onChange={(e) => {
                      setPendingStates((pendingStates) => ({
                        ...pendingStates!,
                        own: {
                          ...pendingStates!.own!,
                          settings: {
                            ...pendingStates!.own!.settings,
                            matchType: e.target.value as number,
                          },
                        },
                      }));
                    }}
                    disabled={false}
                  >
                    {MATCH_TYPES.map((_v, k) => (
                      <MenuItem key={k} value={k}>
                        {k == 0 ? (
                          <Trans i18nKey="play:match-type.single" />
                        ) : k == 1 ? (
                          <Trans i18nKey="play:match-type.triple" />
                        ) : null}
                      </MenuItem>
                    ))}
                  </Select>
                </TableCell>
                <TableCell>
                  {pendingStates?.opponent?.settings.matchType == 0 ? (
                    <Trans i18nKey="play:match-type.single" />
                  ) : pendingStates?.opponent?.settings.matchType == 1 ? (
                    <Trans i18nKey="play:match-type.triple" />
                  ) : null}
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell component="th" sx={{ fontWeight: "bold" }}>
                  <Trans i18nKey="play:input-delay" />
                </TableCell>
                <TableCell>
                  <TextField
                    variant="standard"
                    type="number"
                    value={pendingStates?.own?.settings.inputDelay ?? 0}
                    onChange={(e) => {
                      setPendingStates((pendingStates) => ({
                        ...pendingStates!,
                        own: {
                          ...pendingStates!.own!,
                          settings: {
                            ...pendingStates!.own!.settings,
                            inputDelay: parseInt(e.target.value),
                          },
                        },
                      }));
                    }}
                    InputProps={{ inputProps: { min: 3, max: 10 } }}
                  />
                </TableCell>
                <TableCell>
                  {pendingStates?.opponent?.settings.inputDelay ?? 0}
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </Box>
      </Collapse>
      <Stack
        flexGrow={0}
        flexShrink={0}
        direction="row"
        justifyContent="flex-end"
        spacing={1}
        sx={{ px: 1, mb: 0 }}
        component="form"
        onSubmit={(e: any) => {
          e.preventDefault();

          const abortController = new AbortController();

          const core = new ipc.Core(
            configRef.current.keymapping,
            configRef.current.signalingConnectAddr,
            configRef.current.iceServers,
            linkCode != "" ? linkCode : null,
            {
              env: {
                WGPU_BACKEND:
                  configRef.current.wgpuBackend != null
                    ? configRef.current.wgpuBackend
                    : undefined,
                RUST_LOG: configRef.current.rustLogFilter,
                RUST_BACKTRACE: "1",
              },
              signal: abortController.signal,
            }
          );

          setPendingStates({
            core,
            abortController,
            own: null,
            opponent: null,
          });

          (async () => {
            // eslint-disable-next-line no-constant-condition
            while (true) {
              const msg = await core.receive();
              if (msg == null) {
                throw "unexpected eof from core";
              }
              if (msg.stateInd != null) {
                setState(msg.stateInd.state);
                if (
                  msg.stateInd.state ==
                  FromCoreMessage_StateIndication_State.STARTING
                ) {
                  break;
                }
              }
            }

            if (linkCode == "") {
              // No link code to worry about, just start the game with no settings.
              await core.send({
                smuggleReq: undefined,
                startReq: {
                  romPath: ownROMPath!,
                  savePath: path.join(getSavesPath(app), saveName!),
                  windowTitle: ownGameTitle!,
                  settings: undefined,
                },
              });
            } else {
              // Need to negotiate settings with the opponent.
              const myPendingSettings = defaultMatchSettings(
                config.nickname,
                gameInfo!
              );
              setPendingStates((pendingStates) => ({
                ...pendingStates!,
                opponent: null,
                own: { ready: false, settings: myPendingSettings },
              }));
              await core.send({
                smuggleReq: {
                  data: Message.encode({
                    setSettings: myPendingSettings,
                    commit: undefined,
                    uncommit: undefined,
                    chunk: undefined,
                  }).finish(),
                },
                startReq: undefined,
              });

              // eslint-disable-next-line no-constant-condition
              while (true) {
                const msg = await core.receive();
                if (msg == null) {
                  return;
                }

                if (msg.smuggleInd == null) {
                  throw "expected smuggle indication";
                }

                const lobbyMsg = Message.decode(msg.smuggleInd.data);
                if (lobbyMsg.setSettings == null) {
                  throw "expected lobby set settings";
                }

                setPendingStates((pendingStates) => ({
                  ...pendingStates!,
                  opponent: { ready: false, settings: lobbyMsg.setSettings! },
                }));
              }
            }

            // eslint-disable-next-line no-constant-condition
            while (true) {
              const msg = await core.receive();
              if (msg == null) {
                break;
              }

              if (msg.stateInd != null) {
                setState(msg.stateInd.state);
              }
            }
          })()
            .catch((e) => {
              console.error(e);
            })
            .finally(() => {
              if (abortController != null) {
                abortController.abort();
              }
              setPendingStates(null);
              onExit();
            });
        }}
      >
        <Box flexGrow={1} flexShrink={0}>
          <TextField
            disabled={pendingStates != null}
            size="small"
            label={<Trans i18nKey={"play:link-code"} />}
            value={linkCode}
            onChange={(e) => {
              setLinkCode(
                e.target.value
                  .toLowerCase()
                  .replace(/[^a-z0-9]/g, "")
                  .slice(0, 40)
              );
            }}
            InputProps={{
              endAdornment:
                pendingStates != null ? (
                  <Stack
                    spacing={1}
                    direction="row"
                    sx={{ alignItems: "center" }}
                  >
                    <Typography sx={{ whiteSpace: "nowrap" }}>
                      {state ==
                      FromCoreMessage_StateIndication_State.RUNNING ? (
                        <Trans i18nKey="supervisor:status.running" />
                      ) : state ==
                        FromCoreMessage_StateIndication_State.WAITING ? (
                        <Trans i18nKey="supervisor:status.waiting" />
                      ) : state ==
                        FromCoreMessage_StateIndication_State.CONNECTING ? (
                        <Trans i18nKey="supervisor:status.connecting" />
                      ) : state ==
                        FromCoreMessage_StateIndication_State.STARTING ? (
                        pendingStates.own != null ? null : (
                          <Trans i18nKey="supervisor:status.starting" />
                        )
                      ) : (
                        <Trans i18nKey="supervisor:status.unknown" />
                      )}
                    </Typography>
                    <CircularProgress size="1rem" color="inherit" />
                  </Stack>
                ) : null,
            }}
            fullWidth
          />
        </Box>

        {pendingStates == null ? (
          <Button
            type="submit"
            variant="contained"
            startIcon={linkCode != "" ? <SportsMmaIcon /> : <PlayArrowIcon />}
            disabled={
              pendingStates != null || (linkCode == "" && ownROMPath == null)
            }
          >
            {linkCode != "" ? (
              <Trans i18nKey="play:fight" />
            ) : (
              <Trans i18nKey="play:play" />
            )}
          </Button>
        ) : (
          <>
            {pendingStates.own != null && pendingStates.opponent != null ? (
              <>
                <FormGroup>
                  <FormControlLabel
                    control={
                      <Checkbox
                        value={pendingStates.own.ready}
                        onChange={(_e, v) => {
                          setPendingStates((pendingStates) => ({
                            ...pendingStates!,
                            own: {
                              ...pendingStates!.own!,
                              ready: v,
                            },
                          }));
                        }}
                      />
                    }
                    label={<Trans i18nKey={"play:ready"} />}
                  />
                </FormGroup>
              </>
            ) : null}
            <Button
              color="error"
              variant="contained"
              startIcon={<StopIcon />}
              onClick={() => {
                pendingStates.abortController.abort();
              }}
              disabled={false}
            >
              <Trans i18nKey="play:stop" />
            </Button>
          </>
        )}
      </Stack>
    </Stack>
  );
}

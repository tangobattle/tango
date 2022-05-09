import { readFile } from "fs/promises";
import { isEqual } from "lodash-es";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import { usePrevious } from "react-use";
import semver from "semver";
import { SHAKE } from "sha3";
import { promisify } from "util";
import { brotliCompress } from "zlib";

import { app, shell } from "@electron/remote";
import { ConnectingAirportsOutlined } from "@mui/icons-material";
import CheckIcon from "@mui/icons-material/Check";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import SportsMmaIcon from "@mui/icons-material/SportsMma";
import StopIcon from "@mui/icons-material/Stop";
import WarningIcon from "@mui/icons-material/Warning";
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
import { FromCoreMessage_StateIndication_State } from "../../protos/ipc";
import { GameInfo, Message, NegotiatedState, SetSettings } from "../../protos/lobby";
import { KNOWN_ROMS } from "../../rom";
import { Editor } from "../../saveedit/bn6";
import { useROMPath } from "../hooks";
import { useConfig } from "./ConfigContext";
import { usePatches } from "./PatchesContext";
import { useROMs } from "./ROMsContext";
import { useSaves } from "./SavesContext";
import SaveViewer from "./SaveViewer";

const MATCH_TYPES = ["single", "triple"];

function defaultMatchSettings(nickname: string): SetSettings {
  return {
    nickname,
    inputDelay: 3,
    matchType: 1,
    gameInfo: undefined,
    availableGames: [],
  };
}

function useGetGameTitle() {
  const { patches } = usePatches();
  const { i18n } = useTranslation();

  return React.useCallback(
    (gameInfo: GameInfo) =>
      `${KNOWN_ROMS[gameInfo.rom].title[i18n.resolvedLanguage]} ${
        gameInfo.patch != null
          ? ` + ${patches[gameInfo.patch.name].title} v${
              gameInfo.patch.version
            }`
          : ""
      }`,
    [patches, i18n]
  );
}

function gameInfoMatches(g: GameInfo | null, h: GameInfo) {
  if (g == null) {
    return false;
  }

  if (g.rom != h.rom) {
    return false;
  }

  if ((g.patch == null) != (h.patch == null)) {
    return false;
  }

  if (g.patch == null && h.patch == null) {
    return true;
  }

  return g.patch!.name == h.patch!.name && g.patch!.version == h.patch!.version;
}

function useGetAvailableGames() {
  const { patches } = usePatches();
  return React.useCallback(
    (gameInfo: GameInfo) => {
      let netplayCompatibility = KNOWN_ROMS[gameInfo.rom].netplayCompatibility;
      if (gameInfo.patch != null) {
        netplayCompatibility =
          patches[gameInfo.patch.name].versions[gameInfo.patch.version]
            .netplayCompatibility;
      }

      return Array.from(
        (function* () {
          for (const romName of Object.keys(KNOWN_ROMS)) {
            const rom = KNOWN_ROMS[romName];
            if (rom.netplayCompatibility == netplayCompatibility) {
              yield { rom: romName, patch: undefined };
            }
          }

          for (const patchName of Object.keys(patches)) {
            const patch = patches[patchName];
            for (const version of Object.keys(patch.versions)) {
              if (
                patch.versions[version].netplayCompatibility ==
                netplayCompatibility
              ) {
                yield {
                  rom: patch.forROM,
                  patch: { name: patchName, version },
                };
              }
            }
          }
        })()
      );
    },
    [patches]
  );
}

interface PendingState {
  settings: SetSettings;
  commitment: Uint8Array | null;
}

export default function BattleStarter({
  saveName,
  patch,
  onExit,
  onReadyChange,
  onOpponentSettingsChange,
}: {
  saveName: string | null;
  patch: { name: string; version: string } | null;
  onExit: () => void;
  onReadyChange: (ready: boolean) => void;
  onOpponentSettingsChange: (settings: SetSettings) => void;
}) {
  const { saves } = useSaves();

  const { config } = useConfig();
  const configRef = React.useRef(config);

  const getAvailableGames = useGetAvailableGames();

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
  const [changingCommitment, setChangingCommitment] = React.useState(false);

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

  const previousGameInfo = usePrevious(gameInfo);

  const changeLocalPendingState = React.useCallback(
    (settings: SetSettings) => {
      setPendingStates((pendingStates) => ({
        ...pendingStates!,
        own: {
          ...pendingStates!.own!,
          settings,
        },
        opponent: {
          ...pendingStates!.opponent!,
          commitment: null,
        },
      }));

      pendingStates!.core.send({
        smuggleReq: {
          data: Message.encode({
            setSettings: settings,
            commit: undefined,
            uncommit: undefined,
            chunk: undefined,
          }).finish(),
        },
        startReq: undefined,
      });
    },
    [pendingStates]
  );

  React.useEffect(() => {
    if (
      pendingStates != null &&
      pendingStates.own != null &&
      !isEqual(gameInfo, previousGameInfo)
    ) {
      changeLocalPendingState({
        ...pendingStates.own.settings,
        gameInfo,
        availableGames: gameInfo != null ? getAvailableGames(gameInfo) : [],
      });
    }
  }, [
    gameInfo,
    previousGameInfo,
    changeLocalPendingState,
    getAvailableGames,
    pendingStates,
  ]);

  const myPendingState = pendingStates?.own;

  React.useEffect(() => {
    onReadyChange(myPendingState?.commitment != null);
  }, [myPendingState, onReadyChange]);

  const getGameTitle = useGetGameTitle();

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
                  <Trans i18nKey="play:own-side" />{" "}
                  {pendingStates?.own?.commitment != null ? (
                    <CheckCircleIcon
                      color="success"
                      sx={{
                        fontSize: "1em",
                        marginLeft: "4px",
                        verticalAlign: "middle",
                      }}
                    />
                  ) : null}
                </TableCell>
                <TableCell sx={{ width: "40%", fontWeight: "bold" }}>
                  {pendingStates?.opponent?.settings.nickname ?? ""}{" "}
                  {pendingStates?.opponent?.commitment != null ? (
                    <CheckCircleIcon
                      color="success"
                      sx={{
                        fontSize: "1em",
                        marginLeft: "4px",
                        verticalAlign: "middle",
                      }}
                    />
                  ) : null}
                </TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              <TableRow>
                <TableCell component="th" sx={{ fontWeight: "bold" }}>
                  <Trans i18nKey="play:game" />
                </TableCell>
                <TableCell>
                  {gameInfo != null ? (
                    getGameTitle(gameInfo)
                  ) : (
                    <Trans i18nKey="play:no-game-selected" />
                  )}
                </TableCell>
                <TableCell>
                  {pendingStates?.opponent?.settings.gameInfo != null ? (
                    getGameTitle(pendingStates?.opponent?.settings.gameInfo)
                  ) : (
                    <Trans i18nKey="play:no-game-selected" />
                  )}
                </TableCell>
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
                    disabled={pendingStates?.own?.commitment != null}
                    onChange={(e) => {
                      changeLocalPendingState({
                        ...pendingStates!.own!.settings,
                        matchType: e.target.value as number,
                      });
                    }}
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
                  </Select>{" "}
                  {pendingStates?.opponent?.settings.matchType !=
                  pendingStates?.own?.settings.matchType ? (
                    <Tooltip
                      title={<Trans i18nKey="play:mismatching-match-type" />}
                    >
                      <WarningIcon
                        color="warning"
                        sx={{
                          fontSize: "1em",
                          verticalAlign: "middle",
                        }}
                      />
                    </Tooltip>
                  ) : null}
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
                    disabled={pendingStates?.own?.commitment != null}
                    onChange={(e) => {
                      changeLocalPendingState({
                        ...pendingStates!.own!.settings,
                        inputDelay: parseInt(e.target.value),
                      });
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
                  windowTitle: getGameTitle(gameInfo!),
                  settings: undefined,
                },
              });
            } else {
              // Need to negotiate settings with the opponent.
              const myPendingSettings = defaultMatchSettings(config.nickname);
              myPendingSettings.gameInfo = gameInfo;
              myPendingSettings.availableGames =
                gameInfo != null ? getAvailableGames(gameInfo) : [];
              setPendingStates((pendingStates) => ({
                ...pendingStates!,
                opponent: null,
                own: { commitment: null, settings: myPendingSettings },
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
                if (lobbyMsg.uncommit != null) {
                  setPendingStates((pendingStates) => ({
                    ...pendingStates!,
                    opponent: { ...pendingStates!.opponent!, commitment: null },
                  }));
                  continue;
                }

                if (lobbyMsg.commit != null) {
                  setPendingStates((pendingStates) => ({
                    ...pendingStates!,
                    opponent: {
                      ...pendingStates!.opponent!,
                      commitment: lobbyMsg.commit!.commitment,
                    },
                  }));
                  continue;
                }

                if (lobbyMsg.setSettings == null) {
                  throw "expected lobby set settings";
                }

                setPendingStates((pendingStates) => ({
                  ...pendingStates!,
                  opponent: {
                    commitment: null,
                    settings: lobbyMsg.setSettings!,
                  },
                  own: {
                    ...pendingStates!.own!,
                    commitment: null,
                  },
                }));

                onOpponentSettingsChange(lobbyMsg.setSettings);
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
                        checked={pendingStates.own.commitment != null}
                        disabled={
                          saveName == null ||
                          pendingStates.own.settings.matchType !=
                            pendingStates.opponent.settings.matchType ||
                          !pendingStates.opponent.settings.availableGames.some(
                            (g) =>
                              gameInfoMatches(
                                pendingStates.own!.settings.gameInfo ?? null,
                                g
                              )
                          ) ||
                          changingCommitment ||
                          (pendingStates.own.commitment != null &&
                            pendingStates.opponent.commitment != null)
                        }
                        indeterminate={changingCommitment}
                        onChange={(_e, v) => {
                          setChangingCommitment(true);
                          (async () => {
                            let commitment: Uint8Array | null = null;

                            if (v) {
                              const saveData = await readFile(
                                path.join(getSavesPath(app), saveName!)
                              );
                              const nonce = crypto.getRandomValues(
                                new Uint8Array(16)
                              );

                              const state = {
                                nonce,
                                saveData: await promisify(brotliCompress)(
                                  saveData
                                ),
                              };

                              const shake128 = new SHAKE(128);
                              shake128.update(Buffer.from("tango:state:"));
                              shake128.update(
                                Buffer.from(
                                  NegotiatedState.encode(state).finish()
                                )
                              );

                              commitment = new Uint8Array(shake128.digest());
                            }

                            if (commitment != null) {
                              await pendingStates.core.send({
                                smuggleReq: {
                                  data: Message.encode({
                                    setSettings: undefined,
                                    commit: {
                                      commitment,
                                      numChunks: 0, // TODO
                                    },
                                    uncommit: undefined,
                                    chunk: undefined,
                                  }).finish(),
                                },
                                startReq: undefined,
                              });
                            } else {
                              await pendingStates.core.send({
                                smuggleReq: {
                                  data: Message.encode({
                                    setSettings: undefined,
                                    commit: undefined,
                                    uncommit: {},
                                    chunk: undefined,
                                  }).finish(),
                                },
                                startReq: undefined,
                              });
                            }

                            setPendingStates((pendingStates) => ({
                              ...pendingStates!,
                              own: {
                                ...pendingStates!.own!,
                                commitment,
                              },
                            }));
                            setChangingCommitment(false);
                          })();
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

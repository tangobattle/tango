import { timingSafeEqual } from "crypto";
import * as datefns from "date-fns";
import { clipboard } from "electron";
import { readFile, writeFile } from "fs/promises";
import { isEqual } from "lodash-es";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import { usePrevious } from "react-use";
import useStateRef from "react-usestateref";
import { SHAKE } from "sha3";
import { promisify } from "util";
import { brotliCompress, brotliDecompress } from "zlib";

import { app } from "@electron/remote";
import CasinoOutlinedIcon from "@mui/icons-material/CasinoOutlined";
import CheckCircleIcon from "@mui/icons-material/CheckCircle";
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
import FormControlLabel from "@mui/material/FormControlLabel";
import FormGroup from "@mui/material/FormGroup";
import IconButton from "@mui/material/IconButton";
import MenuItem from "@mui/material/MenuItem";
import Modal from "@mui/material/Modal";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Switch from "@mui/material/Switch";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { makeROM } from "../../game";
import * as ipc from "../../ipc";
import { getReplaysPath, getSavesPath } from "../../paths";
import {
    FromCoreMessage_StateIndication_State, ToCoreMessage_StartRequest
} from "../../protos/ipc";
import { GameInfo, Message, NegotiatedState, SetSettings } from "../../protos/lobby";
import randomCode from "../../randomcode";
import { ReplayInfo } from "../../replay";
import { KNOWN_ROMS } from "../../rom";
import * as bn6 from "../../saveedit/bn6";
import { useGetPatchPath, useGetROMPath } from "../hooks";
import { useConfig } from "./ConfigContext";
import CopyButton from "./CopyButton";
import { usePatches } from "./PatchesContext";
import { useROMs } from "./ROMsContext";
import { useSaves } from "./SavesContext";
import SaveViewer from "./SaveViewer";
import { useTempDir } from "./TempDirContext";

const MATCH_TYPES = ["single", "triple"];

function defaultMatchSettings(nickname: string): SetSettings {
  return {
    nickname,
    inputDelay: 3,
    matchType: 1,
    gameInfo: undefined,
    availableGames: [],
    openSetup: false,
  };
}

function useGetGameTitle() {
  const { patches } = usePatches();
  const { i18n } = useTranslation();

  return React.useCallback(
    (gameInfo: GameInfo) =>
      `${KNOWN_ROMS[gameInfo.rom].title[i18n.resolvedLanguage]}${
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
  const { roms } = useROMs();

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
          for (const romName of Object.keys(roms)) {
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
    [patches, roms]
  );
}

function useHasGame() {
  const { roms } = useROMs();
  const { patches } = usePatches();
  return React.useCallback(
    (gameInfo: GameInfo) => {
      return (
        Object.prototype.hasOwnProperty.call(roms, gameInfo.rom) &&
        (gameInfo.patch != null
          ? Object.prototype.hasOwnProperty.call(
              patches,
              gameInfo.patch.name
            ) &&
            Object.prototype.hasOwnProperty.call(
              patches[gameInfo.patch.name].versions,
              gameInfo.patch.version
            )
          : true)
      );
    },
    [roms, patches]
  );
}

function makeCommitment(s: Uint8Array): Uint8Array {
  const shake128 = new SHAKE(128);
  shake128.update(Buffer.from("tango:state:"));
  shake128.update(Buffer.from(s));
  return new Uint8Array(shake128.digest());
}

function GenerateRandomCodeButton({
  onClick,
}: {
  onClick: (e: React.MouseEvent<HTMLButtonElement>, code: string) => void;
}) {
  const [clicked, setClicked] = React.useState(false);

  return (
    <Tooltip
      title={
        clicked ? (
          <Trans i18nKey="common:copied-to-clipboard" />
        ) : (
          <Trans i18nKey="play:generate-random-code" />
        )
      }
    >
      <IconButton
        onClick={(e) => {
          const code = randomCode();
          clipboard.writeText(code);
          onClick(e, code);
          setClicked(true);
          setTimeout(() => {
            setClicked(false);
          }, 1000);
        }}
      >
        <CasinoOutlinedIcon />
      </IconButton>
    </Tooltip>
  );
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
  onOpponentSettingsChange: (settings: SetSettings | null) => void;
}) {
  const { saves } = useSaves();

  const { config } = useConfig();
  const { tempDir } = useTempDir();
  const getROMPath = useGetROMPath();
  const getPatchPath = useGetPatchPath();

  const configRef = React.useRef(config);
  configRef.current = config;

  const saveNameRef = React.useRef(saveName);
  saveNameRef.current = saveName;

  const getAvailableGames = useGetAvailableGames();
  const hasGame = useHasGame();

  const [errorDialogState, setErrorDialogState] = React.useState<{
    stderr: string;
    exitStatus: ipc.ExitStatus;
  } | null>(null);

  const [linkCode, setLinkCode] = React.useState("");
  const [state, setState] =
    React.useState<FromCoreMessage_StateIndication_State>(
      FromCoreMessage_StateIndication_State.UNKNOWN
    );
  const [pendingStates, setPendingStates, pendingStatesRef] = useStateRef<{
    core: ipc.Core;
    abortController: AbortController;
    own: {
      settings: SetSettings;
      negotiatedState: NegotiatedState | null;
    } | null;
    opponent: {
      settings: SetSettings;
      commitment: Uint8Array | null;
    } | null;
  } | null>(null);
  const [changingCommitment, setChangingCommitment] = React.useState(false);
  const [rtt, setRtt] = React.useState<number | null>(null);

  const [openSetupEditor, setOpenSetupEditor] =
    React.useState<bn6.Editor | null>(null);

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
    [pendingStates, setPendingStates]
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
    onReadyChange(myPendingState?.negotiatedState != null);
  }, [myPendingState, onReadyChange]);

  const getGameTitle = useGetGameTitle();

  return (
    <>
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
                    {pendingStates?.own?.negotiatedState != null ? (
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
                    {pendingStates?.opponent?.settings.nickname ?? ""}
                    {rtt != null ? (
                      <Trans
                        i18nKey="play:connection-quality"
                        values={{ rtt: Math.round(rtt / 1000 / 1000) }}
                      />
                    ) : null}
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
                    )}{" "}
                    {!pendingStates?.opponent?.settings.availableGames.some(
                      (g) =>
                        gameInfoMatches(
                          pendingStates?.own?.settings.gameInfo ?? null,
                          g
                        )
                    ) ? (
                      <Tooltip
                        title={<Trans i18nKey="play:unsupported-game" />}
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
                    {pendingStates?.opponent?.settings.gameInfo != null ? (
                      getGameTitle(pendingStates?.opponent?.settings.gameInfo)
                    ) : (
                      <Trans i18nKey="play:no-game-selected" />
                    )}{" "}
                    {pendingStates?.opponent?.settings.gameInfo != null &&
                    !hasGame(pendingStates?.opponent?.settings.gameInfo) ? (
                      <Tooltip
                        title={<Trans i18nKey="play:unavailable-game" />}
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
                      disabled={pendingStates?.own?.negotiatedState != null}
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
                      value={Math.min(
                        Math.max(
                          pendingStates?.own?.settings.inputDelay ?? 0,
                          3
                        ),
                        10
                      )}
                      disabled={pendingStates?.own?.negotiatedState != null}
                      onChange={(e) => {
                        changeLocalPendingState({
                          ...pendingStates!.own!.settings,
                          inputDelay: parseInt(e.target.value),
                        });
                      }}
                      InputProps={{ inputProps: { min: 3, max: 10 } }}
                    />{" "}
                    <Button
                      disabled={rtt == null}
                      size="small"
                      color="primary"
                      variant="outlined"
                      onClick={() => {
                        changeLocalPendingState({
                          ...pendingStates!.own!.settings,
                          inputDelay: Math.min(
                            10,
                            Math.max(
                              3,
                              Math.round(
                                ((rtt! / 1000 / 1000 / 2) * 60) / 1000
                              ) +
                                1 -
                                3
                            )
                          ),
                        });
                      }}
                    >
                      <Trans i18nKey="play:auto-input-delay" />
                    </Button>
                  </TableCell>
                  <TableCell>
                    {pendingStates?.opponent?.settings.inputDelay ?? 0}
                  </TableCell>
                </TableRow>
                <TableRow>
                  <TableCell component="th" sx={{ fontWeight: "bold" }}>
                    <Trans i18nKey="play:open-setup" />
                  </TableCell>
                  <TableCell>
                    <Switch
                      size="small"
                      checked={pendingStates?.own?.settings.openSetup ?? true}
                      onChange={(_e, v) => {
                        changeLocalPendingState({
                          ...pendingStates!.own!.settings,
                          openSetup: v,
                        });
                      }}
                    />
                    {pendingStates?.opponent?.settings.openSetup !=
                    pendingStates?.own?.settings.openSetup ? (
                      <Tooltip
                        title={<Trans i18nKey="play:mismatching-open-setup" />}
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
                    <Switch
                      size="small"
                      checked={
                        pendingStates?.opponent?.settings.openSetup ?? true
                      }
                      disabled={true}
                    />
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
                const outROMPath = path.join(
                  tempDir,
                  `${gameInfo!.rom}${
                    gameInfo!.patch != null
                      ? `+${gameInfo!.patch.name}-v${gameInfo!.patch.version}`
                      : ""
                  }.gba`
                );
                await makeROM(
                  getROMPath(gameInfo!.rom),
                  gameInfo!.patch != null
                    ? getPatchPath(gameInfo!.patch)
                    : null,
                  outROMPath
                );

                await core.send({
                  smuggleReq: undefined,
                  startReq: {
                    romPath: outROMPath,
                    savePath: path.join(getSavesPath(app), saveName!),
                    windowTitle: getGameTitle(gameInfo!),
                    settings: undefined,
                  },
                });
              } else {
                // Need to negotiate settings with the opponent.
                const myPendingSettings = defaultMatchSettings(
                  configRef.current.nickname
                );
                myPendingSettings.gameInfo = gameInfo;
                myPendingSettings.availableGames =
                  gameInfo != null ? getAvailableGames(gameInfo) : [];
                setPendingStates((pendingStates) => ({
                  ...pendingStates!,
                  opponent: null,
                  own: { negotiatedState: null, settings: myPendingSettings },
                }));

                // After this point, do not read from gameInfo or saveName!
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

                const remoteChunks = [];

                // eslint-disable-next-line no-constant-condition
                while (true) {
                  const msg = await core.receive();
                  if (msg == null) {
                    throw "expected ipc message";
                  }

                  if (msg.connectionQualityInd != null) {
                    setRtt(msg.connectionQualityInd.rtt);
                    continue;
                  }

                  if (msg.smuggleInd == null) {
                    throw "expected smuggle indication";
                  }

                  const lobbyMsg = Message.decode(msg.smuggleInd.data);
                  if (lobbyMsg.uncommit != null) {
                    setPendingStates((pendingStates) => ({
                      ...pendingStates!,
                      opponent: {
                        ...pendingStates!.opponent!,
                        commitment: null,
                      },
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

                    if (
                      pendingStatesRef.current!.own!.negotiatedState != null
                    ) {
                      break;
                    }

                    continue;
                  }

                  if (lobbyMsg.chunk != null) {
                    remoteChunks.push(lobbyMsg.chunk.chunk);
                    break;
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
                      negotiatedState: null,
                    },
                  }));

                  onOpponentSettingsChange(lobbyMsg.setSettings);
                }

                const localMarshaledState = await promisify(brotliCompress)(
                  NegotiatedState.encode(
                    pendingStatesRef.current!.own!.negotiatedState!
                  ).finish()
                );

                const CHUNK_SIZE = 32 * 1024;

                const CHUNKS_REQUIRED = 5;
                for (let i = 0; i < CHUNKS_REQUIRED; i++) {
                  await core.send({
                    smuggleReq: {
                      data: Message.encode({
                        setSettings: undefined,
                        commit: undefined,
                        uncommit: undefined,
                        chunk: {
                          chunk: localMarshaledState.subarray(
                            i * CHUNK_SIZE,
                            (i + 1) * CHUNK_SIZE
                          ),
                        },
                      }).finish(),
                    },
                    startReq: undefined,
                  });

                  if (remoteChunks.length < CHUNKS_REQUIRED) {
                    const msg = await core.receive();
                    if (msg == null) {
                      throw "expected ipc message";
                    }

                    if (msg.connectionQualityInd != null) {
                      setRtt(msg.connectionQualityInd.rtt / 1000 / 1000);
                      continue;
                    }

                    if (msg.smuggleInd == null) {
                      throw "expected smuggle indication";
                    }

                    const lobbyMsg = Message.decode(msg.smuggleInd.data);
                    if (lobbyMsg.chunk == null) {
                      throw "expected chunk";
                    }

                    remoteChunks.push(lobbyMsg.chunk.chunk);
                  }
                }

                const remoteMarshaledState = new Uint8Array(
                  await promisify(brotliDecompress)(
                    new Uint8Array(Buffer.concat(remoteChunks))
                  )
                );
                const remoteCommitment = makeCommitment(remoteMarshaledState);

                if (
                  !timingSafeEqual(
                    remoteCommitment,
                    pendingStatesRef.current!.opponent!.commitment!
                  )
                ) {
                  throw "commitment mismatch";
                }

                const remoteState =
                  NegotiatedState.decode(remoteMarshaledState);

                const rngSeed =
                  pendingStatesRef.current!.own!.negotiatedState!.nonce.slice();
                for (let i = 0; i < rngSeed.length; i++) {
                  rngSeed[i] ^= remoteState.nonce[i];
                }

                const ownGameSettings = pendingStatesRef.current!.own!.settings;
                const ownGameInfo = ownGameSettings.gameInfo!;

                const outOwnROMPath = path.join(
                  tempDir,
                  `${ownGameInfo.rom}${
                    ownGameInfo.patch != null
                      ? `+${ownGameInfo.patch.name}-v${ownGameInfo.patch.version}`
                      : ""
                  }.gba`
                );
                await makeROM(
                  getROMPath(ownGameInfo.rom),
                  ownGameInfo.patch != null
                    ? getPatchPath(ownGameInfo.patch)
                    : null,
                  outOwnROMPath
                );

                const opponentGameSettings =
                  pendingStatesRef.current!.opponent!.settings;
                const opponentGameInfo = opponentGameSettings.gameInfo!;

                const outOpponentROMPath = path.join(
                  tempDir,
                  `${opponentGameInfo.rom}${
                    opponentGameInfo.patch != null
                      ? `+${opponentGameInfo.patch.name}-v${opponentGameInfo.patch.version}`
                      : ""
                  }.gba`
                );
                await makeROM(
                  getROMPath(opponentGameInfo.rom),
                  opponentGameInfo.patch != null
                    ? getPatchPath(opponentGameInfo.patch)
                    : null,
                  outOpponentROMPath
                );

                const now = new Date();

                const prefix = `${datefns.format(
                  now,
                  "yyyyMMddHHmmss"
                )}-vs-${encodeURIComponent(
                  opponentGameSettings.nickname
                )}-${linkCode}`;

                const shadowSavePath = path.join(tempDir, prefix + ".sav");
                await writeFile(shadowSavePath, remoteState.saveData);

                if (opponentGameSettings.openSetup) {
                  setOpenSetupEditor(
                    new bn6.Editor(
                      bn6.Editor.sramDumpToRaw(
                        new Uint8Array(remoteState.saveData).buffer
                      ),
                      opponentGameInfo.rom
                    )
                  );
                }

                const enc = new TextEncoder();

                const startReq = {
                  romPath: outOwnROMPath,
                  savePath: path.join(getSavesPath(app), saveNameRef.current!),
                  windowTitle: getGameTitle(ownGameInfo),
                  settings: {
                    shadowSavePath,
                    shadowRomPath: outOpponentROMPath,
                    inputDelay: ownGameSettings.inputDelay,
                    shadowInputDelay: opponentGameSettings.inputDelay,
                    matchType: ownGameSettings.matchType,
                    opponentNickname:
                      ownGameInfo.patch == null
                        ? opponentGameSettings.nickname
                        : undefined,
                    replaysPath: path.join(getReplaysPath(app), prefix),
                    replayMetadata: enc.encode(
                      JSON.stringify({
                        ts: now.valueOf(),
                        linkCode,
                        rom: ownGameInfo.rom,
                        patch:
                          ownGameInfo.patch != null
                            ? {
                                name: ownGameInfo.patch.name,
                                version: ownGameInfo.patch.version,
                              }
                            : null,
                        remote: {
                          nickname: opponentGameSettings.nickname,
                          rom: opponentGameInfo.rom,
                          patch:
                            opponentGameInfo.patch != null
                              ? {
                                  name: opponentGameInfo.patch.name,
                                  version: opponentGameInfo.patch.version,
                                }
                              : null,
                        },
                      } as ReplayInfo)
                    ),
                    rngSeed,
                  },
                } as ToCoreMessage_StartRequest;

                // eslint-disable-next-line no-console
                console.info("issuing start request", {
                  ...startReq,
                  settings: { ...startReq.settings, replayMetadata: undefined },
                });

                await core.send({
                  smuggleReq: undefined,
                  startReq,
                });
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
                (async () => {
                  const exitStatus = await core.wait();
                  const stderr = core.getStderr();
                  if (
                    exitStatus.exitCode != 0 &&
                    exitStatus.signalCode != "SIGTERM"
                  ) {
                    setErrorDialogState({ stderr, exitStatus });
                  }
                  onReadyChange(false);
                  onOpponentSettingsChange(null);
                  setOpenSetupEditor(null);
                  setPendingStates(null);
                  onExit();
                })();
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
                    .replace(/[^a-z0-9-]/g, "")
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
                  ) : (
                    <GenerateRandomCodeButton
                      onClick={(_e, code) => {
                        setLinkCode(code);
                      }}
                    />
                  ),
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
                pendingStates != null || (linkCode == "" && gameInfo == null)
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
                          checked={pendingStates.own.negotiatedState != null}
                          disabled={
                            saveName == null ||
                            pendingStates.own.settings.matchType !=
                              pendingStates.opponent.settings.matchType ||
                            pendingStates.own.settings.openSetup !=
                              pendingStates.opponent.settings.openSetup ||
                            !pendingStates.opponent.settings.availableGames.some(
                              (g) =>
                                gameInfoMatches(
                                  pendingStates.own!.settings.gameInfo ?? null,
                                  g
                                )
                            ) ||
                            pendingStates.opponent.settings.gameInfo == null ||
                            !hasGame(
                              pendingStates.opponent.settings.gameInfo
                            ) ||
                            changingCommitment ||
                            (pendingStates.own.negotiatedState != null &&
                              pendingStates.opponent.commitment != null)
                          }
                          indeterminate={changingCommitment}
                          onChange={(_e, v) => {
                            setChangingCommitment(true);
                            (async () => {
                              let commitment: Uint8Array | null = null;
                              let negotiatedState: NegotiatedState | null =
                                null;

                              if (v) {
                                const saveData = await readFile(
                                  path.join(getSavesPath(app), saveName!)
                                );
                                const nonce = crypto.getRandomValues(
                                  new Uint8Array(16)
                                );

                                negotiatedState = {
                                  nonce,
                                  saveData,
                                };

                                commitment = makeCommitment(
                                  Buffer.from(
                                    NegotiatedState.encode(
                                      negotiatedState
                                    ).finish()
                                  )
                                );
                              }

                              if (commitment != null) {
                                await pendingStates.core.send({
                                  smuggleReq: {
                                    data: Message.encode({
                                      setSettings: undefined,
                                      commit: {
                                        commitment,
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
                                  negotiatedState,
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
      {openSetupEditor != null ? (
        <Modal
          open={true}
          onClose={(_e, _reason) => {
            return;
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
              <Stack
                direction="row"
                sx={{ pt: 1, px: 1, alignItems: "center" }}
              >
                <Typography
                  variant="h6"
                  component="h2"
                  flexGrow={0}
                  flexShrink={0}
                  sx={{ px: 1 }}
                >
                  <Trans
                    i18nKey="play:open-setup-title"
                    values={{
                      opponentNickname:
                        pendingStates!.opponent!.settings.nickname,
                    }}
                  />
                </Typography>
              </Stack>
              <Box flexGrow={1} sx={{ display: "flex" }}>
                <SaveViewer editor={openSetupEditor} />
              </Box>
            </Stack>
          </Box>
        </Modal>
      ) : null}
      {errorDialogState != null ? (
        <Modal open={true}>
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
              <Stack spacing={1} flexGrow={1}>
                <Box sx={{ flexGrow: 0, flexShrink: 0 }}>
                  <Trans i18nKey="supervisor:crash" />
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
                    value={errorDialogState.stderr.trimEnd()}
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
                    value={errorDialogState.stderr.trimEnd()}
                  />
                </Box>
                <Stack direction="row" justifyContent="flex-end">
                  <Button
                    variant="contained"
                    color="error"
                    onClick={(_e) => {
                      setErrorDialogState(null);
                    }}
                  >
                    <Trans i18nKey="supervisor:dismiss" />
                  </Button>
                </Stack>
              </Stack>
            </Box>
          </Box>
        </Modal>
      ) : null}
    </>
  );
}

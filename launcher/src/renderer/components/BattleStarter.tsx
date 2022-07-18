import { timingSafeEqual } from "crypto";
import * as datefns from "date-fns";
import { clipboard } from "electron";
import { createReadStream } from "fs";
import { readFile, writeFile } from "fs/promises";
import { isEqual } from "lodash-es";
import fetch from "node-fetch";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import {
    Sparklines, SparklinesLine, SparklinesReferenceLine, SparklinesSpots
} from "react-sparklines";
import { usePrevious } from "react-use";
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
import Collapse from "@mui/material/Collapse";
import Divider from "@mui/material/Divider";
import FormControlLabel from "@mui/material/FormControlLabel";
import FormGroup from "@mui/material/FormGroup";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
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
import useTheme from "@mui/system/useTheme";

import { Config } from "../../config";
import * as discord from "../../discord";
import { makeROM } from "../../game";
import * as ipc from "../../ipc";
import { GetRequest, GetResponse } from "../../protos/generated/iceconfig";
import {
    FromCoreMessage_StateEvent_State, ToCoreMessage_StartRequest
} from "../../protos/generated/ipc";
import {
    GameInfo, GameInfo_Patch, Message, NegotiatedState, SetSettings
} from "../../protos/generated/lobby";
import { ReplayMetadata } from "../../protos/generated/replay";
import randomCode from "../../randomcode";
import { FAMILY_BY_ROM_NAME, KNOWN_ROM_FAMILIES } from "../../rom";
import { Editor, editorClassForGameFamily } from "../../saveedit";
import { useGetPatchPath, useGetROMPath } from "../hooks";
import { fallbackLng } from "../i18n";
import { requestAttention } from "../platform";
import { useConfig } from "./ConfigContext";
import CopyButton from "./CopyButton";
import { AllowEdits as AllowFolderEdits } from "./FolderViewer";
import { usePatches } from "./PatchesContext";
import { useROMs } from "./ROMsContext";
import SaveViewer from "./SaveViewer";
import Spinner from "./Spinner";
import { useTempDir } from "./TempDirContext";

const MATCH_TYPES = ["single", "triple"];

function removeBadPathCharacters(s: string): string {
  return s.replace(/[/\\?%*:|"<>. ]/g, "_");
}

function useGetPatchName() {
  const { patches } = usePatches();
  const { t } = useTranslation();
  return React.useCallback(
    (patchInfo: { name: string; version: string }) => {
      const title = Object.prototype.hasOwnProperty.call(
        patches,
        patchInfo.name
      )
        ? patches[patchInfo.name].title
        : `${t("play:missing-patch", { name: patchInfo.name })}`;
      return `${title} v${patchInfo.version}`;
    },
    [patches, t]
  );
}

function useGetGameFamilyTitle() {
  const getPatchName = useGetPatchName();
  const { i18n } = useTranslation();

  return React.useCallback(
    (gameInfo: GameInfo) =>
      `${
        KNOWN_ROM_FAMILIES[FAMILY_BY_ROM_NAME[gameInfo.rom]].title[
          i18n.resolvedLanguage
        ] ||
        KNOWN_ROM_FAMILIES[FAMILY_BY_ROM_NAME[gameInfo.rom]].title[fallbackLng]
      }${gameInfo.patch != null ? ` + ${getPatchName(gameInfo.patch)}` : ""}`,
    [i18n, getPatchName]
  );
}

function useGetGameTitle() {
  const getPatchName = useGetPatchName();
  const { i18n, t } = useTranslation();

  return React.useCallback(
    (gameInfo: GameInfo) => {
      const family = KNOWN_ROM_FAMILIES[FAMILY_BY_ROM_NAME[gameInfo.rom]];

      const familyName =
        family.title[i18n.resolvedLanguage] || family.title[fallbackLng];

      const romTitle = family.versions[gameInfo.rom].title;

      return `${
        romTitle != null
          ? t("play:rom-name", {
              familyName,
              versionName:
                romTitle[i18n.resolvedLanguage] || romTitle[fallbackLng],
            })
          : familyName
      }${gameInfo.patch != null ? ` + ${getPatchName(gameInfo.patch)}` : ""}`;
    },
    [i18n, t, getPatchName]
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

export function useGetNetplayCompatibility() {
  const { patches } = usePatches();
  return React.useCallback(
    (gameInfo: GameInfo) => {
      let netplayCompatibility = FAMILY_BY_ROM_NAME[gameInfo.rom];
      if (gameInfo.patch != null) {
        if (
          !Object.prototype.hasOwnProperty.call(patches, gameInfo.patch.name) ||
          !Object.prototype.hasOwnProperty.call(
            patches[gameInfo.patch.name].versions,
            gameInfo.patch.version
          )
        ) {
          return null;
        }
        netplayCompatibility =
          patches[gameInfo.patch.name].versions[gameInfo.patch.version]
            .netplayCompatibility;
      }
      return netplayCompatibility;
    },
    [patches]
  );
}

function useIsNetplayCompatible() {
  const getNetplayCompatiblity = useGetNetplayCompatibility();

  return React.useCallback(
    (ownGameInfo: GameInfo | null, opponentGameInfo: GameInfo | null) => {
      const ownNetplayCompatiblity =
        ownGameInfo != null ? getNetplayCompatiblity(ownGameInfo) : null;
      const opponentNetplayCompatiblity =
        opponentGameInfo != null
          ? getNetplayCompatiblity(opponentGameInfo)
          : null;
      if (
        ownNetplayCompatiblity == null ||
        opponentNetplayCompatiblity == null
      ) {
        return null;
      }
      return ownNetplayCompatiblity == opponentNetplayCompatiblity;
    },
    [getNetplayCompatiblity]
  );
}

function useAvailableGames() {
  const { patches } = usePatches();
  const { roms } = useROMs();

  return React.useMemo(() => {
    return Array.from(
      (function* () {
        for (const romName of Object.keys(roms)) {
          yield { rom: romName, patch: undefined };
        }

        for (const patchName of Object.keys(patches)) {
          const patch = patches[patchName];
          for (const version of Object.keys(patch.versions)) {
            for (const r of patch.versions[version].forROMs) {
              yield {
                rom: r.name,
                patch: { name: patchName, version },
              };
            }
          }
        }
      })()
    );
  }, [patches, roms]);
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

function addToArrayCapped<T>(xs: T[], x: T, limit: number) {
  return [...(xs.length >= limit ? xs.slice(1) : xs), x];
}

async function runCallback(
  config: Config,
  signal: AbortSignal,
  linkCode: string,
  ref: React.MutableRefObject<{
    coreRef: React.MutableRefObject<ipc.Core | null>;
    availableGames: SetSettings["availableGames"];
    getGameTitle: (gameInfo: GameInfo) => string;
    getGameFamilyTitle: (gameInfo: GameInfo) => string;
    getPatchPath: (
      rom: string,
      patch: { name: string; version: string }
    ) => string;
    getROMPath: (romName: string) => string;
    onOpponentSettingsChange: (settings: SetSettings | null) => void;
    pendingStates: PendingStates | null;
    setPendingStates: React.Dispatch<
      React.SetStateAction<PendingStates | null>
    >;
    gameInfo: GameInfo | null;
    tempDir: string;
    saveName: string | null;
    setState: React.Dispatch<
      React.SetStateAction<FromCoreMessage_StateEvent_State>
    >;
    config: Config;
    setRtts: React.Dispatch<React.SetStateAction<number[]>>;
    setRevealedSetupEditor: React.Dispatch<React.SetStateAction<Editor | null>>;
  }>
) {
  let iceServers = config.iceServers;

  if (linkCode != "") {
    try {
      const rawResp = await fetch(config.endpoints.iceconfig, {
        method: "POST",
        headers: {
          "User-Agent": `tango-launcher/${app.getVersion()}`,
          "Content-Type": "application/x-protobuf",
        },
        body: Buffer.from(
          GetRequest.encode({
            sessionId: linkCode,
          }).finish()
        ),
        signal: (() => {
          const abortController = new AbortController();
          signal.addEventListener("abort", () => {
            abortController.abort();
          });

          // Abort the relay request after 30 seconds.
          setTimeout(() => {
            abortController.abort();
          }, 10 * 1000);

          return abortController.signal;
        })(),
      });
      if (rawResp.ok) {
        const resp = GetResponse.decode(
          new Uint8Array(await rawResp.arrayBuffer())
        );
        // eslint-disable-next-line no-console
        console.info("iceconfig:", resp);
        iceServers = resp.iceServers.flatMap((iceServer) =>
          iceServer.urls.flatMap((url) => {
            const colonIdx = url.indexOf(":");
            if (colonIdx == -1) {
              return [];
            }
            const proto = url.slice(0, colonIdx);
            const rest = url.slice(colonIdx + 1);
            // libdatachannel doesn't support TURN over TCP: in fact, it explodes!
            const qmarkIdx = rest.lastIndexOf("?");
            if (qmarkIdx != -1 && rest.slice(qmarkIdx + 1) == "transport=tcp") {
              return [];
            }
            return iceServer.username == null || iceServer.credential == null
              ? [`${proto}:${rest}`]
              : [
                  `${proto}:${encodeURIComponent(
                    iceServer.username
                  )}:${encodeURIComponent(iceServer.credential)}@${rest}`,
                ];
          })
        );
      } else {
        throw await rawResp.text();
      }
    } catch (e) {
      console.warn("failed to get relay servers:", e);
    }
  }

  const core = new ipc.Core(
    config.inputMapping,
    config.endpoints.signaling,
    iceServers,
    linkCode,
    {
      env: {
        RUST_LOG: config.rustLogFilter,
        RUST_BACKTRACE: "1",
      },
      signal,
    }
  );
  ref.current.coreRef.current = core;

  if (linkCode != "") {
    discord.setLinkCode(
      linkCode,
      ref.current.gameInfo != null
        ? {
            title: ref.current.getGameFamilyTitle(ref.current.gameInfo),
            family: FAMILY_BY_ROM_NAME[ref.current.gameInfo.rom],
          }
        : null
    );
  } else {
    discord.setSinglePlayer(
      ref.current.gameInfo != null
        ? {
            title: ref.current.getGameFamilyTitle(ref.current.gameInfo),
            family: FAMILY_BY_ROM_NAME[ref.current.gameInfo.rom],
          }
        : null
    );
  }

  // eslint-disable-next-line no-constant-condition
  while (true) {
    const msg = await core.receive();
    if (msg == null) {
      throw "unexpected eof from core";
    }
    if (msg.stateEv != null) {
      ref.current.setState(msg.stateEv.state);
      if (msg.stateEv.state == FromCoreMessage_StateEvent_State.STARTING) {
        break;
      }
    }
  }

  if (linkCode == "") {
    // No link code to worry about, just start the game with no settings.
    const outFullROMName = `${ref.current.gameInfo!.rom}${
      ref.current.gameInfo!.patch != null
        ? `+${ref.current.gameInfo!.patch.name}-v${
            ref.current.gameInfo!.patch.version
          }`
        : ""
    }`;
    const outROMPath = path.join(
      ref.current.tempDir,
      `${outFullROMName.replace(/\0/g, "@")}.gba`
    );
    await makeROM(
      ref.current.getROMPath(ref.current.gameInfo!.rom),
      ref.current.gameInfo!.patch != null
        ? ref.current.getPatchPath(
            ref.current.gameInfo!.rom,
            ref.current.gameInfo!.patch
          )
        : null,
      outROMPath
    );

    await core.send({
      smuggleReq: undefined,
      startReq: {
        romPath: outROMPath,
        savePath: path.join(
          ref.current.config.paths.saves,
          ref.current.saveName!
        ),
        windowTitle: ref.current.getGameTitle(ref.current.gameInfo!),
        windowScale: ref.current.config.windowScale,
        settings: undefined,
      },
    });
  } else {
    requestAttention(app);

    // Need to negotiate settings with the opponent.
    const myPendingSettings: SetSettings = {
      nickname: ref.current.config.nickname!,
      inputDelay: ref.current.config.defaultMatchSettings.inputDelay,
      matchType: ref.current.config.defaultMatchSettings.matchType,
      gameInfo: undefined,
      availableGames: [],
      revealSetup: false,
    };
    myPendingSettings.gameInfo = ref.current.gameInfo ?? undefined;
    myPendingSettings.availableGames = ref.current.availableGames;
    ref.current.setPendingStates((pendingStates) => ({
      ...pendingStates!,
      opponent: null,
      own: { negotiatedState: null, settings: myPendingSettings },
    }));

    discord.setInLobby(
      linkCode,
      myPendingSettings.gameInfo != null
        ? {
            title: ref.current.getGameFamilyTitle(myPendingSettings.gameInfo),
            family: FAMILY_BY_ROM_NAME[myPendingSettings.gameInfo.rom],
          }
        : null
    );

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

      if (msg.connectionQualityEv != null) {
        ref.current.setRtts((rtts) =>
          addToArrayCapped(rtts, msg.connectionQualityEv!.rtt, 9)
        );
        continue;
      }

      if (msg.smuggleEv == null) {
        throw "expected smuggle event";
      }

      const lobbyMsg = Message.decode(msg.smuggleEv.data);
      if (lobbyMsg.uncommit != null) {
        ref.current.setPendingStates((pendingStates) => ({
          ...pendingStates!,
          opponent: {
            ...pendingStates!.opponent!,
            commitment: null,
          },
        }));
        continue;
      }

      if (lobbyMsg.commit != null) {
        ref.current.setPendingStates((pendingStates) => ({
          ...pendingStates!,
          opponent: {
            ...pendingStates!.opponent!,
            commitment: lobbyMsg.commit!.commitment,
          },
        }));

        if (ref.current.pendingStates!.own!.negotiatedState != null) {
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

      ref.current.setPendingStates((pendingStates) => ({
        ...pendingStates!,
        own: {
          ...pendingStates!.own!,
          negotiatedState:
            pendingStates!.opponent != null &&
            isSettingsChangeTrivial(
              pendingStates!.opponent!.settings,
              lobbyMsg.setSettings!
            )
              ? pendingStates!.own!.negotiatedState
              : null,
        },
        opponent: {
          settings: lobbyMsg.setSettings!,
          commitment: null,
        },
      }));

      ref.current.onOpponentSettingsChange(lobbyMsg.setSettings);
    }

    const localMarshaledState = await promisify(brotliCompress)(
      NegotiatedState.encode(
        ref.current.pendingStates!.own!.negotiatedState!
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
        // eslint-disable-next-line no-constant-condition
        while (true) {
          // We keep looping until we get a remote chunk.

          const msg = await core.receive();
          if (msg == null) {
            throw "expected ipc message";
          }

          if (msg.connectionQualityEv != null) {
            ref.current.setRtts((rtts) =>
              addToArrayCapped(rtts, msg.connectionQualityEv!.rtt, 9)
            );
            continue;
          }

          if (msg.smuggleEv == null) {
            throw "expected smuggle event";
          }

          const lobbyMsg = Message.decode(msg.smuggleEv.data);
          if (lobbyMsg.chunk == null) {
            throw "expected chunk";
          }

          remoteChunks.push(lobbyMsg.chunk.chunk);
          break;
        }
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
        ref.current.pendingStates!.opponent!.commitment!
      )
    ) {
      throw "commitment mismatch";
    }

    const remoteState = NegotiatedState.decode(remoteMarshaledState);

    const rngSeed =
      ref.current.pendingStates!.own!.negotiatedState!.nonce.slice();
    for (let i = 0; i < rngSeed.length; i++) {
      rngSeed[i] ^= remoteState.nonce[i];
    }

    const ownGameSettings = ref.current.pendingStates!.own!.settings;
    const ownGameInfo = ownGameSettings.gameInfo!;

    const ownFullROMName = `${ownGameInfo.rom}${
      ownGameInfo.patch != null
        ? `+${ownGameInfo.patch.name}-v${ownGameInfo.patch.version}`
        : ""
    }`;
    const outOwnROMPath = path.join(
      ref.current.tempDir,
      `${ownFullROMName.replace(/\0/g, "@")}.gba`
    );
    await makeROM(
      ref.current.getROMPath(ownGameInfo.rom),
      ownGameInfo.patch != null
        ? ref.current.getPatchPath(ownGameInfo.rom, ownGameInfo.patch)
        : null,
      outOwnROMPath
    );

    const opponentGameSettings = ref.current.pendingStates!.opponent!.settings;
    const opponentGameInfo = opponentGameSettings.gameInfo!;

    const opponentFullROMName = `${opponentGameInfo.rom}${
      opponentGameInfo.patch != null
        ? `+${opponentGameInfo.patch.name}-v${opponentGameInfo.patch.version}`
        : ""
    }`;
    const outOpponentROMPath = path.join(
      ref.current.tempDir,
      `${opponentFullROMName.replace(/\0/g, "@")}.gba`
    );
    await makeROM(
      ref.current.getROMPath(opponentGameInfo.rom),
      opponentGameInfo.patch != null
        ? ref.current.getPatchPath(opponentGameInfo.rom, opponentGameInfo.patch)
        : null,
      outOpponentROMPath
    );

    const now = new Date();

    const prefix = `${datefns.format(
      now,
      "yyyyMMddHHmmss"
    )}-${ownFullROMName.replace(/\0/g, "@")}-vs-${removeBadPathCharacters(
      opponentGameSettings.nickname
    )}-${linkCode}`;

    const shadowSavePath = path.join(ref.current.tempDir, prefix + ".sav");
    await writeFile(shadowSavePath, remoteState.saveData);

    if (opponentGameSettings.revealSetup) {
      const Editor = editorClassForGameFamily(
        FAMILY_BY_ROM_NAME[opponentGameInfo.rom]
      );
      ref.current.setRevealedSetupEditor(
        new Editor(
          Editor.sramDumpToRaw(new Uint8Array(remoteState.saveData).buffer),
          opponentGameInfo.rom
        )
      );
    }

    const startReq = {
      romPath: outOwnROMPath,
      savePath: path.join(
        ref.current.config.paths.saves,
        ref.current.saveName!
      ),
      windowTitle: ref.current.getGameTitle(ownGameInfo),
      windowScale: config.windowScale,
      settings: {
        shadowSavePath,
        shadowRomPath: outOpponentROMPath,
        inputDelay: ownGameSettings.inputDelay,
        shadowInputDelay: opponentGameSettings.inputDelay,
        matchType: ownGameSettings.matchType,
        opponentNickname:
          ownGameInfo.patch == null ? opponentGameSettings.nickname : undefined,
        replaysPath: path.join(ref.current.config.paths.replays, prefix),
        replayMetadata: ReplayMetadata.encode({
          ts: now.valueOf(),
          linkCode: linkCode,
          localSide: {
            nickname: ownGameSettings.nickname,
            gameInfo: ownGameInfo,
            revealSetup: ownGameSettings.revealSetup,
          },
          remoteSide: {
            nickname: opponentGameSettings.nickname,
            gameInfo: opponentGameInfo,
            revealSetup: opponentGameSettings.revealSetup,
          },
        }).finish(),
        maxQueueLength: config.maxQueueLength,
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

    ref.current.setRtts([]);
    discord.setInProgress(linkCode, new Date(), {
      title: ref.current.getGameFamilyTitle(ownGameInfo),
      family: FAMILY_BY_ROM_NAME[ownGameInfo.rom],
    });
  }

  // eslint-disable-next-line no-constant-condition
  while (true) {
    const msg = await core.receive();
    if (msg == null) {
      break;
    }

    if (msg.stateEv != null) {
      ref.current.setState(msg.stateEv.state);
    }

    if (msg.roundEndedEv != null) {
      // eslint-disable-next-line no-console
      console.log("round ended", msg.roundEndedEv);
      if (config.endpoints.replaycollector != "") {
        (async () => {
          const collectorResp = await fetch(config.endpoints.replaycollector, {
            method: "POST",
            headers: {
              "Content-Type": "application/x-tango-replay",
            },
            body: createReadStream(msg.roundEndedEv!.replayFilename),
          });
          if (!collectorResp.ok) {
            console.error(
              "failed to send to collector",
              collectorResp.status,
              await collectorResp.text()
            );
          }
        })();
      }
    }
  }
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
        edge="end"
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

interface PendingStates {
  own: {
    settings: SetSettings;
    negotiatedState: NegotiatedState | null;
  } | null;
  opponent: {
    settings: SetSettings;
    commitment: Uint8Array | null;
  } | null;
}

interface TrivializedSettings {
  matchType: number;
  gameFamily: string | null;
  patch: GameInfo_Patch | null;
  revealSetup: boolean;
}

function trivializeSettings(settings: SetSettings): TrivializedSettings {
  return {
    matchType: settings.matchType,
    gameFamily:
      settings.gameInfo != null
        ? FAMILY_BY_ROM_NAME[settings.gameInfo.rom]
        : null,
    patch: (settings.gameInfo != null ? settings.gameInfo.patch : null) ?? null,
    revealSetup: settings.revealSetup,
  };
}

function isSettingsChangeTrivial(
  previousSettings: SetSettings,
  settings: SetSettings
) {
  return (
    isEqual(
      trivializeSettings(previousSettings),
      trivializeSettings(settings)
    ) || settings.revealSetup
  );
}

export default function BattleStarter({
  saveName,
  gameInfo,
  onExit,
  onReadyChange,
  onOpponentSettingsChange,
}: {
  saveName: string | null;
  gameInfo: GameInfo | null;
  onExit: () => void;
  onReadyChange: (ready: boolean) => void;
  onOpponentSettingsChange: (settings: SetSettings | null) => void;
}) {
  const theme = useTheme();
  const { config, save: saveConfig } = useConfig();
  const { tempDir } = useTempDir();
  const getROMPath = useGetROMPath();
  const getPatchPath = useGetPatchPath();

  const availableGames = useAvailableGames();
  const isNetplayCompatible = useIsNetplayCompatible();
  const hasGame = useHasGame();

  const [exitDialogState, setExitDialogState] = React.useState<{
    stderr: string;
    exitStatus: ipc.ExitStatus;
  } | null>(null);

  const coreRef = React.useRef<ipc.Core | null>(null);
  const abortControllerRef = React.useRef<AbortController | null>(null);

  const [linkCode, setLinkCode] = React.useState("");
  const [state, setState] = React.useState<FromCoreMessage_StateEvent_State>(
    FromCoreMessage_StateEvent_State.UNKNOWN
  );
  const [pendingStates, setPendingStates] =
    React.useState<PendingStates | null>(null);

  const [changingCommitment, setChangingCommitment] = React.useState(false);
  const [rtts, setRtts] = React.useState<number[]>([]);

  const [revealedSetupEditor, setRevealedSetupEditor] =
    React.useState<Editor | null>(null);

  const previousGameInfo = usePrevious(gameInfo);
  const previousAvailableGames = usePrevious(availableGames);

  const getGameTitle = useGetGameTitle();
  const getGameFamilyTitle = useGetGameFamilyTitle();

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
          commitment: isSettingsChangeTrivial(
            pendingStates!.own!.settings,
            settings
          )
            ? pendingStates!.opponent!.commitment
            : null,
        },
      }));

      // eslint-disable-next-line no-console
      console.info("local pending state changed", settings);
      saveConfig((config) => ({
        ...config,
        defaultMatchSettings: {
          inputDelay: settings.inputDelay,
          matchType: settings.matchType,
        },
      }));
      coreRef.current!.send({
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
    [saveConfig]
  );

  React.useEffect(() => {
    if (
      isEqual(gameInfo, previousGameInfo) &&
      isEqual(availableGames, previousAvailableGames)
    ) {
      return;
    }

    discord.setLinkCode(
      linkCode,
      gameInfo != null
        ? {
            title: getGameFamilyTitle(gameInfo),
            family: FAMILY_BY_ROM_NAME[gameInfo.rom],
          }
        : null
    );

    if (pendingStates != null && pendingStates.own != null) {
      changeLocalPendingState({
        ...pendingStates.own.settings,
        gameInfo: gameInfo ?? undefined,
        availableGames,
      });
    }
  }, [
    linkCode,
    getGameFamilyTitle,
    gameInfo,
    previousGameInfo,
    availableGames,
    previousAvailableGames,
    changeLocalPendingState,
    pendingStates,
  ]);

  const myPendingState = pendingStates?.own;

  React.useEffect(() => {
    onReadyChange(myPendingState?.negotiatedState != null);
  }, [myPendingState, onReadyChange]);

  const runCallbackData = {
    coreRef,
    availableGames,
    getGameTitle,
    getGameFamilyTitle,
    getPatchPath,
    getROMPath,
    onOpponentSettingsChange,
    pendingStates,
    setPendingStates,
    gameInfo,
    tempDir,
    saveName,
    setState,
    config,
    setRtts,
    setRevealedSetupEditor,
  };
  const runCallbackDataRef = React.useRef(runCallbackData);
  runCallbackDataRef.current = runCallbackData;

  const start = React.useCallback(
    (linkCode: string) => {
      setLinkCode(linkCode);

      const abortController = new AbortController();
      abortControllerRef.current = abortController;

      setPendingStates({
        own: null,
        opponent: null,
      });

      runCallback(config, abortController.signal, linkCode, runCallbackDataRef)
        .catch((e) => {
          console.error(e);
        })
        .finally(() => {
          if (abortControllerRef.current != null) {
            abortControllerRef.current.abort();
            abortControllerRef.current = null;
          }
          discord.setDone();
          (async () => {
            const exitStatus = await coreRef.current!.wait();
            const stderr = coreRef.current!.getStderr();
            if (
              exitStatus.exitCode != 0 &&
              exitStatus.signalCode != "SIGTERM"
            ) {
              setExitDialogState({ stderr, exitStatus });
            }
            onReadyChange(false);
            onOpponentSettingsChange(null);
            setRevealedSetupEditor(null);
            setPendingStates(null);
            setState(FromCoreMessage_StateEvent_State.UNKNOWN);
            setRtts([]);
            coreRef.current = null;
            onExit();
          })();
        });
    },
    [config, onExit, onOpponentSettingsChange, onReadyChange]
  );

  React.useEffect(() => {
    const activityJoinCallback = (d: { secret: string }) => {
      if (pendingStates != null) {
        return;
      }
      start(d.secret);
    };
    discord.events.on("activityjoin", activityJoinCallback);
    return () => {
      discord.events.off("activityjoin", activityJoinCallback);
    };
  }, [start, pendingStates]);

  const medianRtt =
    rtts.length > 0
      ? rtts.slice().sort((a, b) => a - b)[Math.floor(rtts.length / 2)]
      : 0;

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
                          verticalAlign: "middle",
                        }}
                      />
                    ) : null}
                  </TableCell>
                  <TableCell sx={{ width: "40%", fontWeight: "bold" }}>
                    {pendingStates?.opponent?.settings.nickname ?? ""}{" "}
                    {rtts.length > 0 ? (
                      <>
                        <div style={{ display: "inline-block", width: "40px" }}>
                          <Sparklines
                            data={rtts}
                            min={0}
                            max={500 * 1000 * 1000}
                            limit={10}
                          >
                            <SparklinesLine
                              color={theme.palette.primary.main}
                            />
                            <SparklinesSpots />
                            <SparklinesReferenceLine type="median" />
                          </Sparklines>
                        </div>{" "}
                        <small>
                          <Trans
                            i18nKey="play:connection-quality"
                            values={{
                              rtt: Math.round(
                                rtts[rtts.length - 1] / 1000 / 1000
                              ),
                            }}
                          />
                        </small>{" "}
                      </>
                    ) : null}
                    {pendingStates?.opponent?.commitment != null ? (
                      <CheckCircleIcon
                        color="success"
                        sx={{
                          fontSize: "1em",
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
                      getGameFamilyTitle(gameInfo)
                    ) : (
                      <Trans i18nKey="play:no-game-selected" />
                    )}{" "}
                    {!isNetplayCompatible(
                      pendingStates?.own?.settings.gameInfo ?? null,
                      pendingStates?.opponent?.settings.gameInfo ?? null
                    ) ? (
                      <Tooltip
                        title={<Trans i18nKey="play:incompatible-game" />}
                      >
                        <WarningIcon
                          color="warning"
                          sx={{
                            fontSize: "1em",
                            verticalAlign: "middle",
                          }}
                        />
                      </Tooltip>
                    ) : !pendingStates?.opponent?.settings.availableGames.some(
                        (g) =>
                          gameInfoMatches(
                            pendingStates?.own?.settings.gameInfo ?? null,
                            g
                          )
                      ) ? (
                      <Tooltip title={<Trans i18nKey="play:no-remote-copy" />}>
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
                      getGameFamilyTitle(
                        pendingStates?.opponent?.settings.gameInfo
                      )
                    ) : (
                      <Trans i18nKey="play:no-game-selected" />
                    )}{" "}
                    {pendingStates?.opponent?.settings.gameInfo != null &&
                    !hasGame(pendingStates.opponent.settings.gameInfo) ? (
                      <Tooltip
                        title={
                          <Trans
                            i18nKey="play:no-local-copy"
                            values={{
                              gameTitle: getGameTitle(
                                pendingStates.opponent.settings.gameInfo
                              ),
                            }}
                          />
                        }
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
                      value={pendingStates?.own?.settings.inputDelay ?? 0}
                      disabled={pendingStates?.own?.negotiatedState != null}
                      onChange={(e) => {
                        let v = parseInt(e.target.value);
                        if (isNaN(v)) {
                          v = 0;
                        }
                        changeLocalPendingState({
                          ...pendingStates!.own!.settings,
                          inputDelay: Math.min(Math.max(v, 2), 10),
                        });
                      }}
                      InputProps={{ inputProps: { min: 2, max: 10 } }}
                    />{" "}
                    <Button
                      disabled={
                        rtts.length == 0 ||
                        pendingStates?.own?.negotiatedState != null
                      }
                      size="small"
                      color="primary"
                      variant="outlined"
                      onClick={() => {
                        changeLocalPendingState({
                          ...pendingStates!.own!.settings,
                          inputDelay: Math.min(
                            10,
                            Math.max(
                              2,
                              Math.round(
                                ((medianRtt / 1000 / 1000 / 2) * 60) / 1000
                              ) +
                                1 -
                                2
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
                    <Trans i18nKey="play:reveal-setup" />
                  </TableCell>
                  <TableCell>
                    <Switch
                      size="small"
                      checked={pendingStates?.own?.settings.revealSetup ?? true}
                      disabled={pendingStates?.own?.negotiatedState != null}
                      onChange={(_e, v) => {
                        changeLocalPendingState({
                          ...pendingStates!.own!.settings,
                          revealSetup: v,
                        });
                      }}
                    />
                  </TableCell>
                  <TableCell>
                    <Switch
                      size="small"
                      checked={
                        pendingStates?.opponent?.settings.revealSetup ?? true
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
          sx={{ px: 1, mb: 0, py: 1 }}
          component="form"
          onSubmit={(e: any) => {
            e.preventDefault();
            start(linkCode);
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
                    .replace(/[\s-]+/g, "-")
                    .replace(/[^a-z0-9-]/g, "")
                    .replace(/^-/g, "")
                    .slice(0, 40)
                );
              }}
              InputProps={{
                endAdornment: (
                  <InputAdornment position="end">
                    {pendingStates != null ? (
                      <Stack
                        spacing={1}
                        direction="row"
                        sx={{ alignItems: "center" }}
                      >
                        <Typography sx={{ whiteSpace: "nowrap" }}>
                          {state == FromCoreMessage_StateEvent_State.RUNNING ? (
                            <Trans i18nKey="supervisor:status.running" />
                          ) : state ==
                            FromCoreMessage_StateEvent_State.WAITING ? (
                            <Trans i18nKey="supervisor:status.waiting" />
                          ) : state ==
                            FromCoreMessage_StateEvent_State.CONNECTING ? (
                            <Trans i18nKey="supervisor:status.connecting" />
                          ) : state ==
                            FromCoreMessage_StateEvent_State.STARTING ? (
                            pendingStates.own != null ? null : (
                              <Trans i18nKey="supervisor:status.starting" />
                            )
                          ) : (
                            <Trans i18nKey="supervisor:status.unknown" />
                          )}
                        </Typography>
                        <Spinner size="1em" />
                      </Stack>
                    ) : (
                      <GenerateRandomCodeButton
                        onClick={(_e, code) => {
                          setLinkCode(code);
                        }}
                      />
                    )}
                  </InputAdornment>
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
                            !isNetplayCompatible(
                              pendingStates?.own?.settings.gameInfo ?? null,
                              pendingStates?.opponent?.settings.gameInfo ?? null
                            ) ||
                            changingCommitment ||
                            (pendingStates.own.negotiatedState != null &&
                              pendingStates.opponent.commitment != null)
                          }
                          indeterminate={changingCommitment}
                          onChange={(_e, v) => {
                            setChangingCommitment(true);
                            (async () => {
                              try {
                                let commitment: Uint8Array | null = null;
                                let negotiatedState: NegotiatedState | null =
                                  null;

                                if (v) {
                                  const saveData = await readFile(
                                    path.join(
                                      runCallbackDataRef.current.config.paths
                                        .saves,
                                      saveName!
                                    )
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
                                  // eslint-disable-next-line no-console
                                  console.info("sending commit to core");
                                  await coreRef.current!.send({
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
                                  // eslint-disable-next-line no-console
                                  console.info("sending uncommit to core");
                                  await coreRef.current!.send({
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
                              } catch (e) {
                                console.error("failed to change commitment", e);
                              } finally {
                                setChangingCommitment(false);
                              }
                            })();
                          }}
                        />
                      }
                      label={
                        <span style={{ userSelect: "none" }}>
                          <Trans i18nKey={"play:ready"} />
                        </span>
                      }
                    />
                  </FormGroup>
                </>
              ) : null}
              <Button
                color="error"
                variant="contained"
                startIcon={<StopIcon />}
                onClick={() => {
                  if (abortControllerRef.current != null) {
                    abortControllerRef.current.abort();
                  }
                }}
                disabled={false}
              >
                <Trans i18nKey="play:stop" />
              </Button>
            </>
          )}
        </Stack>
      </Stack>
      {revealedSetupEditor != null ? (
        <Modal
          open={true}
          onClose={(_e, _reason) => {
            return;
          }}
        >
          <Stack
            sx={{
              width: "100%",
              height: "100%",
              bgcolor: "background.paper",
            }}
            direction="column"
          >
            <Stack direction="row" sx={{ pt: 1, px: 1, alignItems: "center" }}>
              <Typography
                variant="h6"
                component="h2"
                flexGrow={0}
                flexShrink={0}
                sx={{ px: 1 }}
              >
                <Trans
                  i18nKey="play:reveal-setup-title"
                  values={{
                    opponentNickname:
                      pendingStates!.opponent!.settings.nickname,
                  }}
                />
              </Typography>
            </Stack>
            <Box flexGrow={1} sx={{ display: "flex", py: 1 }}>
              <SaveViewer
                editor={revealedSetupEditor}
                allowFolderEdits={AllowFolderEdits.None}
              />
            </Box>
          </Stack>
        </Modal>
      ) : null}
      {exitDialogState != null ? (
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
                bgcolor: "background.paper",
                boxShadow: 24,
                px: 3,
                py: 2,
                display: "flex",
              }}
            >
              <Stack spacing={1} flexGrow={1}>
                {exitDialogState.exitStatus.exitCode ==
                ipc.ExitCode.EXIT_CODE_LOST_CONNECTION ? (
                  <Box sx={{ flexGrow: 0, flexShrink: 0, width: 400 }}>
                    <Trans i18nKey="supervisor:error.lost-connection" />
                  </Box>
                ) : exitDialogState.exitStatus.exitCode ==
                  ipc.ExitCode.EXIT_CODE_PROTOCOL_VERSION_TOO_OLD ? (
                  <Box sx={{ flexGrow: 0, flexShrink: 0, width: 400 }}>
                    <Trans i18nKey="supervisor:error.protocol-version-too-old" />
                  </Box>
                ) : exitDialogState.exitStatus.exitCode ==
                  ipc.ExitCode.EXIT_CODE_PROTOCOL_VERSION_TOO_NEW ? (
                  <Box sx={{ flexGrow: 0, flexShrink: 0, width: 400 }}>
                    <Trans i18nKey="supervisor:error.protocol-version-too-new" />
                  </Box>
                ) : (
                  <>
                    <Box sx={{ flexGrow: 0, flexShrink: 0, width: 600 }}>
                      <Trans i18nKey="supervisor:error.unknown" />
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
                        value={exitDialogState.stderr.trimEnd()}
                        sx={{
                          position: "absolute",
                          right: "16px",
                          top: "8px",
                          zEvex: 999,
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
                        value={exitDialogState.stderr.trimEnd()}
                      />
                    </Box>
                  </>
                )}
                <Stack direction="row" justifyContent="flex-end">
                  <Button
                    variant="contained"
                    color="error"
                    onClick={(_e) => {
                      setExitDialogState(null);
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

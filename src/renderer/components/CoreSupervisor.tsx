import * as ipc from "../../ipc";
import React from "react";
import tmp from "tmp-promise";
import { useConfig } from "./ConfigContext";
import { usePatches } from "./PatchesContext";
import { useROMs } from "./ROMsContext";
import { makeROM } from "../../game";
import { Trans, useTranslation } from "react-i18next";
import { KNOWN_ROMS } from "../../rom";
import Modal from "@mui/material/Modal";
import Box from "@mui/material/Box";
import CircularProgress from "@mui/material/CircularProgress";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import { findPatchVersion, PatchVersionInfo } from "../../patchinfo";
import { ReplayInfo } from "../../replay";
import { getPatchesPath, getReplaysPath, getROMsPath } from "../../paths";
import path from "path";

export function CoreSupervisor({
  romName,
  patchName,
  patchVersionRequirement,
  sessionID,
  onExit,
}: {
  romName: string;
  patchName: string | null;
  patchVersionRequirement?: string;
  sessionID?: string;
  onExit: (exitStatus: ipc.ExitStatus) => void;
}) {
  const { roms } = useROMs();
  const { config } = useConfig();
  const { patches } = usePatches();
  const { i18n } = useTranslation();

  const romsRef = React.useRef(roms);
  const configRef = React.useRef(config);
  const patchesRef = React.useRef(patches);
  const i18nRef = React.useRef(i18n);

  const onExitRef = React.useRef(onExit);
  React.useEffect(() => {
    onExitRef.current = onExit;
  }, [onExit]);

  const [state, setState] = React.useState<ipc.State | null>(null);
  const [stderr, setStderr] = React.useState<string[]>([]);
  const [exitStatus, setExitStatus] = React.useState<ipc.ExitStatus | null>(
    null
  );

  const abortControllerRef = React.useRef<AbortController>(null!);
  if (abortControllerRef.current == null) {
    abortControllerRef.current = new AbortController();
  }

  React.useEffect(() => {
    let romTmpFile: tmp.FileResult | null = null;

    (async () => {
      const romFilename = romsRef.current[romName];
      let netplayCompatiblity = KNOWN_ROMS[romName].netplayCompatiblity;

      let patchVersion: { name: string; info: PatchVersionInfo } | null = null;
      if (patchName != null) {
        const patchInfo = patchesRef.current[patchName];
        const patchVersionName = findPatchVersion(
          patchInfo,
          patchVersionRequirement || "*"
        );
        if (patchVersionName == null) {
          throw "could not find patch with appropriate version";
        }

        const patchVersionInfo = patchInfo.versions[patchVersionName];
        patchVersion = {
          name: patchVersionName,
          info: patchVersionInfo,
        };
        netplayCompatiblity = patchVersionInfo.netplayCompatibility;
      }

      romTmpFile = await makeROM(
        path.join(getROMsPath(), romFilename),
        patchVersion != null
          ? path.join(
              getPatchesPath(),
              patchName!,
              `v${patchVersion.name}.${patchVersion.info.format}`
            )
          : null
      );

      const core = new ipc.Core(
        {
          window_title: `${
            KNOWN_ROMS[romName].title[i18nRef.current.resolvedLanguage]
          }${
            patchName != null ? ` + ${patchesRef.current[patchName].title}` : ""
          }`,
          rom_path: romTmpFile!.path,
          save_path: "saves/exe6f.sav",
          keymapping: configRef.current.keymapping,
          match_settings:
            sessionID == null
              ? null
              : {
                  session_id: `${netplayCompatiblity}-${sessionID}`,
                  input_delay: 0,
                  match_type: 0,
                  matchmaking_connect_addr:
                    configRef.current.matchmakingConnectAddr,
                  ice_servers: configRef.current.iceServers,
                  replay_prefix: path.join(getReplaysPath(), sessionID),
                  replay_metadata: JSON.stringify({
                    rom: romName,
                    patch:
                      patchVersion != null
                        ? { name: patchName, version: patchVersion.name }
                        : null,
                  } as ReplayInfo),
                },
        },
        {
          signal: abortControllerRef.current.signal,
        }
      );
      core.on("exit", (exitStatus) => {
        setExitStatus(exitStatus);
        onExitRef.current(exitStatus);
      });
      core.on("state", (state) => {
        setState(state);
      });
      core.on("stderr", (stderr) => {
        setStderr((lines) => {
          lines.push(stderr);
          return lines;
        });
      });
    })();

    return () => {
      if (romTmpFile != null) {
        romTmpFile.cleanup();
      }
      abortControllerRef.current.abort();
    };
  }, [romName, patchName, patchVersionRequirement, sessionID]);

  return (
    <Modal open={true}>
      <Box
        sx={{
          position: "absolute",
          top: "50%",
          left: "50%",
          transform: "translate(-50%, -50%)",
          width: 400,
          bgcolor: "background.paper",
          boxShadow: 24,
          p: 4,
        }}
      >
        <Stack
          direction="row"
          justifyContent="flex-start"
          alignItems="center"
          spacing={2}
        >
          <CircularProgress disableShrink size="2rem" />
          <Typography>
            {state == null ? (
              <Trans i18nKey="supervisor:status.starting"></Trans>
            ) : state == "Running" ? (
              <Trans i18nKey="supervisor:status.running"></Trans>
            ) : state == "Waiting" ? (
              <Trans i18nKey="supervisor:status.waiting"></Trans>
            ) : state == "Connecting" ? (
              <Trans i18nKey="supervisor:status.connecting"></Trans>
            ) : null}
          </Typography>
        </Stack>
      </Box>
    </Modal>
  );
}

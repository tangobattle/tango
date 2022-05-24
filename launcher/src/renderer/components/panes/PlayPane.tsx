import { readFile } from "fs/promises";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import semver from "semver";

import { app, shell } from "@electron/remote";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import RefreshIcon from "@mui/icons-material/Refresh";
import SportsEsportsOutlinedIcon from "@mui/icons-material/SportsEsportsOutlined";
import WarningIcon from "@mui/icons-material/Warning";
import Box from "@mui/material/Box";
import Collapse from "@mui/material/Collapse";
import FormControl from "@mui/material/FormControl";
import IconButton from "@mui/material/IconButton";
import InputLabel from "@mui/material/InputLabel";
import ListItemText from "@mui/material/ListItemText";
import ListSubheader from "@mui/material/ListSubheader";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { getBasePath, getSavesPath } from "../../../paths";
import { SetSettings } from "../../../protos/lobby";
import { KNOWN_ROMS } from "../../../rom";
import * as bn6 from "../../../saveedit/bn6";
import BattleStarter, { useGetNetplayCompatibility } from "../BattleStarter";
import { usePatches } from "../PatchesContext";
import { useROMs } from "../ROMsContext";
import { useSaves } from "../SavesContext";
import SaveViewer from "../SaveViewer";

function SaveViewerWrapper({
  filename,
  romName,
  incarnation,
}: {
  filename: string;
  romName: string;
  incarnation: number;
}) {
  const [editor, setEditor] = React.useState<bn6.Editor | null>(null);

  React.useEffect(() => {
    (async () => {
      const e = new bn6.Editor(
        bn6.Editor.sramDumpToRaw(
          (await readFile(path.join(getSavesPath(app), filename))).buffer
        ),
        romName
      );
      setEditor(e);
    })();
  }, [filename, romName, incarnation]);

  if (editor == null) {
    return null;
  }

  return <SaveViewer editor={editor} />;
}

export default function SavesPane({ active }: { active: boolean }) {
  const { saves, rescan: rescanSaves } = useSaves();
  const { patches, rescan: rescanPatches } = usePatches();
  const { roms, rescan: rescanROMs } = useROMs();
  const { i18n } = useTranslation();

  const [patchOptionsOpen, setPatchOptionsOpen] = React.useState(false);
  const [battleReady, setBattleReady] = React.useState(false);

  const [saveName_, setSaveName] = React.useState<string | null>(null);
  const [incarnation, setIncarnation] = React.useState(0);
  const [opponentSettings, setOpponentSettings] =
    React.useState<SetSettings | null>(null);
  const opponentAvailableGames = opponentSettings?.availableGames ?? [];

  const getNetplayCompatibility = useGetNetplayCompatibility();

  const saveName =
    saveName_ != null && Object.prototype.hasOwnProperty.call(saves, saveName_)
      ? saveName_
      : null;

  const groupedSaves: { [key: string]: string[] } = {};
  for (const k of Object.keys(saves)) {
    groupedSaves[saves[k].romName] = groupedSaves[saves[k].romName] || [];
    groupedSaves[saves[k].romName].push(k);
  }

  const romNames = Object.keys(roms);
  romNames.sort((k1, k2) => {
    const title1 = KNOWN_ROMS[k1].title[i18n.resolvedLanguage];
    const title2 = KNOWN_ROMS[k2].title[i18n.resolvedLanguage];
    return title1 < title2 ? -1 : title1 > title2 ? 1 : 0;
  });

  const [patchName_, setPatchName] = React.useState<string | null>(null);
  const patchName =
    patchName_ != null &&
    Object.prototype.hasOwnProperty.call(patches, patchName_)
      ? patchName_
      : null;

  const save = saveName != null ? saves[saveName] : null;

  const eligiblePatchNames = React.useMemo(() => {
    const eligiblePatchNames =
      save != null
        ? Object.keys(patches).filter((p) => patches[p].forROM == save.romName)
        : [];
    eligiblePatchNames.sort();
    return eligiblePatchNames;
  }, [patches, save]);

  const patchInfo = patchName != null ? patches[patchName] : null;

  const patchVersions = React.useMemo(
    () =>
      patchInfo != null ? semver.rsort(Object.keys(patchInfo.versions)) : null,
    [patchInfo]
  );

  const [patchVersion_, setPatchVersion] = React.useState<string | null>(null);
  const patchVersion =
    patchName != null &&
    patchVersion_ != null &&
    Object.prototype.hasOwnProperty.call(
      patches[patchName].versions,
      patchVersion_
    )
      ? patchVersion_
      : null;

  React.useEffect(() => {
    if (patchVersions == null) {
      setPatchVersion(null);
      return;
    }
    setPatchVersion(patchVersions[0]);
  }, [patchVersions]);

  const listFormatter = new Intl.ListFormat(i18n.resolvedLanguage, {
    style: "long",
    type: "conjunction",
  });

  return (
    <Box
      sx={{
        my: 1,
        flexGrow: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Stack sx={{ flexGrow: 1, width: 0 }}>
        <Box flexGrow={0} flexShrink={0} sx={{ px: 1 }}>
          <Stack spacing={1} flexGrow={0} flexShrink={0} direction="row">
            <Tooltip title={<Trans i18nKey="play:show-hide-extra-options" />}>
              <IconButton
                onClick={() => {
                  setPatchOptionsOpen((o) => !o);
                }}
              >
                {patchOptionsOpen ? (
                  <KeyboardArrowUpIcon />
                ) : (
                  <KeyboardArrowDownIcon />
                )}
              </IconButton>
            </Tooltip>
            <FormControl fullWidth size="small">
              <InputLabel id="select-save-label">
                <Trans i18nKey="play:select-save" />
              </InputLabel>
              <Select
                labelId="select-save-label"
                label={<Trans i18nKey="play:select-save" />}
                value={saveName ?? ""}
                disabled={battleReady}
                renderValue={(v) => {
                  if (v == "") {
                    return null;
                  }
                  return (
                    <>
                      {saveName != null &&
                      opponentSettings?.gameInfo != null &&
                      !Object.keys(patches)
                        .filter(
                          (p) => patches[p].forROM == saves[saveName].romName
                        )
                        .flatMap((p) =>
                          Object.keys(patches[p].versions).map(
                            (v) => patches[p].versions[v].netplayCompatibility
                          )
                        )
                        .concat([
                          KNOWN_ROMS[saves[saveName].romName]
                            .netplayCompatibility,
                        ])
                        .some(
                          (nc) =>
                            nc ==
                            getNetplayCompatibility(opponentSettings!.gameInfo!)
                        ) ? (
                        <Tooltip
                          title={<Trans i18nKey="play:incompatible-game" />}
                        >
                          <WarningIcon
                            color="warning"
                            sx={{
                              fontSize: "1em",
                              marginRight: "8px",
                              verticalAlign: "middle",
                            }}
                          />
                        </Tooltip>
                      ) : opponentAvailableGames.length > 0 &&
                        !opponentAvailableGames.some(
                          (g) => g.rom == saves[v].romName
                        ) ? (
                        <Tooltip
                          title={<Trans i18nKey="play:no-remote-copy" />}
                        >
                          <WarningIcon
                            color="warning"
                            sx={{
                              fontSize: "1em",
                              marginRight: "8px",
                              verticalAlign: "middle",
                            }}
                          />
                        </Tooltip>
                      ) : null}
                      {v}{" "}
                      <small>
                        {
                          KNOWN_ROMS[saves[v].romName].title[
                            i18n.resolvedLanguage
                          ]
                        }
                      </small>
                    </>
                  );
                }}
                onChange={(e) => {
                  if (
                    saveName == null ||
                    saves[e.target.value].romName != saves[saveName].romName
                  ) {
                    setPatchName(null);
                    setPatchVersion(null);
                  }
                  setSaveName(e.target.value);
                }}
              >
                {Object.keys(KNOWN_ROMS).flatMap((romName) => {
                  const saveNames = groupedSaves[romName] || [];
                  if (saveNames.length == 0) {
                    return [];
                  }

                  saveNames.sort();

                  return [
                    [
                      <ListSubheader key="title" sx={{ userSelect: "none" }}>
                        {KNOWN_ROMS[romName].title[i18n.resolvedLanguage]}
                      </ListSubheader>,
                      ...saveNames.map((v) => {
                        return (
                          <MenuItem
                            key={v}
                            value={v}
                            disabled={romNames.indexOf(romName) == -1}
                          >
                            {opponentSettings?.gameInfo != null &&
                            !Object.keys(patches)
                              .filter(
                                (p) => patches[p].forROM == saves[v].romName
                              )
                              .flatMap((p) =>
                                Object.keys(patches[p].versions).map(
                                  (v) =>
                                    patches[p].versions[v].netplayCompatibility
                                )
                              )
                              .concat([
                                KNOWN_ROMS[saves[v].romName]
                                  .netplayCompatibility,
                              ])
                              .some(
                                (nc) =>
                                  nc ==
                                  getNetplayCompatibility(
                                    opponentSettings!.gameInfo!
                                  )
                              ) ? (
                              <Tooltip
                                title={
                                  <Trans i18nKey="play:incompatible-game" />
                                }
                              >
                                <WarningIcon
                                  color="warning"
                                  sx={{
                                    fontSize: "1em",
                                    marginRight: "8px",
                                    verticalAlign: "middle",
                                  }}
                                />
                              </Tooltip>
                            ) : opponentAvailableGames.length > 0 &&
                              !opponentAvailableGames.some(
                                (g) => g.rom == romName
                              ) ? (
                              <Tooltip
                                title={<Trans i18nKey="play:no-remote-copy" />}
                              >
                                <WarningIcon
                                  color="warning"
                                  sx={{
                                    fontSize: "1em",
                                    marginRight: "8px",
                                    verticalAlign: "middle",
                                  }}
                                />
                              </Tooltip>
                            ) : null}{" "}
                            {v}
                          </MenuItem>
                        );
                      }),
                    ],
                  ];
                })}
              </Select>
            </FormControl>
            <Tooltip title={<Trans i18nKey="play:open-dir" />}>
              <IconButton
                onClick={() => {
                  if (saveName == null) {
                    shell.openPath(getBasePath(app));
                  } else {
                    shell.showItemInFolder(
                      path.join(getSavesPath(app), saveName)
                    );
                  }
                }}
              >
                <FolderOpenIcon />
              </IconButton>
            </Tooltip>
            <Tooltip title={<Trans i18nKey="play:rescan" />}>
              <IconButton
                onClick={() => {
                  (async () => {
                    await Promise.allSettled([
                      rescanROMs(),
                      rescanPatches(),
                      rescanSaves(),
                    ]);
                  })();
                }}
              >
                <RefreshIcon />
              </IconButton>
            </Tooltip>
          </Stack>
          <Collapse in={patchOptionsOpen}>
            <Stack
              flexGrow={0}
              flexShrink={0}
              justifyContent="flex-end"
              direction="row"
              spacing={1}
              sx={{ px: 1, mt: 1 }}
            >
              <FormControl fullWidth size="small" sx={{ width: "250%" }}>
                <InputLabel id="game-label">
                  <Trans i18nKey="play:patch-name" />
                </InputLabel>
                <Select
                  labelId="game-label"
                  disabled={saveName == null || battleReady}
                  size="small"
                  value={JSON.stringify(patchName)}
                  label={<Trans i18nKey="play:patch-name" />}
                  onChange={(e) => {
                    setPatchName(JSON.parse(e.target.value));
                    setPatchVersion(null);
                  }}
                  renderValue={(v) => {
                    const patchName = JSON.parse(v);
                    if (patchName == null) {
                      return (
                        <>
                          {opponentSettings?.gameInfo != null &&
                          saveName != null &&
                          KNOWN_ROMS[saves[saveName].romName]
                            .netplayCompatibility !=
                            getNetplayCompatibility(
                              opponentSettings.gameInfo
                            ) ? (
                            <Tooltip
                              title={<Trans i18nKey="play:incompatible-game" />}
                            >
                              <WarningIcon
                                color="warning"
                                sx={{
                                  fontSize: "1em",
                                  marginRight: "8px",
                                  verticalAlign: "middle",
                                }}
                              />
                            </Tooltip>
                          ) : opponentAvailableGames.length > 0 &&
                            !opponentAvailableGames.some(
                              (g) =>
                                save != null &&
                                g.rom == save.romName &&
                                g.patch == null
                            ) ? (
                            <Tooltip
                              title={<Trans i18nKey="play:no-remote-copy" />}
                            >
                              <WarningIcon
                                color="warning"
                                sx={{
                                  fontSize: "1em",
                                  marginRight: "8px",
                                  verticalAlign: "middle",
                                }}
                              />
                            </Tooltip>
                          ) : null}{" "}
                          <Trans i18nKey="play:unpatched" />
                        </>
                      );
                    }
                    return (
                      <>
                        {opponentSettings?.gameInfo != null &&
                        saveName != null &&
                        !Object.keys(patches[patchName].versions)
                          .map(
                            (v) =>
                              patches[patchName].versions[v]
                                .netplayCompatibility
                          )
                          .some(
                            (nc) =>
                              nc ==
                              getNetplayCompatibility(
                                opponentSettings!.gameInfo!
                              )
                          ) ? (
                          <Tooltip
                            title={<Trans i18nKey="play:incompatible-game" />}
                          >
                            <WarningIcon
                              color="warning"
                              sx={{
                                fontSize: "1em",
                                marginRight: "8px",
                                verticalAlign: "middle",
                              }}
                            />
                          </Tooltip>
                        ) : opponentAvailableGames.length > 0 &&
                          !opponentAvailableGames.some(
                            (g) =>
                              save != null &&
                              g.rom == save.romName &&
                              g.patch != null &&
                              g.patch.name == patchName
                          ) ? (
                          <Tooltip
                            title={<Trans i18nKey="play:no-remote-copy" />}
                          >
                            <WarningIcon
                              color="warning"
                              sx={{
                                fontSize: "1em",
                                marginRight: "8px",
                                verticalAlign: "middle",
                              }}
                            />
                          </Tooltip>
                        ) : null}{" "}
                        {patches[patchName].title}{" "}
                        <small>
                          <Trans
                            i18nKey="play:patch-byline"
                            values={{
                              authors: listFormatter.format(
                                patches[patchName].authors.flatMap(({ name }) =>
                                  name != null ? [name] : []
                                )
                              ),
                            }}
                          />
                        </small>
                      </>
                    );
                  }}
                  fullWidth
                >
                  <MenuItem value="null">
                    {opponentSettings?.gameInfo != null &&
                    saveName != null &&
                    KNOWN_ROMS[saves[saveName].romName].netplayCompatibility !=
                      getNetplayCompatibility(opponentSettings.gameInfo) ? (
                      <Tooltip
                        title={<Trans i18nKey="play:incompatible-game" />}
                      >
                        <WarningIcon
                          color="warning"
                          sx={{
                            fontSize: "1em",
                            marginRight: "8px",
                            verticalAlign: "middle",
                          }}
                        />
                      </Tooltip>
                    ) : opponentAvailableGames.length > 0 &&
                      !opponentAvailableGames.some(
                        (g) =>
                          save != null &&
                          g.rom == save.romName &&
                          g.patch == null
                      ) ? (
                      <Tooltip title={<Trans i18nKey="play:no-remote-copy" />}>
                        <WarningIcon
                          color="warning"
                          sx={{
                            fontSize: "1em",
                            marginRight: "8px",
                            verticalAlign: "middle",
                          }}
                        />
                      </Tooltip>
                    ) : null}{" "}
                    <Trans i18nKey="play:unpatched" />
                  </MenuItem>
                  {eligiblePatchNames.map((patchName) => {
                    const v = JSON.stringify(patchName);
                    return (
                      <MenuItem key={v} value={v}>
                        <ListItemText
                          primary={
                            <>
                              {opponentSettings?.gameInfo != null &&
                              !Object.keys(patches[patchName].versions)
                                .map(
                                  (v) =>
                                    patches[patchName].versions[v]
                                      .netplayCompatibility
                                )
                                .some(
                                  (nc) =>
                                    nc ==
                                    getNetplayCompatibility(
                                      opponentSettings!.gameInfo!
                                    )
                                ) ? (
                                <Tooltip
                                  title={
                                    <Trans i18nKey="play:incompatible-game" />
                                  }
                                >
                                  <WarningIcon
                                    color="warning"
                                    sx={{
                                      fontSize: "1em",
                                      marginRight: "8px",
                                      verticalAlign: "middle",
                                    }}
                                  />
                                </Tooltip>
                              ) : opponentAvailableGames.length > 0 &&
                                !opponentAvailableGames.some(
                                  (g) =>
                                    save != null &&
                                    g.rom == save.romName &&
                                    g.patch != null &&
                                    g.patch.name == patchName
                                ) ? (
                                <Tooltip
                                  title={
                                    <Trans i18nKey="play:no-remote-copy" />
                                  }
                                >
                                  <WarningIcon
                                    color="warning"
                                    sx={{
                                      fontSize: "1em",
                                      marginRight: "8px",
                                      verticalAlign: "middle",
                                    }}
                                  />
                                </Tooltip>
                              ) : null}{" "}
                              {patches[patchName].title}
                            </>
                          }
                          secondary={
                            <Trans
                              i18nKey="play:patch-byline"
                              values={{
                                authors: listFormatter.format(
                                  patches[patchName].authors.flatMap(
                                    ({ name }) => (name != null ? [name] : [])
                                  )
                                ),
                              }}
                            />
                          }
                        />
                      </MenuItem>
                    );
                  })}
                </Select>
              </FormControl>
              <FormControl fullWidth size="small">
                <InputLabel id="patch-version-label">
                  <Trans i18nKey="play:patch-version" />
                </InputLabel>
                <Select
                  labelId="patch-version-label"
                  disabled={
                    saveName == null || patchName == null || battleReady
                  }
                  size="small"
                  value={patchVersion || ""}
                  label={<Trans i18nKey="play:patch-version" />}
                  onChange={(e) => {
                    setPatchVersion(e.target.value);
                  }}
                  fullWidth
                >
                  {patchVersions != null
                    ? patchVersions.map((version) => {
                        return (
                          <MenuItem key={version} value={version}>
                            {opponentSettings?.gameInfo != null &&
                            patchName != null &&
                            patchVersion != null &&
                            patches[patchName].versions[patchVersion]
                              .netplayCompatibility !=
                              getNetplayCompatibility(
                                opponentSettings!.gameInfo!
                              ) ? (
                              <Tooltip
                                title={
                                  <Trans i18nKey="play:incompatible-game" />
                                }
                              >
                                <WarningIcon
                                  color="warning"
                                  sx={{
                                    fontSize: "1em",
                                    marginRight: "8px",
                                    verticalAlign: "middle",
                                  }}
                                />
                              </Tooltip>
                            ) : opponentAvailableGames.length > 0 &&
                              !opponentAvailableGames.some(
                                (g) =>
                                  save != null &&
                                  g.rom == save.romName &&
                                  g.patch != null &&
                                  g.patch.name == patchName &&
                                  g.patch.version == version
                              ) ? (
                              <Tooltip
                                title={<Trans i18nKey="play:no-remote-copy" />}
                              >
                                <WarningIcon
                                  color="warning"
                                  sx={{
                                    fontSize: "1em",
                                    marginRight: "8px",
                                    verticalAlign: "middle",
                                  }}
                                />
                              </Tooltip>
                            ) : null}{" "}
                            {version}
                          </MenuItem>
                        );
                      })
                    : []}
                </Select>
              </FormControl>
            </Stack>
          </Collapse>
        </Box>
        {saveName != null ? (
          <Stack direction="column" flexGrow={1}>
            <SaveViewerWrapper
              romName={saves[saveName].romName}
              filename={saveName}
              incarnation={incarnation}
            />
          </Stack>
        ) : (
          <Box
            flexGrow={1}
            display="flex"
            justifyContent="center"
            alignItems="center"
            sx={{ userSelect: "none", color: "text.disabled" }}
          >
            <Stack alignItems="center" spacing={1}>
              <SportsEsportsOutlinedIcon sx={{ fontSize: "4rem" }} />
              <Typography variant="h6">
                <Trans i18nKey="play:no-save-selected" />
              </Typography>
            </Stack>
          </Box>
        )}
        <BattleStarter
          saveName={saveName}
          patch={
            patchVersion != null
              ? {
                  name: patchName!,
                  version: patchVersion,
                }
              : null
          }
          onExit={() => {
            setIncarnation((incarnation) => incarnation + 1);
          }}
          onReadyChange={(ready) => {
            setBattleReady(ready);
          }}
          onOpponentSettingsChange={(settings) => {
            setOpponentSettings(settings);
          }}
        />
      </Stack>
    </Box>
  );
}

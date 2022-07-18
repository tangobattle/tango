import { readFile } from "fs/promises";
import { sortBy } from "lodash-es";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import { TransitionGroup } from "react-transition-group";
import semver from "semver";

import { shell } from "@electron/remote";
import ArrowBackIcon from "@mui/icons-material/ArrowBack";
import CloseIcon from "@mui/icons-material/Close";
import FolderOpenOutlinedIcon from "@mui/icons-material/FolderOpenOutlined";
import RefreshIcon from "@mui/icons-material/Refresh";
import SportsEsportsOutlinedIcon from "@mui/icons-material/SportsEsportsOutlined";
import WarningIcon from "@mui/icons-material/Warning";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Collapse from "@mui/material/Collapse";
import FormControl from "@mui/material/FormControl";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import InputLabel from "@mui/material/InputLabel";
import List from "@mui/material/List";
import ListItemButton from "@mui/material/ListItemButton";
import ListItemText from "@mui/material/ListItemText";
import ListSubheader from "@mui/material/ListSubheader";
import MenuItem from "@mui/material/MenuItem";
import Modal from "@mui/material/Modal";
import OutlinedInput from "@mui/material/OutlinedInput";
import Select from "@mui/material/Select";
import Slide from "@mui/material/Slide";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { makeROM } from "../../../game";
import { SetSettings } from "../../../protos/generated/lobby";
import { FAMILY_BY_ROM_NAME, KNOWN_ROM_FAMILIES } from "../../../rom";
import { Editor, editorClassForGameFamily } from "../../../saveedit";
import { useGetPatchPath, useGetROMPath } from "../../hooks";
import { fallbackLng } from "../../i18n";
import BattleStarter, { useGetNetplayCompatibility } from "../BattleStarter";
import { useConfig } from "../ConfigContext";
import { AllowEdits as AllowFolderEdits } from "../FolderViewer";
import { usePatches } from "../PatchesContext";
import { useROMs } from "../ROMsContext";
import { useSaves } from "../SavesContext";
import SaveViewer from "../SaveViewer";

function SaveSelector({
  initialSelection,
  opponentSettings,
  onSelect,
  onClose,
}: {
  initialSelection: { romName: string; saveName: string } | null;
  opponentSettings: SetSettings | null;
  onSelect: (v: { romName: string; saveName: string } | null) => void;
  onClose: () => void;
}) {
  const { i18n } = useTranslation();
  const { config } = useConfig();

  const getNetplayCompatibility = useGetNetplayCompatibility();
  const opponentAvailableGames =
    opponentSettings != null ? opponentSettings.availableGames : [];

  const { patches, rescan: rescanPatches } = usePatches();
  const { saves, rescan: rescanSaves } = useSaves();
  const { roms, rescan: rescanROMs } = useROMs();

  const [selectedROM, setSelectedROM] = React.useState<string | null>(
    initialSelection != null ? initialSelection.romName : null
  );

  const groupedSaves: { [key: string]: string[] } = {};
  const saveNames = Object.keys(saves);
  saveNames.sort();
  for (const k of saveNames) {
    for (const romName of saves[k]) {
      groupedSaves[romName] = groupedSaves[romName] || [];
      groupedSaves[romName].push(k);
    }
  }

  for (const saves of Object.values(groupedSaves)) {
    saves.sort();
  }

  const romNames = sortBy(
    Object.values(KNOWN_ROM_FAMILIES).flatMap((f) => Object.keys(f.versions)),
    (k) => {
      const family = KNOWN_ROM_FAMILIES[FAMILY_BY_ROM_NAME[k]];
      const romInfo = family.versions[k];
      return [
        Object.prototype.hasOwnProperty.call(roms, k) ? 0 : 1,
        family.lang == i18n.resolvedLanguage ? 0 : 1,
        family.lang,
        FAMILY_BY_ROM_NAME[k],
        romInfo.order,
      ];
    }
  );

  return (
    <Stack
      sx={{
        width: "100%",
        height: "100%",
        bgcolor: "background.paper",
      }}
      direction="column"
    >
      <Stack sx={{ flexGrow: 1 }}>
        <Stack
          spacing={1}
          flexGrow={0}
          flexShrink={0}
          direction="row"
          sx={{ p: 1 }}
        >
          <FormControl fullWidth size="small">
            <InputLabel id="select-game-label">
              <Trans i18nKey="play:select-game" />
            </InputLabel>
            <Select
              label={<Trans i18nKey="play:select-game" />}
              value={selectedROM ?? ""}
              onChange={(e) => {
                setSelectedROM(e.target.value);
              }}
            >
              {romNames.map((romName) => (
                <MenuItem
                  disabled={
                    !Object.prototype.hasOwnProperty.call(roms, romName)
                  }
                  key={romName}
                  sx={{ userSelect: "none" }}
                  value={romName}
                  selected={romName === selectedROM}
                >
                  {opponentSettings?.gameInfo != null &&
                  !Object.values(patches)
                    .flatMap((p) =>
                      Object.values(p.versions).flatMap((v) =>
                        v.forROMs.some((r) => r.name == romName)
                          ? [v.netplayCompatibility]
                          : []
                      )
                    )
                    .concat([FAMILY_BY_ROM_NAME[romName]])
                    .some(
                      (nc) =>
                        nc ==
                        getNetplayCompatibility(opponentSettings!.gameInfo!)
                    ) ? (
                    <Tooltip title={<Trans i18nKey="play:incompatible-game" />}>
                      <WarningIcon
                        color="warning"
                        fontSize="inherit"
                        sx={{
                          mr: 0.5,
                          verticalAlign: "middle",
                        }}
                      />
                    </Tooltip>
                  ) : opponentAvailableGames.length > 0 &&
                    !opponentAvailableGames.some((g) => g.rom == romName) ? (
                    <Tooltip title={<Trans i18nKey="play:no-remote-copy" />}>
                      <WarningIcon
                        color="warning"
                        fontSize="inherit"
                        sx={{
                          mr: 0.5,
                          verticalAlign: "middle",
                        }}
                      />
                    </Tooltip>
                  ) : null}{" "}
                  {(() => {
                    const family =
                      KNOWN_ROM_FAMILIES[FAMILY_BY_ROM_NAME[romName]];

                    const familyName =
                      family.title[i18n.resolvedLanguage] ||
                      family.title[fallbackLng];

                    const romTitle = family.versions[romName].title;

                    if (romTitle == null) {
                      return <>{familyName}</>;
                    }

                    return (
                      <Trans
                        i18nKey="play:rom-name"
                        values={{
                          familyName,
                          versionName:
                            romTitle[i18n.resolvedLanguage] ||
                            romTitle[fallbackLng],
                        }}
                      />
                    );
                  })()}
                </MenuItem>
              ))}
            </Select>
          </FormControl>
          <Tooltip title={<Trans i18nKey="play:open-dir" />}>
            <IconButton
              edge="end"
              onClick={() => {
                shell.openPath(config.paths.saves);
              }}
            >
              <FolderOpenOutlinedIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title={<Trans i18nKey="play:rescan" />}>
            <IconButton
              onClick={() => {
                (async () => {
                  await Promise.allSettled([
                    rescanROMs(),
                    rescanSaves(),
                    rescanPatches(),
                  ]);
                })();
              }}
            >
              <RefreshIcon />
            </IconButton>
          </Tooltip>
          <Tooltip title={<Trans i18nKey="common:close" />}>
            <IconButton
              onClick={() => {
                onClose();
              }}
            >
              <CloseIcon />
            </IconButton>
          </Tooltip>
        </Stack>
        <Box display="flex" flexGrow={1} overflow="auto" sx={{ height: 0 }}>
          {selectedROM != null ? (
            <List disablePadding dense key={selectedROM} sx={{ flexGrow: 1 }}>
              <ListSubheader disableGutters>
                <ListItemButton
                  color="primary"
                  onClick={() => {
                    setSelectedROM(null);
                  }}
                >
                  <ListItemText
                    primaryTypographyProps={{
                      sx: {
                        display: "inline-flex",
                        alignItems: "center",
                      },
                    }}
                    sx={{
                      color: "text.secondary",
                    }}
                  >
                    <ArrowBackIcon fontSize="inherit" sx={{ mr: 0.5 }} />{" "}
                    <Trans i18nKey="play:return-to-games-list" />
                  </ListItemText>
                </ListItemButton>
              </ListSubheader>
              <TransitionGroup>
                {(groupedSaves[selectedROM] || []).map((saveName) => (
                  <Collapse key={saveName}>
                    <ListItemButton
                      selected={
                        initialSelection != null
                          ? selectedROM == initialSelection.romName &&
                            saveName == initialSelection.saveName
                          : false
                      }
                      onClick={() => {
                        onSelect({ romName: selectedROM, saveName });
                      }}
                    >
                      <ListItemText>
                        {opponentSettings?.gameInfo != null &&
                        !Object.values(patches)
                          .flatMap((p) =>
                            Object.values(p.versions).flatMap((v) =>
                              v.forROMs.some((r) => r.name == selectedROM)
                                ? [v.netplayCompatibility]
                                : []
                            )
                          )
                          .concat([FAMILY_BY_ROM_NAME[selectedROM]])
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
                              fontSize="inherit"
                              sx={{
                                verticalAlign: "middle",
                              }}
                            />
                          </Tooltip>
                        ) : opponentAvailableGames.length > 0 &&
                          !opponentAvailableGames.some(
                            (g) => g.rom == selectedROM
                          ) ? (
                          <Tooltip
                            title={<Trans i18nKey="play:no-remote-copy" />}
                          >
                            <WarningIcon
                              color="warning"
                              fontSize="inherit"
                              sx={{
                                verticalAlign: "middle",
                              }}
                            />
                          </Tooltip>
                        ) : null}{" "}
                        {saveName}
                      </ListItemText>
                    </ListItemButton>
                  </Collapse>
                ))}
              </TransitionGroup>
            </List>
          ) : (
            <List disablePadding dense key={selectedROM} sx={{ flexGrow: 1 }}>
              <TransitionGroup>
                {romNames.map((romName) => (
                  <Collapse key={romName}>
                    <ListItemButton
                      disabled={
                        !Object.prototype.hasOwnProperty.call(roms, romName)
                      }
                      onClick={() => {
                        setSelectedROM(romName);
                      }}
                    >
                      <ListItemText>
                        {opponentSettings?.gameInfo != null &&
                        !Object.values(patches)
                          .flatMap((p) =>
                            Object.values(p.versions).flatMap((v) =>
                              v.forROMs.some((r) => r.name == romName)
                                ? [v.netplayCompatibility]
                                : []
                            )
                          )
                          .concat([FAMILY_BY_ROM_NAME[romName]])
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
                              fontSize="inherit"
                              sx={{
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
                              fontSize="inherit"
                              sx={{
                                verticalAlign: "middle",
                              }}
                            />
                          </Tooltip>
                        ) : null}{" "}
                        {(() => {
                          const family =
                            KNOWN_ROM_FAMILIES[FAMILY_BY_ROM_NAME[romName]];

                          const familyName =
                            family.title[i18n.resolvedLanguage] ||
                            family.title[fallbackLng];

                          const romTitle = family.versions[romName].title;

                          if (romTitle == null) {
                            return <>{familyName}</>;
                          }

                          return (
                            <Trans
                              i18nKey="play:rom-name"
                              values={{
                                familyName,
                                versionName:
                                  romTitle[i18n.resolvedLanguage] ||
                                  romTitle[fallbackLng],
                              }}
                            />
                          );
                        })()}
                      </ListItemText>
                    </ListItemButton>
                  </Collapse>
                ))}
              </TransitionGroup>
            </List>
          )}
        </Box>
      </Stack>
    </Stack>
  );
}

function SaveViewerWrapper({
  filename,
  romName,
  patch,
  incarnation,
  battleReady,
}: {
  filename: string;
  romName: string;
  patch: { name: string; version: string } | null;
  incarnation: number;
  battleReady: boolean;
}) {
  const { config } = useConfig();
  const [editor, setEditor] = React.useState<Editor | null>(null);

  const getROMPath = useGetROMPath();
  const getPatchPath = useGetPatchPath();

  const romPath = getROMPath(romName);
  const patchPath = patch != null ? getPatchPath(romName, patch) : null;

  React.useEffect(() => {
    (async () => {
      const Editor = editorClassForGameFamily(FAMILY_BY_ROM_NAME[romName]);
      setEditor(
        new Editor(
          Editor.sramDumpToRaw(
            (await readFile(path.join(config.paths.saves, filename))).buffer
          ),
          await makeROM(romPath, patchPath),
          romName
        )
      );
    })();
  }, [config.paths.saves, filename, romName, incarnation, romPath, patchPath]);

  if (editor == null) {
    return null;
  }

  return (
    <SaveViewer
      allowFolderEdits={
        battleReady ? AllowFolderEdits.None : AllowFolderEdits.All
      }
      editor={editor}
    />
  );
}

export default function SavesPane({ active }: { active: boolean }) {
  const { saves } = useSaves();
  const { patches } = usePatches();
  const { roms } = useROMs();
  const { i18n } = useTranslation();

  const [saveSelectorOpen, setSaveSelectorOpen] = React.useState(false);
  const [battleReady, setBattleReady] = React.useState(false);

  const [selectedSave_, setSelectedSave] = React.useState<{
    romName: string;
    saveName: string;
  } | null>(null);
  const [incarnation, setIncarnation] = React.useState(0);
  const [opponentSettings, setOpponentSettings] =
    React.useState<SetSettings | null>(null);
  const opponentAvailableGames = opponentSettings?.availableGames ?? [];

  const getNetplayCompatibility = useGetNetplayCompatibility();

  const selectedSave =
    selectedSave_ != null &&
    Object.prototype.hasOwnProperty.call(saves, selectedSave_.saveName) &&
    Object.prototype.hasOwnProperty.call(roms, selectedSave_.romName)
      ? selectedSave_
      : null;

  const [patchName_, setPatchName] = React.useState<string | null>(null);
  const patchName =
    patchName_ != null &&
    Object.prototype.hasOwnProperty.call(patches, patchName_) &&
    selectedSave != null &&
    Object.values(patches[patchName_].versions).some((p) =>
      p.forROMs.some((r) => r.name == selectedSave.romName)
    )
      ? patchName_
      : null;

  const eligiblePatchNames = React.useMemo(() => {
    const eligiblePatchNames =
      selectedSave != null
        ? Object.keys(patches).filter((p) =>
            Object.values(patches[p].versions).some((v) =>
              v.forROMs.some((r) => r.name == selectedSave.romName)
            )
          )
        : [];
    eligiblePatchNames.sort();
    return eligiblePatchNames;
  }, [patches, selectedSave]);

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
        flexGrow: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Stack sx={{ flexGrow: 1, width: 0 }}>
        <Box flexGrow={0} flexShrink={0} sx={{ px: 1, pt: 1 }}>
          <Stack spacing={1} flexGrow={0} flexShrink={0} direction="row">
            <Modal
              open={saveSelectorOpen}
              onClose={(_e, _reason) => {
                setSaveSelectorOpen(false);
              }}
            >
              <Slide in={saveSelectorOpen} direction="up" unmountOnExit>
                <Box>
                  <SaveSelector
                    initialSelection={selectedSave}
                    opponentSettings={opponentSettings}
                    onClose={() => {
                      setSaveSelectorOpen(false);
                    }}
                    onSelect={(selected) => {
                      setSelectedSave(selected);
                      setSaveSelectorOpen(false);
                    }}
                  />
                </Box>
              </Slide>
            </Modal>

            <FormControl fullWidth size="small">
              <InputLabel id="select-save-label">
                <Trans i18nKey="play:select-save" />
              </InputLabel>
              <OutlinedInput
                label={<Trans i18nKey="play:select-save" />}
                readOnly
                disabled={battleReady}
                value={selectedSave != null ? JSON.stringify(selectedSave) : ""}
                inputComponent={React.forwardRef(
                  ({ value, className }, ref) => (
                    <Box
                      ref={ref}
                      sx={{
                        height: "auto",
                        minHeight: "1.4375em",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                      }}
                      className={className}
                    >
                      {(() => {
                        if (value == "") {
                          return null;
                        }

                        const selection = JSON.parse(value);

                        return (
                          <>
                            {opponentSettings?.gameInfo != null &&
                            !Object.values(patches)
                              .flatMap((p) =>
                                Object.values(p.versions).flatMap((v) =>
                                  v.forROMs.some(
                                    (r) => r.name == selection.romName
                                  )
                                    ? [v.netplayCompatibility]
                                    : []
                                )
                              )
                              .concat([FAMILY_BY_ROM_NAME[selection.romName]])
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
                                  fontSize="inherit"
                                  sx={{
                                    verticalAlign: "middle",
                                  }}
                                />
                              </Tooltip>
                            ) : opponentAvailableGames.length > 0 &&
                              !opponentAvailableGames.some(
                                (g) => g.rom == selection.romName
                              ) ? (
                              <Tooltip
                                title={<Trans i18nKey="play:no-remote-copy" />}
                              >
                                <WarningIcon
                                  color="warning"
                                  fontSize="inherit"
                                  sx={{
                                    verticalAlign: "middle",
                                  }}
                                />
                              </Tooltip>
                            ) : null}{" "}
                            {selection.saveName}{" "}
                            <small>
                              {(() => {
                                const family =
                                  KNOWN_ROM_FAMILIES[
                                    FAMILY_BY_ROM_NAME[selection.romName]
                                  ];

                                const familyName =
                                  family.title[i18n.resolvedLanguage] ||
                                  family.title[fallbackLng];

                                const romTitle =
                                  family.versions[selection.romName].title;

                                if (romTitle == null) {
                                  return <>{familyName}</>;
                                }

                                return (
                                  <Trans
                                    i18nKey="play:rom-name"
                                    values={{
                                      familyName,
                                      versionName:
                                        romTitle[i18n.resolvedLanguage] ||
                                        romTitle[fallbackLng],
                                    }}
                                  />
                                );
                              })()}
                            </small>
                          </>
                        );
                      })()}
                    </Box>
                  )
                )}
                onClick={() => {
                  setSaveSelectorOpen(true);
                }}
                endAdornment={
                  <InputAdornment position="end">
                    <Button
                      sx={{ marginRight: "-10px" }}
                      size="small"
                      variant="contained"
                      disabled={battleReady}
                      onClick={(e) => {
                        e.stopPropagation();
                        setSaveSelectorOpen(true);
                      }}
                    >
                      <Trans i18nKey="play:select-save-button" />
                    </Button>
                  </InputAdornment>
                }
              />
            </FormControl>
          </Stack>
          <Stack
            flexGrow={0}
            flexShrink={0}
            justifyContent="flex-end"
            direction="row"
            spacing={1}
            sx={{ mt: 1 }}
          >
            <FormControl fullWidth size="small">
              <InputLabel id="game-label">
                <Trans i18nKey="play:patch-name" />
              </InputLabel>
              <Select
                labelId="game-label"
                disabled={selectedSave == null || battleReady}
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
                        selectedSave != null &&
                        FAMILY_BY_ROM_NAME[selectedSave.romName] !=
                          getNetplayCompatibility(opponentSettings.gameInfo) ? (
                          <Tooltip
                            title={<Trans i18nKey="play:incompatible-game" />}
                          >
                            <WarningIcon
                              color="warning"
                              fontSize="inherit"
                              sx={{
                                verticalAlign: "middle",
                              }}
                            />
                          </Tooltip>
                        ) : opponentAvailableGames.length > 0 &&
                          !opponentAvailableGames.some(
                            (g) =>
                              selectedSave != null &&
                              g.rom == selectedSave.romName &&
                              g.patch == null
                          ) ? (
                          <Tooltip
                            title={<Trans i18nKey="play:no-remote-copy" />}
                          >
                            <WarningIcon
                              color="warning"
                              fontSize="inherit"
                              sx={{
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
                      selectedSave != null &&
                      !Object.keys(patches[patchName].versions)
                        .map(
                          (v) =>
                            patches[patchName].versions[v].netplayCompatibility
                        )
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
                            fontSize="inherit"
                            sx={{
                              verticalAlign: "middle",
                            }}
                          />
                        </Tooltip>
                      ) : opponentAvailableGames.length > 0 &&
                        !opponentAvailableGames.some(
                          (g) =>
                            selectedSave != null &&
                            g.rom == selectedSave.romName &&
                            g.patch != null &&
                            g.patch.name == patchName
                        ) ? (
                        <Tooltip
                          title={<Trans i18nKey="play:no-remote-copy" />}
                        >
                          <WarningIcon
                            color="warning"
                            fontSize="inherit"
                            sx={{
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
                  selectedSave != null &&
                  FAMILY_BY_ROM_NAME[selectedSave.romName] !=
                    getNetplayCompatibility(opponentSettings.gameInfo) ? (
                    <Tooltip title={<Trans i18nKey="play:incompatible-game" />}>
                      <WarningIcon
                        color="warning"
                        fontSize="inherit"
                        sx={{
                          verticalAlign: "middle",
                        }}
                      />
                    </Tooltip>
                  ) : opponentAvailableGames.length > 0 &&
                    !opponentAvailableGames.some(
                      (g) =>
                        selectedSave != null &&
                        g.rom == selectedSave.romName &&
                        g.patch == null
                    ) ? (
                    <Tooltip title={<Trans i18nKey="play:no-remote-copy" />}>
                      <WarningIcon
                        color="warning"
                        fontSize="inherit"
                        sx={{
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
                                  fontSize="inherit"
                                  sx={{
                                    verticalAlign: "middle",
                                  }}
                                />
                              </Tooltip>
                            ) : opponentAvailableGames.length > 0 &&
                              !opponentAvailableGames.some(
                                (g) =>
                                  selectedSave != null &&
                                  g.rom == selectedSave.romName &&
                                  g.patch != null &&
                                  g.patch.name == patchName
                              ) ? (
                              <Tooltip
                                title={<Trans i18nKey="play:no-remote-copy" />}
                              >
                                <WarningIcon
                                  color="warning"
                                  fontSize="inherit"
                                  sx={{
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
                                patches[patchName].authors.flatMap(({ name }) =>
                                  name != null ? [name] : []
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
            <FormControl fullWidth size="small" sx={{ width: "200px" }}>
              <InputLabel id="patch-version-label">
                <Trans i18nKey="play:patch-version" />
              </InputLabel>
              <Select
                labelId="patch-version-label"
                disabled={
                  selectedSave == null || patchName == null || battleReady
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
                              title={<Trans i18nKey="play:incompatible-game" />}
                            >
                              <WarningIcon
                                color="warning"
                                fontSize="inherit"
                                sx={{
                                  verticalAlign: "middle",
                                }}
                              />
                            </Tooltip>
                          ) : opponentAvailableGames.length > 0 &&
                            !opponentAvailableGames.some(
                              (g) =>
                                selectedSave != null &&
                                g.rom == selectedSave.romName &&
                                g.patch != null &&
                                g.patch.name == patchName &&
                                g.patch.version == version
                            ) ? (
                            <Tooltip
                              title={<Trans i18nKey="play:no-remote-copy" />}
                            >
                              <WarningIcon
                                color="warning"
                                fontSize="inherit"
                                sx={{
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
        </Box>
        {selectedSave != null ? (
          <Stack direction="column" flexGrow={1}>
            <SaveViewerWrapper
              romName={selectedSave.romName}
              patch={
                patchVersion != null
                  ? {
                      name: patchName!,
                      version: patchVersion,
                    }
                  : null
              }
              filename={selectedSave.saveName}
              incarnation={incarnation}
              battleReady={battleReady}
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
          saveName={selectedSave != null ? selectedSave.saveName : null}
          gameInfo={
            selectedSave != null
              ? {
                  rom: selectedSave.romName,
                  patch:
                    patchVersion != null
                      ? {
                          name: patchName!,
                          version: patchVersion,
                        }
                      : undefined,
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

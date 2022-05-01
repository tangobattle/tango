import * as datefns from "date-fns";
import { readFile } from "fs/promises";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import semver from "semver";

import { app, shell } from "@electron/remote";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import RefreshIcon from "@mui/icons-material/Refresh";
import SportsEsportsOutlinedIcon from "@mui/icons-material/SportsEsportsOutlined";
import SportsMmaIcon from "@mui/icons-material/SportsMma";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Collapse from "@mui/material/Collapse";
import FormControl from "@mui/material/FormControl";
import IconButton from "@mui/material/IconButton";
import InputAdornment from "@mui/material/InputAdornment";
import InputLabel from "@mui/material/InputLabel";
import ListItemText from "@mui/material/ListItemText";
import ListSubheader from "@mui/material/ListSubheader";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Slider from "@mui/material/Slider";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { getReplaysPath, getSavesPath } from "../../../paths";
import { KNOWN_ROMS } from "../../../rom";
import { Editor } from "../../../saveedit/bn6";
import { CopyButton } from "../CopyButton";
import { CoreSupervisor } from "../CoreSupervisor";
import { usePatches } from "../PatchesContext";
import { useROMs } from "../ROMsContext";
import { useSaves } from "../SavesContext";
import SaveViewer from "../SaveViewer";

const MATCH_TYPES = ["single", "triple"];

function SaveViewerWrapper({
  filename,
  romName,
  incarnation,
}: {
  filename: string;
  romName: string;
  incarnation: number;
}) {
  const [editor, setEditor] = React.useState<Editor | null>(null);

  React.useEffect(() => {
    (async () => {
      const e = new Editor(
        Editor.sramDumpToRaw(
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

export default function PlayPane({ active }: { active: boolean }) {
  const { saves, rescan: rescanSaves } = useSaves();
  const { patches, rescan: rescanPatches } = usePatches();
  const { roms, rescan: rescanROMs } = useROMs();
  const { i18n } = useTranslation();

  const [patchOptionsOpen, setPatchOptionsOpen] = React.useState(false);
  const [extraOptionsOpen, setExtraOptionsOpen] = React.useState(false);

  const [saveName_, setSaveName] = React.useState<string | null>(null);
  const [startedState, setStartedState] = React.useState<{
    linkCode: string | null;
  } | null>(null);
  const [incarnation, setIncarnation] = React.useState(0);

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

  const [patchName, setPatchName] = React.useState<string | null>(null);
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

  const [patchVersion, setPatchVersion] = React.useState<string | null>(null);
  React.useEffect(() => {
    if (patchVersions == null) {
      setPatchVersion(null);
      return;
    }
    setPatchVersion(patchVersions[0]);
  }, [patchVersions]);

  const [matchType, setMatchType] = React.useState(1);
  const [inputDelay, setInputDelay] = React.useState(3);
  const [linkCode, setLinkCode] = React.useState("");

  const romInfo = save != null ? KNOWN_ROMS[save.romName] : null;

  const netplayCompatibility =
    romInfo != null
      ? patchInfo != null &&
        patchVersion != null &&
        patchInfo.versions[patchVersion] != null
        ? patchInfo.versions[patchVersion].netplayCompatibility
        : romInfo.netplayCompatibility
      : "";

  const now = new Date();
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
      <Stack sx={{ flexGrow: 1, width: 0 }} spacing={1}>
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
                renderValue={(v) => {
                  if (v == "") {
                    return null;
                  }
                  return (
                    <>
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
                {romNames.map((romName) => {
                  const saveNames = groupedSaves[romName] || [];
                  saveNames.sort();

                  return [
                    <ListSubheader key="title" sx={{ userSelect: "none" }}>
                      {KNOWN_ROMS[romName].title[i18n.resolvedLanguage]}
                    </ListSubheader>,
                    ...saveNames.map((v) => {
                      return (
                        <MenuItem key={v} value={v}>
                          {v}
                        </MenuItem>
                      );
                    }),
                  ];
                })}
              </Select>
            </FormControl>
            <Tooltip title={<Trans i18nKey="play:open-dir" />}>
              <IconButton
                onClick={() => {
                  if (saveName == null) {
                    shell.openPath(getSavesPath(app));
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
              <Box flexGrow={5} flexShrink={0}>
                <FormControl fullWidth size="small">
                  <InputLabel id="game-label">
                    <Trans i18nKey="play:patch-name" />
                  </InputLabel>
                  <Select
                    labelId="game-label"
                    disabled={saveName == null}
                    size="small"
                    value={JSON.stringify(patchName)}
                    label={<Trans i18nKey={"play:patch-name"} />}
                    onChange={(e) => {
                      setPatchName(JSON.parse(e.target.value));
                      setPatchVersion(null);
                    }}
                    renderValue={(v) => {
                      const patchName = JSON.parse(v);
                      if (patchName == null) {
                        return <Trans i18nKey="play:unpatched" />;
                      }
                      return (
                        <>
                          {patches[patchName].title}{" "}
                          <small>
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
                          </small>
                        </>
                      );
                    }}
                    fullWidth
                  >
                    <MenuItem value="null">
                      <Trans i18nKey="play:unpatched" />
                    </MenuItem>
                    {eligiblePatchNames.map((patchName) => {
                      const v = JSON.stringify(patchName);
                      return (
                        <MenuItem key={v} value={v}>
                          <ListItemText
                            primary={patches[patchName].title}
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
              </Box>
              <Box flexGrow={1} flexShrink={0}>
                <FormControl fullWidth size="small">
                  <InputLabel id="patch-version-label">
                    <Trans i18nKey="play:patch-version" />
                  </InputLabel>
                  <Select
                    labelId="patch-version-label"
                    disabled={saveName == null || patchName == null}
                    size="small"
                    value={patchVersion || ""}
                    label={<Trans i18nKey={"play:patch-version"} />}
                    onChange={(e) => {
                      setPatchVersion(e.target.value);
                    }}
                    fullWidth
                  >
                    {patchVersions != null
                      ? patchVersions.map((version) => {
                          return (
                            <MenuItem key={version} value={version}>
                              {version}
                            </MenuItem>
                          );
                        })
                      : []}
                  </Select>
                </FormControl>
              </Box>
            </Stack>
          </Collapse>
        </Box>
        {saveName != null ? (
          <Box flexGrow={1} display="flex">
            <SaveViewerWrapper
              romName={saves[saveName].romName}
              filename={saveName}
              incarnation={incarnation}
            />
          </Box>
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
        <Stack
          component="form"
          onSubmit={(e: any) => {
            e.preventDefault();
            setStartedState({ linkCode: linkCode != "" ? linkCode : null });
          }}
        >
          <Stack
            flexGrow={0}
            flexShrink={0}
            direction="row"
            justifyContent="flex-end"
            spacing={1}
            sx={{ px: 1 }}
          >
            <Tooltip title={<Trans i18nKey="play:show-hide-extra-options" />}>
              <IconButton
                onClick={() => {
                  setExtraOptionsOpen((o) => !o);
                }}
              >
                {extraOptionsOpen ? (
                  <KeyboardArrowDownIcon />
                ) : (
                  <KeyboardArrowUpIcon />
                )}
              </IconButton>
            </Tooltip>
            <Box flexGrow={1} flexShrink={0}>
              <TextField
                disabled={saveName == null}
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
                fullWidth
                InputProps={{
                  startAdornment: (
                    <InputAdornment position="start" sx={{ mr: 0 }}>
                      {romInfo != null ? (
                        <>
                          {netplayCompatibility}-
                          <Select
                            variant="standard"
                            value={matchType}
                            onChange={(e) => {
                              setMatchType(e.target.value as number);
                            }}
                            renderValue={(v) => MATCH_TYPES[v]}
                            disabled={saveName == null}
                          >
                            {MATCH_TYPES.map((v, k) => (
                              <MenuItem key={k} value={k}>
                                <ListItemText
                                  primary={v}
                                  secondary={
                                    k == 0 ? (
                                      <Trans i18nKey="play:match-type.single" />
                                    ) : k == 1 ? (
                                      <Trans i18nKey="play:match-type.triple" />
                                    ) : null
                                  }
                                />
                              </MenuItem>
                            ))}
                          </Select>
                          -
                        </>
                      ) : null}
                    </InputAdornment>
                  ),
                  endAdornment: (
                    <InputAdornment position="end">
                      <CopyButton
                        disabled={saveName == null}
                        value={`${netplayCompatibility}-${MATCH_TYPES[matchType]}-${linkCode}`}
                      />
                    </InputAdornment>
                  ),
                }}
              />
            </Box>
            <Button
              type="submit"
              variant="contained"
              startIcon={linkCode != "" ? <SportsMmaIcon /> : <PlayArrowIcon />}
              disabled={saveName == null}
            >
              {linkCode != "" ? (
                <Trans i18nKey="play:fight" />
              ) : (
                <Trans i18nKey="play:play" />
              )}
              {startedState != null ? (
                <CoreSupervisor
                  incarnation={incarnation}
                  romName={saves[saveName!].romName}
                  patch={
                    patchVersion != null
                      ? { name: patchName!, version: patchVersion }
                      : undefined
                  }
                  matchSettings={
                    startedState.linkCode != null
                      ? {
                          sessionID: `${netplayCompatibility}-${MATCH_TYPES[matchType]}-${startedState.linkCode}`,
                          replaysPath: path.join(
                            getReplaysPath(app),
                            `${datefns.format(
                              now,
                              "yyyyMMddHHmmmmss"
                            )}-${netplayCompatibility}-${
                              MATCH_TYPES[matchType]
                            }-${startedState.linkCode}`
                          ),
                          replayInfo: {
                            ts: now.valueOf(),
                            rom: saves[saveName!].romName!,
                            patch:
                              patchInfo != null
                                ? {
                                    name: patchName!,
                                    version: patchVersion!,
                                  }
                                : null,
                          },
                          inputDelay,
                          matchType,
                        }
                      : undefined
                  }
                  savePath={path.join(getSavesPath(app), saveName!)}
                  windowTitle={`${
                    KNOWN_ROMS[saves[saveName!].romName].title[
                      i18n.resolvedLanguage
                    ]
                  }${patchVersion != null ? ` + ${patchInfo!.title}` : ""}`}
                  onExit={() => {
                    setStartedState(null);
                    setIncarnation((incarnation) => incarnation + 1);
                  }}
                />
              ) : null}
            </Button>
          </Stack>
          <Collapse in={extraOptionsOpen}>
            <Stack spacing={1}>
              <Box sx={{ px: 1 }}>
                <Typography sx={{ userSelect: "none" }} variant="body2">
                  <Trans i18nKey="play:input-delay" />
                </Typography>
                <Slider
                  value={inputDelay}
                  marks
                  min={0}
                  max={10}
                  valueLabelDisplay="auto"
                  size="small"
                  onChange={(e, value) => {
                    setInputDelay(value as number);
                  }}
                />
              </Box>
            </Stack>
          </Collapse>
        </Stack>
      </Stack>
    </Box>
  );
}

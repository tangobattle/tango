import { Trans, useTranslation } from "react-i18next";
import semver from "semver";
import React from "react";
import Stack from "@mui/material/Stack";
import Tabs from "@mui/material/Tabs";
import Tab from "@mui/material/Tab";
import Chip from "@mui/material/Chip";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableRow from "@mui/material/TableRow";
import TableCell from "@mui/material/TableCell";
import Select from "@mui/material/Select";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import RefreshIcon from "@mui/icons-material/Refresh";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import SportsMmaIcon from "@mui/icons-material/SportsMma";
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import ListSubheader from "@mui/material/ListSubheader";
import Collapse from "@mui/material/Collapse";
import SportsEsportsOutlinedIcon from "@mui/icons-material/SportsEsportsOutlined";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import MenuItem from "@mui/material/MenuItem";
import { useSaves } from "../SavesContext";
import { KNOWN_ROMS } from "../../../rom";
import {
  getPatchesPath,
  getReplaysPath,
  getROMsPath,
  getSavesPath,
} from "../../../paths";
import { readFile } from "fs/promises";
import path from "path";
import * as bn6 from "../../../saveedit/bn6";
import { CoreSupervisor } from "../CoreSupervisor";
import { useROMs } from "../ROMsContext";
import { shell } from "@electron/remote";
import { usePatches } from "../PatchesContext";
import TextField from "@mui/material/TextField";
import InputAdornment from "@mui/material/InputAdornment";
import ListItemText from "@mui/material/ListItemText";
import { CopyButton } from "../CopyButton";

const MATCH_TYPES = ["single", "triple"];

function ModcardsViewer({ editor }: { editor: bn6.Editor }) {
  const { i18n } = useTranslation();

  const modcards: { id: number; enabled: boolean }[] = [];
  for (let i = 0; i < editor.getModcardCount(); i++) {
    modcards.push(editor.getModcard(i));
  }

  const DEBUFF_COLOR = "#b55ade";
  const BUFF_COLOR = "#ffbd18";
  const OFF_COLOR = "#bdbdbd";

  return (
    <Table size="small">
      <TableBody>
        {modcards.map(({ id, enabled }, i) => {
          const modcard = bn6.MODCARDS[id]!;
          return (
            <TableRow key={i}>
              <TableCell>
                {modcard.name[i18n.resolvedLanguage as "en" | "ja"]}{" "}
                <small>{modcard.mb}MB</small>
              </TableCell>
              <TableCell style={{ verticalAlign: "top" }}>
                <Stack spacing={0.5}>
                  {modcard.parameters.flatMap((l, i) =>
                    l.version == null ||
                    l.version == editor.getGameInfo().version
                      ? [
                          <Chip
                            key={i}
                            label={l.name[i18n.resolvedLanguage as "en" | "ja"]}
                            size="small"
                            sx={{
                              justifyContent: "flex-start",
                              backgroundColor: enabled
                                ? l.debuff
                                  ? DEBUFF_COLOR
                                  : BUFF_COLOR
                                : OFF_COLOR,
                            }}
                          />,
                        ]
                      : []
                  )}
                </Stack>
              </TableCell>
              <TableCell style={{ verticalAlign: "top" }}>
                <Stack spacing={0.5}>
                  {modcard.abilities.flatMap((l, i) =>
                    l.version == null ||
                    l.version == editor.getGameInfo().version
                      ? [
                          <Chip
                            key={i}
                            label={l.name[i18n.resolvedLanguage as "en" | "ja"]}
                            size="small"
                            sx={{
                              justifyContent: "flex-start",
                              backgroundColor: enabled
                                ? l.debuff
                                  ? DEBUFF_COLOR
                                  : BUFF_COLOR
                                : OFF_COLOR,
                            }}
                          />,
                        ]
                      : []
                  )}
                </Stack>
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}

function FolderChipRow({
  chip,
}: {
  chip: {
    id: number;
    code: string;
    isRegular: boolean;
    isTag1: boolean;
    isTag2: boolean;
    count: number;
  };
}) {
  const { id, code, isRegular, isTag1, isTag2, count } = chip;

  const { i18n } = useTranslation();
  const [open, setOpen] = React.useState(false);
  React.useEffect(() => {
    setOpen(false);
  }, [chip]);

  const MEGA_BG = "#adefef";
  const GIGA_BG = "#f7cee7";
  const backgroundColor =
    bn6.CHIPS[id]!.class == "giga"
      ? GIGA_BG
      : bn6.CHIPS[id]!.class == "mega"
      ? MEGA_BG
      : null;

  return (
    <TableRow sx={{ backgroundColor }}>
      <TableCell sx={{ width: "32px", textAlign: "right" }}>
        <strong>{count}x</strong>
      </TableCell>
      <TableCell sx={{ width: 0 }}>
        <img
          height="32"
          width="32"
          src={(() => {
            try {
              return require(`../../../../static/images/games/bn6/chipicons/${id}.png`);
            } catch (e) {
              return "";
            }
          })()}
          style={{ imageRendering: "pixelated" }}
        />
      </TableCell>
      <TableCell component="th">
        {bn6.CHIPS[id]!.name[i18n.resolvedLanguage as "en" | "ja"]}{" "}
        {code.replace(/\*/g, "﹡")}{" "}
        {isRegular ? (
          <Chip
            label={<Trans i18nKey="play:folder.regular-chip" />}
            sx={{ backgroundColor: "#FF42A5", color: "white" }}
            size="small"
          />
        ) : null}{" "}
        {isTag1 ? (
          <Chip
            label={<Trans i18nKey="play:folder.tag-chip" />}
            sx={{ backgroundColor: "#29F721", color: "white" }}
            size="small"
          />
        ) : null}{" "}
        {isTag2 ? (
          <Chip
            label={<Trans i18nKey="play:folder.tag-chip" />}
            sx={{ backgroundColor: "#29F721", color: "white" }}
            size="small"
          />
        ) : null}
      </TableCell>
      <TableCell sx={{ width: 0 }}>
        <img
          height="28"
          width="28"
          src={require(`../../../../static/images/games/bn6/elements/${bn6
            .CHIPS[id]!.element!}.png`)}
          style={{ imageRendering: "pixelated" }}
        />
      </TableCell>
      <TableCell sx={{ width: "56px", textAlign: "right" }}>
        <strong>{bn6.CHIPS[id]!.damage!}</strong>
      </TableCell>
      <TableCell sx={{ width: "64px", textAlign: "right" }}>
        {bn6.CHIPS[id]!.mb!}MB
      </TableCell>
    </TableRow>
  );
}

function FolderViewer({ editor }: { editor: bn6.Editor }) {
  const chips: {
    id: number;
    code: string;
    isRegular: boolean;
    isTag1: boolean;
    isTag2: boolean;
    count: number;
  }[] = [];
  const chipCounter: { [key: string]: number } = {};
  for (let i = 0; i < 30; i++) {
    const chip = editor.getChip(editor.getEquippedFolder(), i);
    if (chip == null) {
      continue;
    }
    const chipKey = `${chip.id}:${chip.code}`;
    if (!Object.prototype.hasOwnProperty.call(chipCounter, chipKey)) {
      chipCounter[chipKey] = 0;
      chips.push({
        ...chip,
        isRegular: false,
        isTag1: false,
        isTag2: false,
        count: 0,
      });
    }
    chipCounter[chipKey]++;
  }

  for (const chip of chips) {
    chip.count = chipCounter[`${chip.id}:${chip.code}`];

    const regularChipIdx = editor.getRegularChipIndex(
      editor.getEquippedFolder()
    );
    if (regularChipIdx != null) {
      const regularChip = editor.getChip(
        editor.getEquippedFolder(),
        regularChipIdx
      )!;
      if (chip.id == regularChip.id && chip.code == regularChip.code) {
        chip.isRegular = true;
      }
    }

    const tagChip1Idx = editor.getTagChip1Index(editor.getEquippedFolder());
    if (tagChip1Idx != null) {
      const tagChip1 = editor.getChip(editor.getEquippedFolder(), tagChip1Idx)!;
      if (chip.id == tagChip1.id && chip.code == tagChip1.code) {
        chip.isTag1 = true;
      }
    }

    const tagChip2Idx = editor.getTagChip2Index(editor.getEquippedFolder());
    if (tagChip2Idx != null) {
      const tagChip2 = editor.getChip(editor.getEquippedFolder(), tagChip2Idx)!;
      if (chip.id == tagChip2.id && chip.code == tagChip2.code) {
        chip.isTag2 = true;
      }
    }
  }

  return (
    <Table size="small">
      <TableBody>
        {chips.map((chip, i) => (
          <FolderChipRow key={i} chip={chip} />
        ))}
      </TableBody>
    </Table>
  );
}

function SaveViewer({
  filename,
  incarnation,
}: {
  filename: string;
  incarnation: number;
}) {
  const [tab, setTab] = React.useState("navicust");
  const [editor, setEditor] = React.useState<bn6.Editor | null>(null);

  React.useEffect(() => {
    (async () => {
      setEditor(
        new bn6.Editor(
          bn6.Editor.sramDumpToRaw(
            (await readFile(path.join(getSavesPath(), filename))).buffer
          )
        )
      );
    })();
  }, [filename, incarnation]);

  if (editor == null) {
    return null;
  }

  return (
    <Stack flexGrow={1} flexShrink={0}>
      <Box flexGrow={0}>
        <Tabs
          sx={{ px: 1 }}
          value={tab}
          onChange={(e, value) => {
            setTab(value);
          }}
        >
          <Tab label={<Trans i18nKey="play:tab.navicust" />} value="navicust" />
          <Tab label={<Trans i18nKey="play:tab.folder" />} value="folder" />
          <Tab
            label={<Trans i18nKey="play:tab.modcards" />}
            value="modcards"
            disabled={!editor.supportsModcards()}
          />
        </Tabs>
      </Box>
      <Box
        flexGrow={1}
        display={tab == "navicust" ? undefined : "none"}
        sx={{ px: 1, height: 0, minWidth: 0 }}
      >
        Not supported yet :(
      </Box>
      <Box
        flexGrow={1}
        display={tab == "folder" ? undefined : "none"}
        sx={{ px: 1, height: 0, minWidth: 0 }}
        overflow="auto"
      >
        <FolderViewer editor={editor} />
      </Box>
      {editor.supportsModcards() ? (
        <Box
          flexGrow={1}
          display={tab == "modcards" ? undefined : "none"}
          sx={{ px: 1, height: 0, minWidth: 0 }}
        >
          <ModcardsViewer editor={editor} />
        </Box>
      ) : null}
    </Stack>
  );
}

export default function PlayPane({ active }: { active: boolean }) {
  const { saves, rescan: rescanSaves } = useSaves();
  const { patches, rescan: rescanPatches } = usePatches();
  const { roms, rescan: rescanROMs } = useROMs();
  const { i18n } = useTranslation();

  const [extraOptionsOpen, setExtraOptionsOpen] = React.useState(false);

  const [saveName_, setSaveName] = React.useState<string | null>(null);
  const [started, setStarted] = React.useState(false);
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

  const romNames = Object.keys(groupedSaves);
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

  const [matchType, setMatchType] = React.useState(0);
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

  const sessionID = `${netplayCompatibility}-${MATCH_TYPES[matchType]}-${linkCode}`;

  return (
    <Box
      sx={{
        my: 1,
        flexGrow: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Stack sx={{ flexGrow: 1 }} spacing={1}>
        <Box flexGrow={0} flexShrink={0} sx={{ px: 1 }}>
          <Stack spacing={1} direction="row">
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
                  const saveNames = groupedSaves[romName];
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
                    shell.openPath(getSavesPath());
                  } else {
                    shell.showItemInFolder(path.join(getSavesPath(), saveName));
                  }
                }}
              >
                <FolderOpenIcon />
              </IconButton>
            </Tooltip>
            <Tooltip title={<Trans i18nKey="play:reload-saves" />}>
              <IconButton
                onClick={() => {
                  (async () => {
                    await rescanROMs();
                    await rescanPatches();
                    await rescanSaves();
                  })();
                }}
              >
                <RefreshIcon />
              </IconButton>
            </Tooltip>
          </Stack>
        </Box>
        {saveName != null ? (
          <Box flexGrow={1} display="flex">
            <SaveViewer filename={saveName} incarnation={incarnation} />
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
        <Stack>
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
                  setLinkCode(e.target.value.replace(/\s/g, "").toLowerCase());
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
                        value={sessionID}
                      />
                    </InputAdornment>
                  ),
                }}
              />
            </Box>
            <Button
              variant="contained"
              startIcon={linkCode != "" ? <SportsMmaIcon /> : <PlayArrowIcon />}
              disabled={saveName == null}
              onClick={() => {
                setStarted(true);
              }}
            >
              {linkCode != "" ? (
                <Trans i18nKey="play:fight" />
              ) : (
                <Trans i18nKey="play:play" />
              )}
              {started ? (
                <CoreSupervisor
                  incarnation={incarnation}
                  romPath={path.join(
                    getROMsPath(),
                    roms[saves[saveName!].romName]
                  )}
                  patchPath={
                    patchVersion != null
                      ? path.join(
                          getPatchesPath(),
                          patchName!,
                          `v${patchVersion}.${
                            patchInfo!.versions[patchVersion].format
                          }`
                        )
                      : undefined
                  }
                  matchSettings={
                    linkCode != ""
                      ? {
                          sessionID,
                          replaysPath: path.join(getReplaysPath()),
                          replayInfo: {
                            rom: saves[saveName!].romName!,
                            patch: { name: patchName!, version: patchVersion! },
                          },
                        }
                      : undefined
                  }
                  savePath={path.join(getSavesPath(), saveName!)}
                  windowTitle={`${
                    KNOWN_ROMS[saves[saveName!].romName].title[
                      i18n.resolvedLanguage
                    ]
                  }${patchVersion != null ? ` + ${patchInfo!.title}` : ""}`}
                  onExit={(_exitStatus) => {
                    setStarted(false);
                    setIncarnation((incarnation) => incarnation + 1);
                  }}
                />
              ) : null}
            </Button>
          </Stack>
          <Collapse in={extraOptionsOpen}>
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
                    fullWidth
                  >
                    <MenuItem value="null">
                      <Trans i18nKey="play:unpatched" />
                    </MenuItem>
                    {eligiblePatchNames.map((patchName) => {
                      const v = JSON.stringify(patchName);
                      return (
                        <MenuItem key={v} value={v}>
                          {patches[patchName].title}
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
        </Stack>
      </Stack>
    </Box>
  );
}

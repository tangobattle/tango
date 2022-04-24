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
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import ListSubheader from "@mui/material/ListSubheader";
import Collapse from "@mui/material/Collapse";
import GridViewIcon from "@mui/icons-material/GridView";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import MenuItem from "@mui/material/MenuItem";
import { useSaves } from "../SavesContext";
import { KNOWN_ROMS } from "../../../rom";
import { getPatchesPath, getROMsPath, getSavesPath } from "../../../paths";
import { readFile } from "fs/promises";
import path from "path";
import * as bn6 from "../../../saveedit/bn6";
import { CoreSupervisor } from "../CoreSupervisor";
import { useROMs } from "../ROMsContext";
import { shell } from "@electron/remote";
import { usePatches } from "../PatchesContext";

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
    <TableRow>
      <TableCell sx={{ width: 0 }}>
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
            label={<Trans i18nKey="saves:folder.regular-chip" />}
            sx={{ backgroundColor: "#FF42A5", color: "white" }}
            size="small"
          />
        ) : null}{" "}
        {isTag1 ? (
          <Chip
            label={<Trans i18nKey="saves:folder.tag-chip" />}
            sx={{ backgroundColor: "#29F721", color: "white" }}
            size="small"
          />
        ) : null}{" "}
        {isTag2 ? (
          <Chip
            label={<Trans i18nKey="saves:folder.tag-chip" />}
            sx={{ backgroundColor: "#29F721", color: "white" }}
            size="small"
          />
        ) : null}
      </TableCell>
      <TableCell sx={{ width: 0, textAlign: "right" }}>
        {bn6.CHIPS[id]!.damage!}
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
      <TableCell sx={{ width: 0, textAlign: "right" }}>
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
          <Tab
            label={<Trans i18nKey="saves:tab.navicust" />}
            value="navicust"
          />
          <Tab label={<Trans i18nKey="saves:tab.folder" />} value="folder" />
          <Tab
            label={<Trans i18nKey="saves:tab.modcards" />}
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

export default function SavesPane({ active }: { active: boolean }) {
  const { saves, rescan: rescanSaves } = useSaves();
  const { patches } = usePatches();
  const { roms } = useROMs();
  const { i18n } = useTranslation();

  const [selection_, setSelection] = React.useState<string | null>(null);
  const [started, setStarted] = React.useState(false);
  const [incarnation, setIncarnation] = React.useState(0);

  const selection =
    selection_ != null &&
    Object.prototype.hasOwnProperty.call(saves, selection_)
      ? selection_
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

  const [selectedPatch, setSelectedPatch] = React.useState<string | null>(null);
  const save = selection != null ? saves[selection] : null;

  const eligiblePatchNames = React.useMemo(() => {
    const eligiblePatchNames =
      save != null
        ? Object.keys(patches).filter((p) => patches[p].forROM == save.romName)
        : [];
    eligiblePatchNames.sort();
    return eligiblePatchNames;
  }, [patches, save]);

  const patchInfo = selectedPatch != null ? patches[selectedPatch] : null;

  const patchVersions = React.useMemo(
    () =>
      patchInfo != null ? semver.rsort(Object.keys(patchInfo.versions)) : null,
    [patchInfo]
  );

  const [selectedPatchVersion, setSelectedPatchVersion] = React.useState<
    string | null
  >(null);
  React.useEffect(() => {
    if (patchVersions == null) {
      setSelectedPatchVersion("");
      return;
    }
    setSelectedPatchVersion(patchVersions[0]);
  }, [patchVersions]);

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
                <Trans i18nKey="saves:select-save" />
              </InputLabel>
              <Select
                labelId="select-save-label"
                label={<Trans i18nKey="saves:select-save" />}
                value={selection ?? ""}
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
                    selection == null ||
                    saves[e.target.value].romName != saves[selection].romName
                  ) {
                    setSelectedPatch(null);
                    setSelectedPatchVersion(null);
                  }
                  setSelection(e.target.value);
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
            <Tooltip title={<Trans i18nKey="saves:open-dir" />}>
              <IconButton
                onClick={() => {
                  if (selection == null) {
                    shell.openPath(getSavesPath());
                  } else {
                    shell.showItemInFolder(
                      path.join(getSavesPath(), selection)
                    );
                  }
                }}
              >
                <FolderOpenIcon />
              </IconButton>
            </Tooltip>
            <Tooltip title={<Trans i18nKey="saves:reload-saves" />}>
              <IconButton
                onClick={() => {
                  rescanSaves();
                }}
              >
                <RefreshIcon />
              </IconButton>
            </Tooltip>
          </Stack>
        </Box>
        {selection != null ? (
          <Box flexGrow={1} display="flex">
            <SaveViewer filename={selection} incarnation={incarnation} />
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
              <GridViewIcon sx={{ fontSize: "4rem" }} />
              <Typography variant="h6">
                <Trans i18nKey="saves:no-save-selected" />
              </Typography>
            </Stack>
          </Box>
        )}
        <Stack
          flexGrow={0}
          flexShrink={0}
          justifyContent="flex-end"
          direction="row"
          spacing={1}
          sx={{ px: 1 }}
        >
          <Box flexGrow={5} flexShrink={0}>
            <FormControl fullWidth size="small">
              <InputLabel id="game-label">
                <Trans i18nKey="saves:patch-name" />
              </InputLabel>
              <Select
                labelId="game-label"
                disabled={selection == null}
                size="small"
                value={JSON.stringify(selectedPatch)}
                label={<Trans i18nKey={"saves:patch-name"} />}
                onChange={(e) => {
                  setSelectedPatch(JSON.parse(e.target.value));
                  setSelectedPatchVersion(null);
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
                <Trans i18nKey="saves:patch-version" />
              </InputLabel>
              <Select
                labelId="patch-version-label"
                disabled={selection == null || selectedPatch == null}
                size="small"
                value={selectedPatchVersion || ""}
                label={<Trans i18nKey={"saves:patch-version"} />}
                onChange={(e) => {
                  setSelectedPatchVersion(e.target.value);
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
          <Button
            variant="contained"
            disabled={selection == null}
            startIcon={<PlayArrowIcon />}
            onClick={() => {
              setStarted(true);
            }}
          >
            <Trans i18nKey="saves:play" />
          </Button>
          {started ? (
            <CoreSupervisor
              incarnation={incarnation}
              romPath={path.join(
                getROMsPath(),
                roms[saves[selection!].romName]
              )}
              patchPath={
                selectedPatchVersion != null
                  ? path.join(
                      getPatchesPath(),
                      selectedPatch!,
                      `v${selectedPatchVersion}.${
                        patchInfo!.versions[selectedPatchVersion].format
                      }`
                    )
                  : undefined
              }
              savePath={path.join(getSavesPath(), selection!)}
              windowTitle={`${
                KNOWN_ROMS[saves[selection!].romName].title[
                  i18n.resolvedLanguage
                ]
              }${selectedPatchVersion != null ? ` + ${patchInfo!.title}` : ""}`}
              onExit={(_exitStatus) => {
                setStarted(false);
                setIncarnation((incarnation) => incarnation + 1);
              }}
            />
          ) : null}
        </Stack>
      </Stack>
    </Box>
  );
}

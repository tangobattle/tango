import { Trans, useTranslation } from "react-i18next";
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
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import ListSubheader from "@mui/material/ListSubheader";
import GridViewIcon from "@mui/icons-material/GridView";
import MenuItem from "@mui/material/MenuItem";
import { useSaves } from "../SavesContext";
import { KNOWN_ROMS } from "../../../rom";
import { getROMsPath, getSavesPath } from "../../../paths";
import { readFile } from "fs/promises";
import path from "path";
import * as bn6 from "../../../saveedit/bn6";
import i18n from "../../i18n";
import { CoreSupervisor } from "../CoreSupervisor";
import { useROMs } from "../ROMsContext";

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

    const tagChip2Idx = editor.getTagChip1Index(editor.getEquippedFolder());
    if (tagChip2Idx != null) {
      const tagChip2 = editor.getChip(editor.getEquippedFolder(), tagChip2Idx)!;
      if (chip.id == tagChip2.id && chip.code == tagChip2.code) {
        chip.isTag2 = true;
      }
    }
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
        <Table size="small">
          <TableBody>
            {chips.map(({ id, code, count, isRegular, isTag1, isTag2 }, i) => {
              return (
                <TableRow key={i}>
                  <TableCell sx={{ width: 0 }}>{count}x</TableCell>
                  <TableCell>
                    {bn6.CHIPS[id]!.name[i18n.resolvedLanguage as "en" | "ja"]}{" "}
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
                  <TableCell sx={{ fontFamily: "monospace", width: 0 }}>
                    {code}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </Box>
      <Box
        flexGrow={1}
        display={tab == "modcards" ? undefined : "none"}
        sx={{ px: 1, height: 0, minWidth: 0 }}
      >
        Not supported yet :(
      </Box>
    </Stack>
  );
}

export default function SavesPane({ active }: { active: boolean }) {
  const { saves, rescan: rescanSaves } = useSaves();
  const { roms } = useROMs();
  const { i18n } = useTranslation();

  const [selection, setSelection] = React.useState<string | null>(null);
  const [started, setStarted] = React.useState(false);
  const [incarnation, setIncarnation] = React.useState(0);

  React.useEffect(() => {
    if (
      selection != null &&
      !Object.prototype.hasOwnProperty.call(saves, selection)
    ) {
      setSelection(null);
    }
  }, [saves, selection]);

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
                onChange={(e) => {
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
          spacing={2}
          sx={{ px: 1 }}
        >
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
              romPath={path.join(
                getROMsPath(),
                roms[saves[selection!].romName]
              )}
              savePath={path.join(getSavesPath(), selection!)}
              windowTitle={
                KNOWN_ROMS[saves[selection!].romName].title[
                  i18n.resolvedLanguage
                ]
              }
              onExit={(_exitStatus) => {
                setIncarnation((incarnation) => incarnation + 1);
                setStarted(false);
              }}
            />
          ) : null}
        </Stack>
      </Stack>
    </Box>
  );
}

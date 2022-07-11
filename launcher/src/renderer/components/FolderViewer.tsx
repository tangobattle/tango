import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { clipboard } from "@electron/remote";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tooltip from "@mui/material/Tooltip";
import useTheme from "@mui/system/useTheme";

import { Chip as ChipInfo, FolderEditor } from "../../saveedit";
import { fallbackLng } from "../i18n";

const MEGA_BG = {
  dark: "#52849c",
  light: "#adefef",
};

const GIGA_BG = {
  dark: "#8c3152",
  light: "#f7cee7",
};

function romNameToAssetFolder(romName: string) {
  switch (romName) {
    case "MEGAMAN6_FXXBR6E":
    case "MEGAMAN6_GXXBR5E":
    case "ROCKEXE6_RXXBR6J":
    case "ROCKEXE6_GXXBR5J":
      return "bn6";
    case "MEGAMANBN4BMB4BE":
    case "MEGAMANBN4RSB4WE":
    case "ROCK_EXE4_BMB4BJ":
    case "ROCK_EXE4_RSB4WJ":
    case "ROCKEXE4.5ROBR4J":
      return "bn4";
  }
  throw `unknown rom name: ${romName}`;
}

function FolderChipRow({
  groupedChip,
  romName,
  chipData,
}: {
  groupedChip: {
    id: number;
    code: string;
    isRegular: boolean;
    isTag1: boolean;
    isTag2: boolean;
    count: number;
  };
  romName: string;
  chipData: (ChipInfo | null)[];
}) {
  const { id, code, isRegular, isTag1, isTag2, count } = groupedChip;
  const theme = useTheme();

  const { i18n } = useTranslation();

  const backgroundColor =
    chipData[id]!.class == "giga"
      ? GIGA_BG[theme.palette.mode]
      : chipData[id]!.class == "mega"
      ? MEGA_BG[theme.palette.mode]
      : null;

  const chipInfo = chipData[id];
  if (chipInfo == null || chipInfo.description == null) {
    return null;
  }

  return (
    <TableRow sx={{ backgroundColor }}>
      <TableCell sx={{ width: "28px", textAlign: "right" }}>
        <strong>{count}x</strong>
      </TableCell>
      <TableCell sx={{ width: 0 }}>
        <img
          height="28"
          width="28"
          src={(() => {
            try {
              return require(`../../../static/images/games/${romNameToAssetFolder(
                romName
              )}/chipicons/${id}.png`);
            } catch (e) {
              return "";
            }
          })()}
          style={{ imageRendering: "pixelated" }}
        />
      </TableCell>
      <TableCell component="th">
        <Tooltip
          title={
            chipInfo.description[
              i18n.resolvedLanguage as keyof typeof chipInfo.description
            ] ||
            chipInfo.description[
              fallbackLng as keyof typeof chipInfo.description
            ]
          }
          placement="right"
        >
          <span>
            {chipInfo.name[
              i18n.resolvedLanguage as keyof typeof chipInfo.name
            ] || chipInfo.name[fallbackLng as keyof typeof chipInfo.name]}{" "}
            {code}
          </span>
        </Tooltip>{" "}
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
          src={require(`../../../static/images/games/${romNameToAssetFolder(
            romName
          )}/elements/${chipInfo.element ?? "null"}.png`)}
          style={{ imageRendering: "pixelated" }}
        />
      </TableCell>
      <TableCell sx={{ width: "56px", textAlign: "right" }}>
        <strong>{(chipInfo.damage ?? 0) > 0 ? chipInfo.damage : null}</strong>
      </TableCell>
      <TableCell sx={{ width: "64px", textAlign: "right" }}>
        {chipInfo.mb ?? 0}MB
      </TableCell>
    </TableRow>
  );
}

export default function FolderViewer({
  romName,
  editor,
  active,
}: {
  romName: string;
  editor: FolderEditor;
  active: boolean;
}) {
  const chips: ({ id: number; code: string } | null)[] = [];
  for (let i = 0; i < 30; i++) {
    const chip = editor.getChip(editor.getEquippedFolder(), i);
    chips.push(chip);
  }

  if (!editor.isRegularChipInPlace()) {
    const regChipIdx = editor.getRegularChipIndex(editor.getEquippedFolder());
    if (regChipIdx != null) {
      chips.splice(regChipIdx, 0, ...chips.splice(0, 1));
    }
  }

  const { i18n } = useTranslation();

  const groupedChips: {
    id: number;
    code: string;
    isRegular: boolean;
    isTag1: boolean;
    isTag2: boolean;
    count: number;
  }[] = [];
  const chipCounter: { [key: string]: number } = {};
  for (const chip of chips) {
    if (chip == null) {
      continue;
    }
    const chipKey = `${chip.id}:${chip.code}`;
    if (!Object.prototype.hasOwnProperty.call(chipCounter, chipKey)) {
      chipCounter[chipKey] = 0;
      groupedChips.push({
        ...chip,
        isRegular: false,
        isTag1: false,
        isTag2: false,
        count: 0,
      });
    }
    chipCounter[chipKey]++;
  }

  for (const groupedChip of groupedChips) {
    groupedChip.count = chipCounter[`${groupedChip.id}:${groupedChip.code}`];

    const regularChipIdx = editor.getRegularChipIndex(
      editor.getEquippedFolder()
    );
    if (regularChipIdx != null) {
      const regularChip = chips[regularChipIdx]!;
      if (
        groupedChip.id == regularChip.id &&
        groupedChip.code == regularChip.code
      ) {
        groupedChip.isRegular = true;
      }
    }

    const tagChip1Idx = editor.getTagChip1Index(editor.getEquippedFolder());
    if (tagChip1Idx != null) {
      const tagChip1 = chips[tagChip1Idx]!;
      if (groupedChip.id == tagChip1.id && groupedChip.code == tagChip1.code) {
        groupedChip.isTag1 = true;
      }
    }

    const tagChip2Idx = editor.getTagChip2Index(editor.getEquippedFolder());
    if (tagChip2Idx != null) {
      const tagChip2 = chips[tagChip2Idx]!;
      if (groupedChip.id == tagChip2.id && groupedChip.code == tagChip2.code) {
        groupedChip.isTag2 = true;
      }
    }
  }

  const chipData = editor.getChipData();

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          <Table size="small">
            <TableBody>
              {groupedChips.map((groupedChip, i) => (
                <FolderChipRow
                  key={i}
                  groupedChip={groupedChip}
                  romName={romName}
                  chipData={chipData}
                />
              ))}
            </TableBody>
          </Table>
        </Box>
        <Box>
          <Stack
            flexGrow={0}
            flexShrink={0}
            direction="row"
            justifyContent="flex-end"
            spacing={1}
            sx={{ px: 1, mb: 0 }}
          >
            <Button
              startIcon={<ContentCopyIcon />}
              onClick={() => {
                clipboard.writeText(
                  groupedChips
                    .map(({ id, code, count, isRegular, isTag1, isTag2 }) => {
                      const chip = chipData[id];
                      const chipName =
                        chip!.name[i18n.resolvedLanguage] ||
                        chip!.name[fallbackLng];
                      return `${count}\t${chipName}\t${code}\t${[
                        ...(isRegular ? ["[REG]"] : []),
                        ...(isTag1 ? ["[TAG]"] : []),
                        ...(isTag2 ? ["[TAG]"] : []),
                      ].join(" ")}`;
                    })
                    .join("\n")
                );
              }}
            >
              <Trans i18nKey="common:copy-to-clipboard" />
            </Button>
          </Stack>
        </Box>
      </Stack>
    </Box>
  );
}

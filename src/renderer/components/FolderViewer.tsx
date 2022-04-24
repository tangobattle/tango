import React from "react";
import { Trans, useTranslation } from "react-i18next";

import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tooltip from "@mui/material/Tooltip";

import * as bn6 from "../../saveedit/bn6";

const MEGA_BG = "#adefef";
const GIGA_BG = "#f7cee7";

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
              return require(`../../../static/images/games/bn6/chipicons/${id}.png`);
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
            bn6.CHIPS[id]!.description![i18n.resolvedLanguage as "en" | "ja"]
          }
          placement="right"
        >
          <span>
            {bn6.CHIPS[id]!.name[i18n.resolvedLanguage as "en" | "ja"]}{" "}
            {code.replace(/\*/g, "﹡")}
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
          src={require(`../../../static/images/games/bn6/elements/${bn6.CHIPS[
            id
          ]!.element!}.png`)}
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

export default function FolderViewer({
  editor,
  active,
}: {
  editor: bn6.Editor;
  active: boolean;
}) {
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
    <Box
      flexGrow={1}
      display={active ? "block" : "none"}
      overflow="auto"
      sx={{ px: 1, height: 0, minWidth: 0 }}
    >
      <Table size="small">
        <TableBody>
          {chips.map((chip, i) => (
            <FolderChipRow key={i} chip={chip} />
          ))}
        </TableBody>
      </Table>
    </Box>
  );
}

import React from "react";
import { Trans } from "react-i18next";

import Box from "@mui/material/Box";
import Checkbox from "@mui/material/Checkbox";
import Chip from "@mui/material/Chip";
import FormControlLabel from "@mui/material/FormControlLabel";
import FormGroup from "@mui/material/FormGroup";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import useTheme from "@mui/system/useTheme";

import { Chip as ChipInfo, FolderEditor } from "../../../saveedit";
import { CopyButtonWithLabel } from "../CopyButton";

export enum AllowEdits {
  None,
  RegTagOnly,
  All,
}

const MEGA_BG = {
  dark: "#52849c",
  light: "#adefef",
};

const GIGA_BG = {
  dark: "#8c3152",
  light: "#f7cee7",
};

const DARK_BG = {
  dark: "#31395A",
  light: "#B58CD6",
};

function FolderChipRow({
  showGrouped,
  code,
  isRegular,
  isTag1,
  isTag2,
  count,
  chipInfo,
  elementIcons,
}: {
  showGrouped: boolean;
  code: string;
  isRegular: boolean;
  isTag1: boolean;
  isTag2: boolean;
  count: number;
  chipInfo: ChipInfo;
  elementIcons: ImageData[];
}) {
  const theme = useTheme();

  const backgroundColor =
    chipInfo != null && chipInfo.class == "giga"
      ? GIGA_BG[theme.palette.mode]
      : chipInfo != null && chipInfo.class == "mega"
      ? MEGA_BG[theme.palette.mode]
      : chipInfo != null && chipInfo.class == "dark"
      ? DARK_BG[theme.palette.mode]
      : null;

  const iconCanvasRef = React.useRef<HTMLCanvasElement | null>(null);
  React.useEffect(() => {
    const ctx = iconCanvasRef.current!.getContext("2d")!;
    ctx.clearRect(
      0,
      0,
      iconCanvasRef.current!.width,
      iconCanvasRef.current!.height
    );
    ctx.putImageData(chipInfo.icon, -1, -1);
  }, [chipInfo]);

  const elementIconCanvasRef = React.useRef<HTMLCanvasElement | null>(null);
  React.useEffect(() => {
    const ctx = elementIconCanvasRef.current!.getContext("2d")!;
    ctx.clearRect(
      0,
      0,
      elementIconCanvasRef.current!.width,
      elementIconCanvasRef.current!.height
    );
    if (chipInfo.element >= elementIcons.length) {
      return;
    }
    ctx.putImageData(elementIcons[chipInfo.element], -1, -1);
  }, [chipInfo, elementIcons]);

  return (
    <TableRow sx={{ backgroundColor }}>
      {showGrouped ? (
        <TableCell sx={{ width: "28px", textAlign: "right" }}>
          <strong>{count}x</strong>
        </TableCell>
      ) : null}
      <TableCell sx={{ width: 0 }}>
        <canvas
          width={14}
          height={14}
          style={{
            width: "28px",
            height: "28px",
            imageRendering: "pixelated",
          }}
          ref={iconCanvasRef}
        />
      </TableCell>
      <TableCell component="th">
        <span>
          {chipInfo.name} {code}
        </span>{" "}
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
        <canvas
          width={14}
          height={14}
          style={{
            width: "28px",
            height: "28px",
            imageRendering: "pixelated",
          }}
          ref={elementIconCanvasRef}
        />
      </TableCell>
      <TableCell sx={{ width: "56px", textAlign: "right" }}>
        <strong>{chipInfo.damage > 0 ? chipInfo.damage : ""}</strong>
      </TableCell>
      <TableCell
        sx={{
          width: "64px",
          textAlign: "right",
          display: chipInfo.mb > 0 ? undefined : "none",
        }}
      >
        {chipInfo.mb > 0 ? `${chipInfo.mb}MB` : null}
      </TableCell>
    </TableRow>
  );
}

export default function FolderViewer({
  editor,
  allowEdits,
  active,
}: {
  editor: FolderEditor;
  allowEdits: AllowEdits;
  active: boolean;
}) {
  const [showGrouped, setShowGrouped] = React.useState(true);
  // TODO: Use this one day.
  void allowEdits;

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

  let groupedChips: {
    firstIndex: number;
    id: number;
    code: string;
    isRegular: boolean;
    isTag1: boolean;
    isTag2: boolean;
    count: number;
  }[];

  if (showGrouped) {
    groupedChips = [];
    const chipCounter: { [key: string]: number } = {};
    for (let i = 0; i < chips.length; ++i) {
      const chip = chips[i];
      if (chip == null) {
        continue;
      }
      const chipKey = `${chip.id}:${chip.code}`;
      if (!Object.prototype.hasOwnProperty.call(chipCounter, chipKey)) {
        chipCounter[chipKey] = 0;
        groupedChips.push({
          ...chip,
          firstIndex: i,
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
        if (
          groupedChip.id == tagChip1.id &&
          groupedChip.code == tagChip1.code
        ) {
          groupedChip.isTag1 = true;
        }
      }

      const tagChip2Idx = editor.getTagChip2Index(editor.getEquippedFolder());
      if (tagChip2Idx != null) {
        const tagChip2 = chips[tagChip2Idx]!;
        if (
          groupedChip.id == tagChip2.id &&
          groupedChip.code == tagChip2.code
        ) {
          groupedChip.isTag2 = true;
        }
      }
    }
  } else {
    groupedChips = chips.flatMap((chip, i) =>
      chip != null
        ? [
            {
              ...chip,
              firstIndex: i,
              isRegular:
                editor.getRegularChipIndex(editor.getEquippedFolder()) == i,
              isTag1: editor.getTagChip1Index(editor.getEquippedFolder()) == i,
              isTag2: editor.getTagChip2Index(editor.getEquippedFolder()) == i,
              count: 1,
            },
          ]
        : []
    );
  }

  const elementIcons = React.useMemo(() => editor.getElementIcons(), [editor]);

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          <Table size="small">
            <TableBody>
              {groupedChips.map((groupedChip) => (
                <FolderChipRow
                  key={groupedChip.firstIndex}
                  showGrouped={showGrouped}
                  elementIcons={elementIcons}
                  code={groupedChip.code}
                  isRegular={groupedChip.isRegular}
                  isTag1={groupedChip.isTag1}
                  isTag2={groupedChip.isTag2}
                  count={groupedChip.count}
                  chipInfo={editor.getChipInfo(groupedChip.id)}
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
            sx={{ px: 1, mb: 0, pt: 1 }}
          >
            <FormGroup>
              <FormControlLabel
                control={
                  <Checkbox
                    checked={showGrouped}
                    onChange={(_e, v) => {
                      setShowGrouped(v);
                    }}
                  />
                }
                label={
                  <span style={{ userSelect: "none" }}>
                    <Trans i18nKey={"play:folder.show-grouped"} />
                  </span>
                }
              />
            </FormGroup>
            <CopyButtonWithLabel
              value={groupedChips
                .flatMap(({ id, code, count, isRegular, isTag1, isTag2 }) => {
                  const chipInfo = editor.getChipInfo(id);
                  return [
                    `${showGrouped ? `${count}\t` : ""}${
                      chipInfo.name
                    }\t${code}\t${[
                      ...(isRegular ? ["[REG]"] : []),
                      ...(isTag1 ? ["[TAG]"] : []),
                      ...(isTag2 ? ["[TAG]"] : []),
                    ].join(" ")}`,
                  ];
                })
                .join("\n")}
              TooltipProps={{ placement: "top" }}
            />
          </Stack>
        </Box>
      </Stack>
    </Box>
  );
}

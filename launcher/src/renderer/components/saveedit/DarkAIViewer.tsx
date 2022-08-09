import { sortBy } from "lodash-es";
import React from "react";

import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import useTheme from "@mui/system/useTheme";

import { Chip as ChipInfo, DarkAIEditor, FolderEditor } from "../../../saveedit";
import { CopyButtonWithLabel } from "../CopyButton";
import { DARK_BG, GIGA_BG, MEGA_BG } from "./FolderViewer";

function DarkAIRow({
  count,
  chipInfo,
  elementIcons,
}: {
  count: number;
  chipInfo: ChipInfo;
  elementIcons: ImageData[];
}) {
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

  const theme = useTheme();

  const backgroundColor =
    chipInfo != null && chipInfo.class == "giga"
      ? GIGA_BG[theme.palette.mode]
      : chipInfo != null && chipInfo.class == "mega"
      ? MEGA_BG[theme.palette.mode]
      : chipInfo != null && chipInfo.class == "dark"
      ? DARK_BG[theme.palette.mode]
      : null;

  return (
    <TableRow sx={{ backgroundColor }}>
      <TableCell sx={{ width: "28px", textAlign: "right" }}>
        <strong>{count}x</strong>
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
          ref={iconCanvasRef}
        />
      </TableCell>
      <TableCell component="th">{chipInfo.name}</TableCell>
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
    </TableRow>
  );
}

export default function DarkAIViewer({
  editor,
  folderEditor,
  active,
}: {
  editor: DarkAIEditor;
  folderEditor: FolderEditor;
  active: boolean;
}) {
  const rows = React.useMemo(() => {
    let standardChipUses = [];
    let megaChipUses = [];
    let gigaChipUses = [];
    let paUses = [];

    for (let id = 0; id < editor.getNumChips(); ++id) {
      const chipInfo = folderEditor.getChipInfo(id);
      const uses = editor.getChipUseCount(id);
      if (uses == 0) {
        continue;
      }

      switch (chipInfo.class) {
        case "standard":
          standardChipUses.push({ id, uses });
          break;
        case "mega":
          megaChipUses.push({ id, uses });
          break;
        case "giga":
          gigaChipUses.push({ id, uses });
          break;
        case "pa":
          paUses.push({ id, uses });
          break;
      }
    }

    standardChipUses = sortBy(standardChipUses, [(x) => -x.uses, (x) => x.id]);
    megaChipUses = sortBy(megaChipUses, [(x) => -x.uses, (x) => x.id]);
    gigaChipUses = sortBy(gigaChipUses, [(x) => -x.uses, (x) => x.id]);
    paUses = sortBy(paUses, [(x) => -x.uses, (x) => x.id]);

    const rows: { id: number; count: number }[] = [];
    for (let i = 0; i < 16; ++i) {
      if (i >= standardChipUses.length) {
        break;
      }
      rows.push({
        id: standardChipUses[i].id,
        count: i < 2 ? 4 : i < 4 ? 2 : 1,
      });
    }
    for (let i = 0; i < 5; ++i) {
      if (i >= megaChipUses.length) {
        break;
      }
      rows.push({
        id: megaChipUses[i].id,
        count: 1,
      });
    }
    if (gigaChipUses.length > 0) {
      rows.push({
        id: gigaChipUses[0].id,
        count: 1,
      });
    }
    if (paUses.length > 0) {
      rows.push({
        id: paUses[0].id,
        count: 1,
      });
    }

    return rows;
  }, [editor, folderEditor]);

  const elementIcons = React.useMemo(
    () => folderEditor.getElementIcons(),
    [folderEditor]
  );

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          <Table size="small">
            <TableBody>
              {rows.map(({ id, count }, i) => {
                const chipInfo = folderEditor.getChipInfo(id);
                return (
                  <DarkAIRow
                    key={i}
                    count={count}
                    chipInfo={chipInfo}
                    elementIcons={elementIcons}
                  />
                );
              })}
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
            <CopyButtonWithLabel
              value={rows
                .flatMap(({ id, count }) => {
                  const chipInfo = folderEditor.getChipInfo(id);
                  return [`${count}\t${chipInfo.name}`];
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

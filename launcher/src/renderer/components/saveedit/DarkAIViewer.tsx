import React from "react";
import { Trans, useTranslation } from "react-i18next";

import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";

import { Chip as ChipInfo, DarkAIEditor, FolderEditor } from "../../../saveedit";
import { CopyButtonWithLabel } from "../CopyButton";

function DarkAIRow({
  chipInfo,
  elementIcons,
}: {
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

  return (
    <TableRow>
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
  const { t } = useTranslation();
  const slots = [];
  for (let i = 0; i < editor.getNumSlots(); ++i) {
    slots.push(editor.getSlot(i));
  }

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
              {slots.map((slot, i) => {
                if (slot == null) {
                  return (
                    <TableRow key={i}>
                      <TableCell sx={{ width: 0 }}></TableCell>
                      <TableCell colSpan={3} sx={{ color: "text.disabled" }}>
                        <Trans i18nKey="play:darkai.unset" />
                      </TableCell>
                    </TableRow>
                  );
                }

                if (slot.type == "combo") {
                  return (
                    <TableRow key={i}>
                      <TableCell sx={{ width: 0 }}></TableCell>
                      <TableCell colSpan={3}>
                        <Trans
                          i18nKey="play:darkai.combo"
                          values={{ i: slot.id + 1 }}
                        />
                      </TableCell>
                    </TableRow>
                  );
                }

                const chipInfo = folderEditor.getChipInfo(slot.id);
                return (
                  <DarkAIRow
                    key={i}
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
              value={slots
                .flatMap((slot) => {
                  if (slot == null) {
                    return [t("play:darkai.unset")];
                  }

                  if (slot.type == "combo") {
                    return [t("play:darkai.combo", { i: slot.id + 1 })];
                  }

                  const chipInfo = folderEditor.getChipInfo(slot.id);
                  return [chipInfo.name];
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

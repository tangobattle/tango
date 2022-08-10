import { sortBy } from "lodash-es";
import React from "react";
import { Trans } from "react-i18next";

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
  chipInfo: ChipInfo | null;
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
    if (chipInfo == null) {
      return;
    }
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
    if (chipInfo == null) {
      return;
    }
    if (chipInfo.element >= elementIcons.length) {
      return;
    }
    ctx.putImageData(elementIcons[chipInfo.element], -1, -1);
  }, [chipInfo, elementIcons]);

  const theme = useTheme();

  const backgroundColor =
    chipInfo != null && chipInfo.dark
      ? DARK_BG[theme.palette.mode]
      : chipInfo != null && chipInfo.class == "mega"
      ? MEGA_BG[theme.palette.mode]
      : chipInfo != null && chipInfo.class == "giga"
      ? GIGA_BG[theme.palette.mode]
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
      <TableCell
        component="th"
        sx={{ color: chipInfo == null ? "text.disabled" : undefined }}
      >
        {chipInfo != null ? (
          chipInfo.name
        ) : (
          <Trans i18nKey="play:darkai.unset" />
        )}
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
        <strong>
          {chipInfo != null && chipInfo.damage > 0 ? chipInfo.damage : ""}
        </strong>
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
  const {
    secondaryStandardChips,
    standardChips,
    megaChips,
    gigaChip,
    combos,
    pa,
  } = React.useMemo(() => {
    let secondaryStandardChipUses = [];
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
        case "standard": {
          standardChipUses.push({ id, uses });
          const secondaryUses = editor.getSecondaryChipUseCount(id);
          if (secondaryUses > 0) {
            secondaryStandardChipUses.push({ id, uses: secondaryUses });
          }
          break;
        }
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

    secondaryStandardChipUses = sortBy(secondaryStandardChipUses, [
      (x) => -x.uses,
      (x) => x.id,
    ]);
    standardChipUses = sortBy(standardChipUses, [(x) => -x.uses, (x) => x.id]);
    megaChipUses = sortBy(megaChipUses, [(x) => -x.uses, (x) => x.id]);
    gigaChipUses = sortBy(gigaChipUses, [(x) => -x.uses, (x) => x.id]);
    paUses = sortBy(paUses, [(x) => -x.uses, (x) => x.id]);

    const secondaryStandardChips: { id: number | null; count: number }[] = [];
    for (let i = 0; i < 3; ++i) {
      secondaryStandardChips.push({
        id:
          i < secondaryStandardChipUses.length
            ? secondaryStandardChipUses[i].id
            : null,
        count: 1,
      });
    }

    const standardChips: { id: number | null; count: number }[] = [];
    for (let i = 0; i < 16; ++i) {
      standardChips.push({
        id: i < standardChipUses.length ? standardChipUses[i].id : null,
        count: i < 2 ? 4 : i < 4 ? 2 : 1,
      });
    }

    const megaChips: { id: number | null; count: number }[] = [];
    for (let i = 0; i < 5; ++i) {
      megaChips.push({
        id: i < megaChipUses.length ? megaChipUses[i].id : null,
        count: 1,
      });
    }

    const gigaChip = {
      id: gigaChipUses.length > 0 ? gigaChipUses[0].id : null,
      count: 1,
    };

    const combos: { id: number | null; count: number }[] = [];
    for (let i = 0; i < 8; ++i) {
      combos.push({
        id: null,
        count: 1,
      });
    }

    const pa = {
      id: paUses.length > 0 ? paUses[0].id : null,
      count: 1,
    };

    return {
      secondaryStandardChips,
      standardChips,
      megaChips,
      gigaChip,
      combos,
      pa,
    };
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
              <TableRow>
                <TableCell colSpan={5} component="th">
                  <strong>
                    <Trans i18nKey="play:darkai.section.secondary-standard" />
                  </strong>
                </TableCell>
              </TableRow>
              {secondaryStandardChips.map(({ id, count }, i) => {
                return (
                  <DarkAIRow
                    key={i}
                    count={count}
                    chipInfo={id != null ? folderEditor.getChipInfo(id) : null}
                    elementIcons={elementIcons}
                  />
                );
              })}
              <TableRow>
                <TableCell colSpan={5} component="th">
                  <strong>
                    <Trans i18nKey="play:darkai.section.standard" />
                  </strong>
                </TableCell>
              </TableRow>
              {standardChips.map(({ id, count }, i) => {
                return (
                  <DarkAIRow
                    key={i}
                    count={count}
                    chipInfo={id != null ? folderEditor.getChipInfo(id) : null}
                    elementIcons={elementIcons}
                  />
                );
              })}
              <TableRow>
                <TableCell colSpan={5} component="th">
                  <strong>
                    <Trans i18nKey="play:darkai.section.mega" />
                  </strong>
                </TableCell>
              </TableRow>
              {megaChips.map(({ id, count }, i) => {
                return (
                  <DarkAIRow
                    key={i}
                    count={count}
                    chipInfo={id != null ? folderEditor.getChipInfo(id) : null}
                    elementIcons={elementIcons}
                  />
                );
              })}
              <TableRow>
                <TableCell colSpan={5} component="th">
                  <strong>
                    <Trans i18nKey="play:darkai.section.giga" />
                  </strong>
                </TableCell>
              </TableRow>
              <DarkAIRow
                count={gigaChip.count}
                chipInfo={
                  gigaChip.id != null
                    ? folderEditor.getChipInfo(gigaChip.id)
                    : null
                }
                elementIcons={elementIcons}
              />
              <TableRow>
                <TableCell colSpan={5} component="th">
                  <strong>
                    <Trans i18nKey="play:darkai.section.combos" />
                  </strong>
                </TableCell>
              </TableRow>
              {combos.map(({ id, count }, i) => {
                return (
                  <DarkAIRow
                    key={i}
                    count={count}
                    chipInfo={id != null ? folderEditor.getChipInfo(id) : null}
                    elementIcons={elementIcons}
                  />
                );
              })}
              <TableRow>
                <TableCell colSpan={5} component="th">
                  <strong>
                    <Trans i18nKey="play:darkai.section.pa" />
                  </strong>
                </TableCell>
              </TableRow>
              <DarkAIRow
                count={pa.count}
                chipInfo={
                  pa.id != null ? folderEditor.getChipInfo(pa.id) : null
                }
                elementIcons={elementIcons}
              />
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
              value={[
                ...secondaryStandardChips,
                ...standardChips,
                ...megaChips,
                gigaChip,
                ...combos,
                pa,
              ]
                .flatMap(({ id, count }) => {
                  return [
                    `${count}\t${
                      id != null ? folderEditor.getChipInfo(id).name : "-"
                    }`,
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

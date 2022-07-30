import React from "react";

import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";

import { BN4ModcardsEditor } from "../../saveedit";
import { CopyButtonWithLabel } from "./CopyButton";

const SLOT_NAMES = ["0A", "0B", "0C", "0D", "0E", "0F"];

export default function BN4ModcardsViewer({
  editor,
  active,
}: {
  editor: BN4ModcardsEditor;
  active: boolean;
}) {
  const modcards: { id: number; enabled: boolean }[] = [];
  for (let i = 0; i < 6; i++) {
    modcards.push(editor.getModcard(i)!);
  }

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          <Table size="small">
            <TableBody>
              {modcards.map(({ id, enabled }, i) => {
                return (
                  <TableRow key={i}>
                    <TableCell component="th">
                      <strong>
                        {enabled ? SLOT_NAMES[i] : <del>{SLOT_NAMES[i]}</del>}
                      </strong>
                    </TableCell>
                    <TableCell>
                      {enabled ? (
                        id
                      ) : (
                        <del>{id.toString().padStart(3, "0")}</del>
                      )}
                    </TableCell>
                  </TableRow>
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
              value={modcards
                .filter(({ enabled }) => enabled)
                .flatMap(({ id }, i) => {
                  return [
                    `${SLOT_NAMES[i]}\t${id.toString().padStart(3, "0")}`,
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

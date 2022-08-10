import React from "react";

import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";

import { BN4ModcardsEditor } from "../../../saveedit";
import { CopyButtonWithLabel } from "../CopyButton";

const SLOT_NAMES = ["0A", "0B", "0C", "0D", "0E", "0F"];

const DEBUFF_COLOR = "#b55ade";
const BUFF_COLOR = "#ffbd18";
const OFF_COLOR = "#bdbdbd";

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
              {modcards.map((slot, i) => {
                const modcard =
                  slot != null ? editor.getModcardInfo(slot.id) : null;

                const formattedName =
                  slot != null ? (
                    <>
                      #{slot.id.toString().padStart(3, "0")}: {modcard!.name}
                      <br />
                      <small>{SLOT_NAMES[i]}</small>
                    </>
                  ) : (
                    <small>{SLOT_NAMES[i]}</small>
                  );

                return (
                  <TableRow key={i}>
                    <TableCell component="th">
                      {slot != null && slot.enabled ? (
                        formattedName
                      ) : (
                        <del>{formattedName}</del>
                      )}
                    </TableCell>
                    <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                      {slot != null ? (
                        <Stack spacing={0.5}>
                          <Chip
                            label={modcard!.effect}
                            size="small"
                            sx={{
                              fontSize: "0.9rem",
                              justifyContent: "flex-start",
                              color: "black",
                              backgroundColor: slot.enabled
                                ? BUFF_COLOR
                                : OFF_COLOR,
                            }}
                          />
                          <Chip
                            label={modcard!.bug}
                            size="small"
                            sx={{
                              fontSize: "0.9rem",
                              justifyContent: "flex-start",
                              color: "black",
                              backgroundColor: slot.enabled
                                ? DEBUFF_COLOR
                                : OFF_COLOR,
                            }}
                          />
                        </Stack>
                      ) : null}
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
                .map((modcard, i) => {
                  return `${SLOT_NAMES[i]}\t${
                    modcard != null && modcard.enabled
                      ? modcard.id.toString().padStart(3, "0")
                      : "---"
                  }`;
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

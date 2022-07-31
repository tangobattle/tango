import React from "react";

import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";

import { ModcardsEditor } from "../../../saveedit";
import { CopyButtonWithLabel } from "../CopyButton";

const DEBUFF_COLOR = "#b55ade";
const BUFF_COLOR = "#ffbd18";
const OFF_COLOR = "#bdbdbd";

export default function ModcardsViewer({
  editor,
  active,
}: {
  editor: ModcardsEditor;
  active: boolean;
}) {
  const modcards: { id: number; enabled: boolean }[] = [];
  for (let i = 0; i < editor.getModcardCount(); i++) {
    modcards.push(editor.getModcard(i)!);
  }

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          <Table size="small">
            <TableBody>
              {modcards.map(({ id, enabled }, i) => {
                const modcard = editor.getModcardInfo(id);
                if (modcard == null) {
                  return null;
                }

                const formattedName = (
                  <>
                    {modcard.name}
                    <br />
                    <small>{modcard.mb}MB</small>
                  </>
                );

                return (
                  <TableRow key={i}>
                    <TableCell>
                      {enabled ? formattedName : <del>{formattedName}</del>}
                    </TableCell>
                    <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                      <Stack spacing={0.5}>
                        {modcard.effects
                          .filter((e) => !e.isAbility)
                          .map((a) => (
                            <Chip
                              key={a.id}
                              label={a.name}
                              size="small"
                              sx={{
                                fontSize: "0.9rem",
                                justifyContent: "flex-start",
                                color: "black",
                                backgroundColor: enabled
                                  ? a.debuff
                                    ? DEBUFF_COLOR
                                    : BUFF_COLOR
                                  : OFF_COLOR,
                              }}
                            />
                          ))}
                      </Stack>
                    </TableCell>
                    <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                      <Stack spacing={0.5}>
                        {modcard.effects
                          .filter((e) => e.isAbility)
                          .map((a) => (
                            <Chip
                              key={a.id}
                              label={a.name}
                              size="small"
                              sx={{
                                fontSize: "0.9rem",
                                justifyContent: "flex-start",
                                color: "black",
                                backgroundColor: enabled
                                  ? a.debuff
                                    ? DEBUFF_COLOR
                                    : BUFF_COLOR
                                  : OFF_COLOR,
                              }}
                            />
                          ))}
                      </Stack>
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
                .flatMap(({ id }) => {
                  const modcard = editor.getModcardInfo(id);
                  if (modcard == null) {
                    return [];
                  }
                  return [`${modcard.name}\t${modcard.mb}`];
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

import { useTranslation } from "react-i18next";
import React from "react";
import Stack from "@mui/material/Stack";
import Chip from "@mui/material/Chip";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableRow from "@mui/material/TableRow";
import TableCell from "@mui/material/TableCell";
import * as bn6 from "../../saveedit/bn6";

const DEBUFF_COLOR = "#b55ade";
const BUFF_COLOR = "#ffbd18";
const OFF_COLOR = "#bdbdbd";

export function ModcardsViewer({ editor }: { editor: bn6.Editor }) {
  const { i18n } = useTranslation();

  const modcards: { id: number; enabled: boolean }[] = [];
  for (let i = 0; i < editor.getModcardCount(); i++) {
    modcards.push(editor.getModcard(i));
  }

  return (
    <Table size="small">
      <TableBody>
        {modcards.map(({ id, enabled }, i) => {
          const modcard = bn6.MODCARDS[id]!;
          return (
            <TableRow key={i}>
              <TableCell>
                {modcard.name[i18n.resolvedLanguage as "en" | "ja"]}{" "}
                <small>{modcard.mb}MB</small>
              </TableCell>
              <TableCell style={{ verticalAlign: "top" }}>
                <Stack spacing={0.5}>
                  {modcard.parameters.flatMap((l, i) =>
                    l.version == null ||
                    l.version == editor.getGameInfo().version
                      ? [
                          <Chip
                            key={i}
                            label={l.name[i18n.resolvedLanguage as "en" | "ja"]}
                            size="small"
                            sx={{
                              justifyContent: "flex-start",
                              backgroundColor: enabled
                                ? l.debuff
                                  ? DEBUFF_COLOR
                                  : BUFF_COLOR
                                : OFF_COLOR,
                            }}
                          />,
                        ]
                      : []
                  )}
                </Stack>
              </TableCell>
              <TableCell style={{ verticalAlign: "top" }}>
                <Stack spacing={0.5}>
                  {modcard.abilities.flatMap((l, i) =>
                    l.version == null ||
                    l.version == editor.getGameInfo().version
                      ? [
                          <Chip
                            key={i}
                            label={l.name[i18n.resolvedLanguage as "en" | "ja"]}
                            size="small"
                            sx={{
                              justifyContent: "flex-start",
                              backgroundColor: enabled
                                ? l.debuff
                                  ? DEBUFF_COLOR
                                  : BUFF_COLOR
                                : OFF_COLOR,
                            }}
                          />,
                        ]
                      : []
                  )}
                </Stack>
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}

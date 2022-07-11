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

import { ModcardsEditor } from "../../saveedit";
import { fallbackLng } from "../i18n";

const DEBUFF_COLOR = "#b55ade";
const BUFF_COLOR = "#ffbd18";
const OFF_COLOR = "#bdbdbd";

function gameVersion(romName: string) {
  switch (romName) {
    case "ROCKEXE6_RXXBR6J":
      return "falzar";
    case "ROCKEXE6_GXXBR5J":
      return "gregar";
  }
  throw `unknown rom name: ${romName}`;
}

export default function ModcardsViewer({
  editor,
  romName,
  active,
}: {
  editor: ModcardsEditor;
  romName: string;
  active: boolean;
}) {
  const { i18n } = useTranslation();

  const modcards: { id: number; enabled: boolean }[] = [];
  for (let i = 0; i < editor.getModcardCount(); i++) {
    modcards.push(editor.getModcard(i)!);
  }

  const modcardData = editor.getModcardData();

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          <Table size="small">
            <TableBody>
              {modcards.map(({ id, enabled }, i) => {
                const modcard = modcardData[id];
                if (modcard == null) {
                  return null;
                }

                return (
                  <TableRow key={i}>
                    <TableCell>
                      {modcard.name[
                        i18n.resolvedLanguage as keyof typeof modcard.name
                      ] ||
                        modcard.name[
                          fallbackLng as keyof typeof modcard.name
                        ]}{" "}
                      <small>{modcard.mb}MB</small>
                    </TableCell>
                    <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                      <Stack spacing={0.5}>
                        {modcard.parameters.flatMap((l, i) =>
                          l.version == null || l.version == gameVersion(romName)
                            ? [
                                <Chip
                                  key={i}
                                  label={
                                    l.name[
                                      i18n.resolvedLanguage as keyof typeof l.name
                                    ] ||
                                    l.name[fallbackLng as keyof typeof l.name]
                                  }
                                  size="small"
                                  sx={{
                                    fontSize: "0.9rem",
                                    justifyContent: "flex-start",
                                    color: "black",
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
                    <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                      <Stack spacing={0.5}>
                        {modcard.abilities.flatMap((l, i) =>
                          l.version == null || l.version == gameVersion(romName)
                            ? [
                                <Chip
                                  key={i}
                                  label={
                                    l.name[
                                      i18n.resolvedLanguage as keyof typeof l.name
                                    ] ||
                                    l.name[fallbackLng as keyof typeof l.name]
                                  }
                                  size="small"
                                  sx={{
                                    fontSize: "0.9rem",
                                    justifyContent: "flex-start",
                                    color: "black",
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
            <Button
              startIcon={<ContentCopyIcon />}
              onClick={() => {
                clipboard.writeText(
                  modcards
                    .filter(({ enabled }) => enabled)
                    .map(({ id }) => {
                      const modcardName =
                        modcardData[id]!.name[i18n.resolvedLanguage] ||
                        modcardData[id]!.name[fallbackLng];
                      return `${modcardName}\t${modcardData[id]!.mb}`;
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

import { sortBy } from "lodash-es";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import semver from "semver";

import { shell } from "@electron/remote";
import CloseIcon from "@mui/icons-material/Close";
import FolderOpenOutlinedIcon from "@mui/icons-material/FolderOpenOutlined";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Link from "@mui/material/Link";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { PatchInfo } from "../../patch";
import { FAMILY_BY_ROM_NAME, KNOWN_ROM_FAMILIES } from "../../rom";
import { fallbackLng } from "../i18n";
import { useConfig } from "./ConfigContext";

export default function PatchInfoDialog({
  name,
  patchInfo,
  onClose,
}: {
  name: string;
  patchInfo: PatchInfo;
  onClose: () => void;
}) {
  const { config } = useConfig();
  const { i18n } = useTranslation();

  const versions = semver.rsort(Object.keys(patchInfo.versions));

  return (
    <Stack
      key={name}
      sx={{
        width: "100%",
        height: "100%",
        bgcolor: "background.paper",
      }}
      direction="column"
    >
      <Stack direction="row" sx={{ pt: 1, px: 1, alignItems: "start" }}>
        <Box sx={{ width: "100%" }}>
          <Typography variant="h6" component="h2" sx={{ px: 1 }}>
            {patchInfo.title!} <small>{name}</small>
          </Typography>
        </Box>
        <Tooltip title={<Trans i18nKey="patches:open-directory" />}>
          <IconButton
            onClick={() => {
              shell.openPath(path.join(config.paths.patches, name));
            }}
          >
            <FolderOpenOutlinedIcon />
          </IconButton>
        </Tooltip>
        <Tooltip title={<Trans i18nKey="common:close" />}>
          <IconButton
            onClick={() => {
              onClose();
            }}
          >
            <CloseIcon />
          </IconButton>
        </Tooltip>
      </Stack>
      <Box
        flexGrow={1}
        sx={{
          display: "flex",
          flexDirection: "column",
          height: 0,
          overflow: "auto",
        }}
      >
        <Box sx={{ width: "100%" }}>
          <Table size="small">
            <TableBody>
              <TableRow>
                <TableCell
                  component="th"
                  sx={{
                    width: "0",
                    maxWidth: "100%",
                    whiteSpace: "nowrap",
                    verticalAlign: "top",
                    color: "text.secondary",
                  }}
                >
                  <Trans i18nKey="patches:authors" />
                </TableCell>
                <TableCell sx={{ verticalAlign: "top" }}>
                  <ul style={{ margin: 0, paddingLeft: "1em" }}>
                    {patchInfo.authors.map((a, i) => (
                      <li key={i}>
                        {a.email != null ? (
                          <Link href={`mailto:${a.email}`} target="_blank">
                            {a.name}
                          </Link>
                        ) : (
                          a.name
                        )}
                      </li>
                    ))}
                  </ul>
                </TableCell>
              </TableRow>
              <TableRow>
                <TableCell
                  component="th"
                  sx={{
                    width: "0",
                    maxWidth: "100%",
                    whiteSpace: "nowrap",
                    verticalAlign: "top",
                    color: "text.secondary",
                  }}
                >
                  <Trans i18nKey="patches:license" />
                </TableCell>
                <TableCell sx={{ verticalAlign: "top" }}>
                  {patchInfo.license != null ? (
                    <Link
                      href={`https://spdx.org/licenses/${patchInfo.license}.html`}
                      target="_blank"
                    >
                      {patchInfo.license}
                    </Link>
                  ) : (
                    <Trans i18nKey="patches:all-rights-reserved" />
                  )}
                </TableCell>
              </TableRow>
              {patchInfo.source != null ? (
                <TableRow>
                  <TableCell
                    component="th"
                    sx={{
                      width: "0",
                      maxWidth: "100%",
                      whiteSpace: "nowrap",
                      verticalAlign: "top",
                      color: "text.secondary",
                    }}
                  >
                    <Trans i18nKey="patches:source" />
                  </TableCell>
                  <TableCell sx={{ verticalAlign: "top" }}>
                    <Link
                      href={patchInfo.source}
                      target="_blank"
                      sx={{ wordWrap: "break-word" }}
                    >
                      {patchInfo.source}
                    </Link>
                  </TableCell>
                </TableRow>
              ) : null}
              <TableRow>
                <TableCell
                  component="th"
                  sx={{
                    width: "0",
                    maxWidth: "100%",
                    whiteSpace: "nowrap",
                    verticalAlign: "top",
                    color: "text.secondary",
                  }}
                >
                  <Trans i18nKey="patches:versions" />
                </TableCell>
                <TableCell sx={{ verticalAlign: "top" }}>
                  <ul style={{ margin: 0, paddingLeft: "1em" }}>
                    {versions.map((v) => (
                      <li key={v}>
                        {v}
                        <ul style={{ margin: 0, paddingLeft: "1em" }}>
                          {sortBy(patchInfo.versions[v].forROMs, (r) => {
                            const family =
                              KNOWN_ROM_FAMILIES[FAMILY_BY_ROM_NAME[r.name]];
                            const romInfo = family.versions[r.name];
                            return [
                              family.lang == i18n.resolvedLanguage ? 0 : 1,
                              family.lang,
                              FAMILY_BY_ROM_NAME[r.name],
                              romInfo.order,
                            ];
                          }).map((r, i) => (
                            <li key={i}>
                              {(() => {
                                const family =
                                  KNOWN_ROM_FAMILIES[
                                    FAMILY_BY_ROM_NAME[r.name]
                                  ];

                                const familyName =
                                  family.title[i18n.resolvedLanguage] ||
                                  family.title[fallbackLng];

                                const romTitle = family.versions[r.name].title;

                                if (romTitle == null) {
                                  return <>{familyName}</>;
                                }

                                return (
                                  <Trans
                                    i18nKey="play:rom-name"
                                    values={{
                                      familyName,
                                      versionName:
                                        romTitle[i18n.resolvedLanguage] ||
                                        romTitle[fallbackLng],
                                    }}
                                  />
                                );
                              })()}
                            </li>
                          ))}
                        </ul>
                      </li>
                    ))}
                  </ul>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </Box>
        <Box sx={{ px: 2 }}>
          <pre style={{ whiteSpace: "pre-wrap" }}>{patchInfo.readme}</pre>
        </Box>
      </Box>
    </Stack>
  );
}

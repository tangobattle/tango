import React from "react";
import { Trans, useTranslation } from "react-i18next";
import semver from "semver";

import HealingIcon from "@mui/icons-material/Healing";
import SyncIcon from "@mui/icons-material/Sync";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Fab from "@mui/material/Fab";
import List from "@mui/material/List";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import ListSubheader from "@mui/material/ListSubheader";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { KNOWN_ROMS } from "../../../rom";
import { fallbackLng } from "../../i18n";
import { useConfig } from "../ConfigContext";
import { usePatches } from "../PatchesContext";

export default function PatchesPane({ active }: { active: boolean }) {
  const { i18n } = useTranslation();

  const { patches, update } = usePatches();
  const { config } = useConfig();

  const [updating, setUpdating] = React.useState(false);

  const groupedPatches: { [key: string]: string[] } = {};
  for (const k of Object.keys(patches)) {
    groupedPatches[patches[k].forROM] = groupedPatches[patches[k].forROM] || [];
    groupedPatches[patches[k].forROM].push(k);
  }

  const romNames = Object.keys(groupedPatches);
  romNames.sort((k1, k2) => {
    const title1 =
      KNOWN_ROMS[k1].title[i18n.resolvedLanguage] ||
      KNOWN_ROMS[k1].title[fallbackLng];
    const title2 =
      KNOWN_ROMS[k2].title[i18n.resolvedLanguage] ||
      KNOWN_ROMS[k2].title[fallbackLng];
    return title1 < title2 ? -1 : title1 > title2 ? 1 : 0;
  });

  const listFormatter = new Intl.ListFormat(i18n.resolvedLanguage, {
    style: "long",
    type: "conjunction",
  });

  return (
    <Box
      sx={{
        position: "relative",
        width: "100%",
        height: "100%",
        display: active ? "flex" : "none",
      }}
    >
      {romNames.length > 0 ? (
        <>
          <List dense>
            {romNames.map((romName) => (
              <React.Fragment key={romName}>
                <ListSubheader>
                  {KNOWN_ROMS[romName].title[i18n.resolvedLanguage] ||
                    KNOWN_ROMS[romName].title[fallbackLng]}
                </ListSubheader>
                {groupedPatches[romName].sort().map((patchName) => (
                  <ListItem key={patchName} sx={{ userSelect: "none" }}>
                    <ListItemText
                      primary={patches[patchName].title}
                      primaryTypographyProps={{
                        sx: {
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        },
                      }}
                      secondary={
                        <Trans
                          i18nKey="patches:byline"
                          values={{
                            version: semver.maxSatisfying(
                              Object.keys(patches[patchName].versions),
                              "*"
                            ),
                            authors: listFormatter.format(
                              patches[patchName].authors.flatMap(({ name }) =>
                                name != null ? [name] : []
                              )
                            ),
                          }}
                        />
                      }
                      secondaryTypographyProps={{
                        sx: {
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        },
                      }}
                    />
                  </ListItem>
                ))}
              </React.Fragment>
            ))}
          </List>
          <Tooltip title={<Trans i18nKey="patches:update" />}>
            <Fab
              color="primary"
              sx={{
                position: "fixed",
                bottom: "16px",
                right: "16px",
                animation: updating ? "spin 2s linear infinite" : null,
                "@keyframes spin": {
                  "0%": {
                    transform: "rotate(360deg)",
                  },
                  "100%": {
                    transform: "rotate(0deg)",
                  },
                },
              }}
              disabled={updating}
              onClick={() => {
                (async () => {
                  try {
                    setUpdating(true);
                    await update(config.patchRepo);
                  } catch (e) {
                    console.error(e);
                  } finally {
                    setUpdating(false);
                  }
                })();
              }}
            >
              <SyncIcon />
            </Fab>
          </Tooltip>
        </>
      ) : (
        <Box
          flexGrow={1}
          display="flex"
          justifyContent="center"
          alignItems="center"
          sx={{ userSelect: "none", color: "text.disabled" }}
        >
          <Stack alignItems="center" spacing={1}>
            <HealingIcon sx={{ fontSize: "4rem" }} />
            <Typography variant="h6">
              <Trans i18nKey="patches:no-patches" />
            </Typography>
            <Button
              disabled={updating}
              startIcon={
                updating ? (
                  <CircularProgress color="inherit" size="1em" />
                ) : null
              }
              variant="contained"
              onClick={() => {
                (async () => {
                  try {
                    setUpdating(true);
                    await update(config.patchRepo);
                  } catch (e) {
                    console.error(e);
                  } finally {
                    setUpdating(false);
                  }
                })();
              }}
            >
              <Trans i18nKey="patches:update" />
            </Button>
          </Stack>
        </Box>
      )}
    </Box>
  );
}

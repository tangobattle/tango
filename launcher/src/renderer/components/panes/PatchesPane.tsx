import React from "react";
import { Trans, useTranslation } from "react-i18next";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import HealingIcon from "@mui/icons-material/Healing";
import SyncIcon from "@mui/icons-material/Sync";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Fab from "@mui/material/Fab";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { PatchInfo } from "../../../patch";
import { useConfig } from "../ConfigContext";
import { usePatches } from "../PatchesContext";

function PatchItem({
  ListChildProps: { style },
  patchName,
  patch,
}: {
  ListChildProps: ListChildComponentProps;
  patchName: string;
  patch: PatchInfo;
}) {
  const { i18n } = useTranslation();

  const listFormatter = new Intl.ListFormat(i18n.resolvedLanguage, {
    style: "long",
    type: "conjunction",
  });

  return (
    <ListItem style={style} key={patchName} sx={{ userSelect: "none" }}>
      <ListItemText
        primary={patch.title}
        primaryTypographyProps={{
          sx: {
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          },
        }}
        secondary={
          <Trans
            i18nKey="play:patch-byline"
            values={{
              authors: listFormatter.format(
                patch.authors.flatMap(({ name }) =>
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
  );
}

export default function PatchesPane({ active }: { active: boolean }) {
  const { patches, update } = usePatches();
  const { config } = useConfig();

  const [updating, setUpdating] = React.useState(false);

  const patchNames = Object.keys(patches);
  patchNames.sort();

  return (
    <Box
      sx={{
        position: "relative",
        width: "100%",
        height: "100%",
        display: active ? "flex" : "none",
      }}
    >
      {patchNames.length > 0 ? (
        <>
          <AutoSizer>
            {({ height, width }) => (
              <FixedSizeList
                height={height}
                width={width}
                itemCount={patchNames.length}
                itemSize={60}
              >
                {(props) => (
                  <PatchItem
                    ListChildProps={props}
                    patchName={patchNames[props.index]}
                    patch={patches[patchNames[props.index]]}
                  />
                )}
              </FixedSizeList>
            )}
          </AutoSizer>
          <Tooltip title={<Trans i18nKey="patches:update" />}>
            <Fab
              color="primary"
              sx={{
                position: "absolute",
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

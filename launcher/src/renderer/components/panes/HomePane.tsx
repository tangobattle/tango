import React from "react";
import { Trans } from "react-i18next";
import { TwitterTimelineEmbed } from "react-twitter-embed";

import AddIcon from "@mui/icons-material/Add";
import ArrowForwardIcon from "@mui/icons-material/ArrowForward";
import SportsMmaOutlinedIcon from "@mui/icons-material/SportsMmaOutlined";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Stack from "@mui/material/Stack";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import useTheme from "@mui/system/useTheme";

import * as lobby from "../../../lobby";
import { useConfig } from "../ConfigContext";

function useCreateLobby() {
  const { config } = useConfig();

  return (
    gameInfo: lobby.GameInfo,
    availablePatches: lobby.Patch[],
    settings: lobby.Settings,
    saveData: Uint8Array,
    options: { signal?: AbortSignal } = {}
  ) => {
    return lobby.create(
      `ws${!config.lobby.insecure ? "s" : ""}://${config.lobby.address}/lobby`,
      "TODO",
      gameInfo,
      availablePatches,
      settings,
      saveData,
      options
    );
  };
}

export default function HomePane({ active }: { active: boolean }) {
  const { config } = useConfig();
  const theme = useTheme();

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        py: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Box flexGrow={1}>
        <Box
          sx={{
            width: "100%",
            height: "100%",
            display: active ? "flex" : "none",
          }}
        >
          <Box
            flexGrow={1}
            display="flex"
            justifyContent="center"
            alignItems="center"
          >
            <Stack alignItems="center" spacing={2} sx={{ width: "60%" }}>
              <Button
                fullWidth
                color="primary"
                size="medium"
                variant="contained"
                onClick={() => {}}
                startIcon={<AddIcon />}
              >
                <Trans i18nKey="home:create-lobby" />
              </Button>
              <Stack direction="row" spacing={1} sx={{ width: "100%" }}>
                <TextField
                  size="small"
                  id="outlined-basic"
                  label="Outlined"
                  variant="outlined"
                  sx={{ flexGrow: 1 }}
                />

                <Button
                  color="primary"
                  size="medium"
                  variant="contained"
                  onClick={() => {}}
                  startIcon={<ArrowForwardIcon />}
                >
                  <Trans i18nKey="home:join-lobby" />
                </Button>
              </Stack>
            </Stack>
          </Box>
        </Box>
      </Box>
    </Box>
  );
}

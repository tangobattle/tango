import React from "react";
import { Trans } from "react-i18next";

import AddIcon from "@mui/icons-material/Add";
import SportsMmaOutlinedIcon from "@mui/icons-material/SportsMmaOutlined";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";

import * as lobby from "../../../lobby";
import { useConfig } from "../ConfigContext";

function useCreateLobby() {
  const { config } = useConfig();

  return (
    gameInfo: lobby.GameInfo,
    settings: lobby.Settings,
    saveData: Uint8Array,
    options: { signal?: AbortSignal } = {}
  ) => {
    return lobby.create(
      `ws${!config.lobby.insecure ? "s" : ""}://${config.lobby.address}/lobby`,
      "TODO",
      gameInfo,
      settings,
      saveData,
      options
    );
  };
}

export default function BattlePane({ active }: { active: boolean }) {
  const createLobby = useCreateLobby();

  const lobbies = [];

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
          {lobbies.length > 0 ? (
            <Stack sx={{ flexGrow: 1, width: 0 }} spacing={1}>
              <Box flexGrow={1} flexShrink={0}></Box>
              <Stack sx={{ px: 1 }}>
                <Box sx={{ alignSelf: "flex-end" }}>
                  <Button
                    color="primary"
                    size="medium"
                    variant="contained"
                    onClick={() => {}}
                    startIcon={<AddIcon />}
                  >
                    <Trans i18nKey="battle:create-lobby" />
                  </Button>
                </Box>
              </Stack>
            </Stack>
          ) : (
            <Box
              flexGrow={1}
              display="flex"
              justifyContent="center"
              alignItems="center"
              sx={{ userSelect: "none", color: "text.disabled" }}
            >
              <Stack alignItems="center" spacing={1}>
                <SportsMmaOutlinedIcon sx={{ fontSize: "4rem" }} />
                <Typography variant="h6">
                  <Trans i18nKey="battle:no-lobbies" />
                </Typography>
                <Button
                  color="primary"
                  size="medium"
                  variant="contained"
                  onClick={() => {}}
                  startIcon={<AddIcon />}
                >
                  <Trans i18nKey="battle:placeholder-create-lobby" />
                </Button>
              </Stack>
            </Box>
          )}
        </Box>
      </Box>
    </Box>
  );
}

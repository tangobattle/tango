import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { app } from "@electron/remote";
import SyncIcon from "@mui/icons-material/Sync";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";

import { getPatchesPath } from "../../../paths";
import { useConfig } from "../ConfigContext";
import { usePatches } from "../PatchesContext";

export default function PatchesPane({ active }: { active: boolean }) {
  const { patches, update } = usePatches();
  const { config } = useConfig();

  const [updating, setUpdating] = React.useState(false);

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        display: active ? "flex" : "none",
      }}
    >
      <Stack sx={{ flexGrow: 1 }}>
        <Stack direction="row" sx={{ px: 1, py: 1 }}>
          <Tooltip title={<Trans i18nKey="patches:update" />}>
            <IconButton
              sx={{
                marginLeft: "auto",
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
                    await update(getPatchesPath(app), config.patchRepo);
                  } catch (e) {
                    console.error(e);
                  } finally {
                    setUpdating(false);
                  }
                })();
              }}
            >
              <SyncIcon />
            </IconButton>
          </Tooltip>
        </Stack>
      </Stack>
    </Box>
  );
}

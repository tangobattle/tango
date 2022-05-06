import React from "react";
import { Trans } from "react-i18next";

import AddIcon from "@mui/icons-material/Add";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Stack from "@mui/material/Stack";

export default function BattlePane({ active }: { active: boolean }) {
  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        py: 1,
        display: active ? "flex" : "none",
      }}
    >
      <Stack sx={{ flexGrow: 1, width: 0 }} spacing={1}>
        <Box flexGrow={1} sx={{ px: 1 }}></Box>

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
    </Box>
  );
}

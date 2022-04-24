import React from "react";

import Box from "@mui/material/Box";

export default function SettingsPane({ active }: { active: boolean }) {
  return (
    <Box
      sx={{
        my: 1,
        flexGrow: 1,
        display: active ? "flex" : "none",
      }}
    >
      TODO
    </Box>
  );
}

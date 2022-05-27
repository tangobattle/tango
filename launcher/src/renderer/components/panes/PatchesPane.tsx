import React from "react";

import Box from "@mui/material/Box";

import { usePatches } from "../PatchesContext";

export default function PatchesPane({ active }: { active: boolean }) {
  const { patches } = usePatches();

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        display: active ? "flex" : "none",
      }}
    >
      TODO
    </Box>
  );
}

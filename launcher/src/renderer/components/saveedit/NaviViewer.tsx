import React from "react";

import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";

import { NaviEditor } from "../../../saveedit";

export default function NaviViewer({
  editor,
  active,
}: {
  editor: NaviEditor;
  active: boolean;
}) {
  const naviInfo = editor.getNaviInfo(editor.getNavi());
  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          {naviInfo.name}
        </Box>
      </Stack>
    </Box>
  );
}
